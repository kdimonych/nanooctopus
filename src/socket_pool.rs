use abstarct_socket::{
    AbstractSocketBuilder,
    socket::{
        SocketAccept, SocketClose, SocketConfig, SocketEndpoint, SocketInfo, SocketWaitReadReady, SocketWaitWriteReady,
        SocketWrite, State,
    },
};
use core::pin::pin;
use defmt_or_log as log;
use embassy_futures::select::*;
use heapless::spsc::Queue;

#[cfg(feature = "embassy_impl")]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(feature = "embassy_impl")]
use embassy_sync::rwlock::{RwLock, RwLockWriteGuard};

#[cfg(feature = "std")]
use std::sync::RwLock;

const KEEP_ALIVE_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(3);
const SOCKET_IO_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(5);

#[cfg(feature = "embassy_impl")]
struct RwLockWrapper<T>(RwLock<NoopRawMutex, T>);
#[cfg(feature = "std")]
struct RwLockWrapper<T>(RwLock<T>);

impl<T> RwLockWrapper<T> {
    pub const fn new(socket: T) -> Self {
        Self(RwLock::new(socket))
    }

    #[cfg(feature = "embassy_impl")]
    pub async fn write(&self) -> RwLockWriteGuard<'_, NoopRawMutex, T> {
        self.0.write().await
    }

    #[cfg(feature = "std")]
    pub async fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
        self.0.write().unwrap()
    }
}

#[cfg(feature = "embassy_impl")]
pub type SocketRef<'pool, Socket> = RwLockWriteGuard<'pool, NoopRawMutex, Socket>;
#[cfg(feature = "std")]
pub type SocketRef<'pool, Socket> = std::sync::RwLockWriteGuard<'pool, Socket>;

pub struct SocketPool<const POOL_SIZE: usize, Socket> {
    sockets: [RwLockWrapper<Socket>; POOL_SIZE],
    ready: RwLockWrapper<Queue<usize, POOL_SIZE>>,
    endpoint: SocketEndpoint,
}

#[allow(dead_code)]
impl<Socket, const POOL_SIZE: usize> SocketPool<POOL_SIZE, Socket> {
    /// Create a new SocketPool
    /// ### Arguments
    /// - `socket_builder`: A mutable reference to a socket builder that will be used to create the sockets in the pool. The builder must implement the `AbstractSocketBuilder` trait for the specific socket type and lifetime.
    /// - `endpoint`: The socket endpoint (e.g., port number) that the sockets in the pool will listen on.
    /// ### Returns
    /// A new instance of `SocketPool` with the specified configuration. The pool will contain `POOL_SIZE` sockets, each initialized using the provided `socket_builder` and configured to listen on the specified `endpoint`.
    /// ### Panics
    /// Panics if the socket builder fails to create a socket with the required buffer configuration.
    /// Panics if the provided `POOL_SIZE` is zero.
    pub fn new(socket_builder: &mut impl AbstractSocketBuilder<Socket = Socket>, endpoint: SocketEndpoint) -> Self
    where
        Socket: SocketConfig,
    {
        log::assert!(POOL_SIZE > 0, "Socket pool size must be greater than zero");
        Self {
            sockets: core::array::from_fn::<_, POOL_SIZE, _>(|_| {
                let mut socket = socket_builder.build().expect("Failed to build socket with buffer");

                // Set keep alive options (This must be set to prevent connections from being closed by NATs)
                socket.set_keep_alive(Some(KEEP_ALIVE_TIMEOUT));
                // This must be set to prevent eternal pending on IO operations
                socket.set_timeout(Some(SOCKET_IO_TIMEOUT));

                RwLockWrapper::new(socket)
            }),
            ready: RwLockWrapper::new(Queue::new()),
            endpoint,
        }
    }

    /// Accepts the next incoming connection or/and wait until data is available on it then return the socket.
    ///
    /// This method polls all sockets in a loop until one is ready
    /// (ready means has established stated and data is available for reading).
    /// All ready sockets are enqueued into the provided `ready` queue.
    ///
    pub async fn acquire_next_request(&self) -> SocketRef<'_, Socket>
    where
        Socket: SocketInfo + SocketWrite + SocketWaitReadReady + SocketWaitWriteReady + SocketAccept + SocketClose,
    {
        let sockets = &self.sockets;

        {
            let endpoint = self.endpoint;
            let pending_sockets = pin!(core::array::from_fn::<_, POOL_SIZE, _>(|idx| accept(
                &sockets[idx],
                endpoint
            )));
            let (socket, ready_idx) = select_slice(pending_sockets).await;

            // Put the index of ready socket into the ready queue so that it can be picked up by the next acquire_next_request call if needed
            self.ready.write().await.enqueue(ready_idx).unwrap();

            // Note: this line is crucial. It keeps the socket locked until the socket is enqueued
            // into the ready queue. This ensures that the socket is not released before it is marked
            // as ready, which could lead to a race condition where the socket is released and acquired
            // by another task before it is marked as ready, causing the first task to wait indefinitely
            // for a socket that is already in use.
            let end_point = socket.remote_endpoint();

            log::info!(
                "SocketPool: Socket[{:?}] {:?} is ready on port {}",
                ready_idx,
                end_point,
                endpoint
            );
        }

        // Get the next ready socket index from the ready queue and return the socket reference
        let idx = self.ready.write().await.dequeue().unwrap();
        sockets[idx].write().await
    }

    /// Get the capacity of the socket pool
    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        POOL_SIZE
    }

    #[inline(always)]
    pub fn endpoint(&self) -> SocketEndpoint {
        self.endpoint
    }
}

async fn accept<'stack, Socket>(
    socket: &'stack RwLockWrapper<Socket>,
    endpoint: SocketEndpoint,
) -> SocketRef<'stack, Socket>
where
    Socket: SocketInfo + SocketWrite + SocketWaitReadReady + SocketWaitWriteReady + SocketAccept + SocketClose,
{
    let mut socket = socket.write().await;
    log::debug!(
        "SocketPool: Socket {:?} released. Current state: {:?}",
        socket.remote_endpoint(),
        socket.state()
    );
    while wait_for_socket_ready(&mut socket, endpoint).await.is_err() {}

    socket
}

async fn wait_for_socket_ready<'stack, Socket>(
    socket: &mut SocketRef<'stack, Socket>,
    endpoint: SocketEndpoint,
) -> Result<(), ()>
where
    Socket: SocketInfo + SocketWrite + SocketWaitReadReady + SocketWaitWriteReady + SocketAccept + SocketClose,
{
    loop {
        log::debug!(
            "SocketPool: Waiting for socket {:?} to be ready. Current state: {:?}",
            socket.remote_endpoint(),
            socket.state()
        );
        match socket.state() {
            State::Established | State::SynSent | State::SynReceived => {
                socket.flush().await.map_err(|e| {
                    log::error!("SocketPool: Error while flushing socket: {:?}", log::Debug2Format(&e));
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::Closed | State::Listen => {
                socket.accept(endpoint).await.map_err(|e| {
                    log::error!(
                        "SocketPool: Error while accepting connection: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::TimeWait | State::FinWait1 | State::Closing | State::LastAck | State::CloseWait => {
                // The socket is in a state where it is either closing or waiting for the remote to close,
                // we need to gracefully bring it down first before accepting a new connection on it.
                socket.close().await.map_err(|e| {
                    log::error!("SocketPool: Error while closing socket: {:?}", log::Debug2Format(&e));
                })?;
                socket.flush().await.map_err(|e| {
                    log::error!("SocketPool: Error while flushing socket: {:?}", log::Debug2Format(&e));
                })?;
                socket.accept(endpoint).await.map_err(|e| {
                    log::error!(
                        "SocketPool: Error while accepting connection: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::FinWait2 => {
                // The socket is waiting for the remote to close, we need to wait for it to be writable before accepting a new connection on it.
                socket.wait_write_ready().await;
            }
        }
    }
}
