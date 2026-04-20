use embassy_net::{
    Stack,
    tcp::{State, TcpSocket},
};

use core::pin::pin;
use defmt_or_log as log;
use embassy_futures::select::*;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::rwlock::{RwLock, RwLockWriteGuard};
use heapless::spsc::Queue;

const KEEP_ALIVE_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(3);
const SOCKET_IO_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(5);

type GuardedSocket<'tcp_stack> = RwLock<NoopRawMutex, TcpSocket<'tcp_stack>>;
pub type SocketRef<'pool, 'tcp_stack> = RwLockWriteGuard<'pool, NoopRawMutex, TcpSocket<'tcp_stack>>;

/// Type alias for socket buffers
pub struct SocketBuffers<const RX_SIZE: usize, const TX_SIZE: usize> {
    rx_buffer: [u8; RX_SIZE],
    tx_buffer: [u8; TX_SIZE],
}

impl<const RX_SIZE: usize, const TX_SIZE: usize> SocketBuffers<RX_SIZE, TX_SIZE> {
    /// Create new socket buffers
    pub const fn new() -> Self {
        Self {
            rx_buffer: [0; RX_SIZE],
            tx_buffer: [0; TX_SIZE],
        }
    }
}

impl<const RX_SIZE: usize, const TX_SIZE: usize> Default for SocketBuffers<RX_SIZE, TX_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SocketPoolBuilder {
    port: u16,
    keep_alive_timeout: embassy_time::Duration,
    socket_io_timeout: embassy_time::Duration,
}

impl SocketPoolBuilder {
    pub const fn new(port: u16) -> Self {
        Self {
            port,
            keep_alive_timeout: KEEP_ALIVE_TIMEOUT,
            socket_io_timeout: SOCKET_IO_TIMEOUT,
        }
    }

    pub const fn with_keep_alive_timeout(mut self, timeout: embassy_time::Duration) -> Self {
        self.keep_alive_timeout = timeout;
        self
    }

    pub const fn with_socket_io_timeout(mut self, timeout: embassy_time::Duration) -> Self {
        self.socket_io_timeout = timeout;
        self
    }

    pub fn build<'socket, 'tcp_stack, const POOL_SIZE: usize, const RX_SIZE: usize, const TX_SIZE: usize>(
        &self,
        buffers: &'socket mut [SocketBuffers<RX_SIZE, TX_SIZE>; POOL_SIZE],
        stack: Stack<'tcp_stack>,
    ) -> SocketPool<'tcp_stack, POOL_SIZE>
    where
        'socket: 'tcp_stack,
    {
        SocketPool::new(buffers, stack, self.port)
    }
}

pub struct SocketPool<'tcp_stack, const POOL_SIZE: usize> {
    sockets: [GuardedSocket<'tcp_stack>; POOL_SIZE],
    ready: RwLock<NoopRawMutex, Queue<usize, POOL_SIZE>>,
    port: u16,
}

#[allow(dead_code)]
impl<'tcp_stack, const POOL_SIZE: usize> SocketPool<'tcp_stack, POOL_SIZE> {
    /// Create a new SocketPool
    fn new<const RX_SIZE: usize, const TX_SIZE: usize>(
        buffers: &'tcp_stack mut [SocketBuffers<RX_SIZE, TX_SIZE>; POOL_SIZE],
        stack: Stack<'tcp_stack>,
        port: u16,
    ) -> Self {
        let mut it = buffers.iter_mut();

        Self {
            sockets: core::array::from_fn::<_, POOL_SIZE, _>(|_| {
                let buffer = unsafe { it.next().unwrap_unchecked() };
                let mut socket = TcpSocket::new(
                    stack,
                    // SAFETY: We have exactly POOL_SIZE buffers
                    &mut buffer.rx_buffer,
                    &mut buffer.tx_buffer,
                );

                // Set keep alive options (This must be set to prevent connections from being closed by NATs)
                socket.set_keep_alive(Some(KEEP_ALIVE_TIMEOUT));
                // This must be set to prevent eternal pending on IO operations
                socket.set_timeout(Some(SOCKET_IO_TIMEOUT));

                log::trace!(
                    "SocketPool: Created socket with RX size {} and TX size {}",
                    RX_SIZE,
                    TX_SIZE
                );
                RwLock::new(socket)
            }),
            ready: RwLock::new(Queue::new()),
            port,
        }
    }

    /// Accepts the next incoming connection or/and wait until data is available on it then return the socket.
    ///
    /// This method polls all sockets in a loop until one is ready
    /// (ready means has established stated and data is available for reading).
    /// All ready sockets are enqueued into the provided `ready` queue.
    ///
    pub async fn acquire_next_request(&self) -> SocketRef<'_, 'tcp_stack> {
        let sockets = &self.sockets;

        {
            let port = self.port;
            let pending_sockets = pin!(core::array::from_fn::<_, POOL_SIZE, _>(|idx| accept(
                &sockets[idx],
                port
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
                port
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
    pub fn port(&self) -> u16 {
        self.port
    }
}

async fn accept<'socket, 'stack>(socket: &'socket GuardedSocket<'stack>, port: u16) -> SocketRef<'socket, 'stack> {
    let mut socket = socket.write().await;
    log::debug!(
        "SocketPool: Socket {:?} released. Current state: {:?}",
        socket.remote_endpoint(),
        socket.state()
    );
    while wait_for_socket_ready(&mut socket, port).await.is_err() {}

    socket
}

async fn wait_for_socket_ready<'socket, 'stack>(socket: &mut SocketRef<'socket, 'stack>, port: u16) -> Result<(), ()> {
    loop {
        log::debug!(
            "SocketPool: Waiting for socket {:?} to be ready. Current state: {:?}",
            socket.remote_endpoint(),
            socket.state()
        );
        match socket.state() {
            State::Established | State::SynSent | State::SynReceived => {
                socket.flush().await.map_err(|e| {
                    log::error!("SocketPool: Error while flushing socket: {:?}", e);
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::Closed | State::Listen => {
                socket.accept(port).await.map_err(|e| {
                    log::error!("SocketPool: Error while accepting connection: {:?}", e);
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::TimeWait | State::FinWait1 | State::Closing | State::LastAck | State::CloseWait => {
                // The socket is in a state where it is either closing or waiting for the remote to close,
                // we need to gracefully bring it down first before accepting a new connection on it.
                socket.close();
                socket.flush().await.map_err(|e| {
                    log::error!("SocketPool: Error while flushing socket: {:?}", e);
                })?;
                socket.accept(port).await.map_err(|e| {
                    log::error!("SocketPool: Error while accepting connection: {:?}", e);
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
