use embassy_net::{
    Stack,
    tcp::{AcceptError, Error, State, TcpSocket},
};

use core::future::Future;
use core::future::poll_fn;
use core::pin::pin;
use core::task::Context;
use core::task::Poll;
use defmt_or_log as log;
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

#[defmt_or_log::derive_format_or_debug]
pub enum SocketPoolError {
    AcceptError(AcceptError),
    IOError(Error),
}

pub struct RoundRobinSocketPoolBuilder {
    port: u16,
    keep_alive_timeout: embassy_time::Duration,
    socket_io_timeout: embassy_time::Duration,
}

impl RoundRobinSocketPoolBuilder {
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
    pub async fn acquire_next_request<'buffer>(&self) -> SocketRef<'_, 'tcp_stack> {
        let sockets = &self.sockets;
        let mut ready = self.ready.write().await;
        let port = self.port;

        poll_fn(|cx| -> Poll<SocketRef<'_, '_>> {
            sockets.iter().enumerate().for_each(|(idx, s_lock)| {
                if let Ok(mut socket) = s_lock.try_write() {
                    if ready.iter().find(|n| **n == idx).is_some() {
                        // This socket is already marked as ready, skip it
                        return;
                    }
                    match socket.poll_wait_next_request_once(cx, port) {
                        Poll::Ready(Ok(())) => {
                            ready.enqueue(idx).ok();
                        }
                        Poll::Ready(Err(error)) => {
                            log::error!("SocketPool: Error while polling socket: {:?}", error);
                        }
                        Poll::Pending => {}
                    };
                }
            });

            ready
                .dequeue()
                .map_or(Poll::Pending, |idx| Poll::Ready(sockets[idx].try_write().unwrap())) // SAFETY: We have at most POOL_SIZE sockets and we only enqueue indices of ready sockets, so this is safe
        })
        .await
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

trait PollSocket {
    fn poll_accept_once(&mut self, cx: &mut Context<'_>, port: u16) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_read_ready_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_write_ready_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>>;
    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_next_request_once(&mut self, cx: &mut Context<'_>, port: u16) -> Poll<Result<(), SocketPoolError>>;
}

impl PollSocket for TcpSocket<'_> {
    fn poll_accept_once(&mut self, cx: &mut Context<'_>, port: u16) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.accept(port))
            .as_mut()
            .poll(cx)
            .map(|res| res.map_err(|e| SocketPoolError::AcceptError(e)))
    }

    fn poll_wait_read_ready_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.wait_read_ready()).as_mut().poll(cx).map(|_| Ok(()))
    }

    fn poll_wait_write_ready_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.wait_write_ready()).as_mut().poll(cx).map(|_| Ok(()))
    }

    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.flush())
            .as_mut()
            .poll(cx)
            .map(|res| res.map_err(|e| SocketPoolError::IOError(e)))
    }

    fn poll_wait_next_request_once(&mut self, cx: &mut Context<'_>, port: u16) -> Poll<Result<(), SocketPoolError>> {
        log::trace!(
            "SocketPool: Socket {:?} in state {:?}",
            self.remote_endpoint(),
            self.state()
        );
        match self.state() {
            State::Established | State::SynSent | State::SynReceived => {
                log::trace!(
                    "SocketPool: Wait for request at socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                return self.poll_wait_read_ready_once(cx);
            }

            State::Closed | State::Listen => {
                // In this case we can safelly poll just accept
                log::trace!(
                    "SocketPool: Accept new connection at socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                match self.poll_accept_once(cx, port) {
                    Poll::Ready(Ok(())) => {
                        log::debug!("SocketPool: New connection {:?} at socket", self.remote_endpoint());
                        self.poll_wait_read_ready_once(cx)
                    } // We got a new connection, start waiting for data
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }

            State::TimeWait | State::FinWait1 | State::Closing | State::LastAck | State::CloseWait => {
                // In this case we have to gracefully bring the socket down first.
                log::trace!(
                    "SocketPool: Close socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                // Close the socket and accept a new connection
                self.close();
                log::debug!(
                    "SocketPool: Closed socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                // Wait for previous operations to flush
                match self.poll_flush_once(cx) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(e)) => {
                        log::error!(
                            "SocketPool: flush error for socket {:?} in state {:?}",
                            self.remote_endpoint(),
                            self.state()
                        );
                        return Poll::Ready(Err(e));
                    }
                    Poll::Pending => return Poll::Pending,
                }
                log::trace!(
                    "SocketPool: Accept new connection at socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                // Previous operations succeeded, accept new connection on this socket
                match self.poll_accept_once(cx, port) {
                    Poll::Ready(Ok(())) => {
                        log::debug!(
                            "SocketPool: New connection at socket {:?} in state {:?}",
                            self.remote_endpoint(),
                            self.state()
                        );
                        self.poll_wait_read_ready_once(cx)
                    } // We got a new connection, start waiting for data
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }

            // In the FinWait2 state this function will always return Pending, hence setting up a waker to be woken later if
            // state changed (I hope so)
            State::FinWait2 => {
                log::debug!(
                    "SocketPool: Wait at socket {:?} in state {:?} to be closed by remote",
                    self.remote_endpoint(),
                    self.state()
                );
                self.poll_wait_write_ready_once(cx)
            }
        }
    }
}
