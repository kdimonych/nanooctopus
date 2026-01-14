use embassy_net::{
    Stack,
    tcp::{AcceptError, Error, State, TcpSocket},
};

use core::cell::{RefCell, RefMut};
use core::future::Future;
use core::future::poll_fn;
use core::pin::pin;
use core::task::Context;
use core::task::Poll;
use heapless::spsc::Queue;

const KEEP_ALIVE_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(3);
const SOCKET_IO_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(5);

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

#[derive(Debug, defmt::Format)]
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

    pub fn build<'a, const POOL_SIZE: usize, const RX_SIZE: usize, const TX_SIZE: usize>(
        &self,
        buffers: &'a mut [SocketBuffers<RX_SIZE, TX_SIZE>; POOL_SIZE],
        stack: Stack<'a>,
    ) -> RoundRobinSocketPool<'a, POOL_SIZE> {
        RoundRobinSocketPool::new(buffers, stack, self.port)
    }
}

pub struct RoundRobinSocketPool<'a, const POOL_SIZE: usize> {
    sockets: [RefCell<TcpSocket<'a>>; POOL_SIZE],
    ready: Queue<RefMut<'a, TcpSocket<'a>>, POOL_SIZE>,
    port: u16,
}

impl<'a, const POOL_SIZE: usize> RoundRobinSocketPool<'a, POOL_SIZE> {
    /// Create a new SocketPool
    fn new<const RX_SIZE: usize, const TX_SIZE: usize>(
        buffers: &'a mut [SocketBuffers<RX_SIZE, TX_SIZE>; POOL_SIZE],
        stack: Stack<'a>,
        port: u16,
    ) -> Self {
        let mut it = buffers.iter_mut();

        Self {
            sockets: core::array::from_fn::<_, POOL_SIZE, _>(|_| {
                let mut socket = TcpSocket::new(
                    stack,
                    // SAFETY: We have exactly POOL_SIZE buffers
                    unsafe { &mut it.next().unwrap_unchecked().rx_buffer },
                    unsafe { &mut it.next().unwrap_unchecked().tx_buffer },
                );

                // Set keep alive options (This must be set to prevent connections from being closed by NATs)
                socket.set_keep_alive(Some(KEEP_ALIVE_TIMEOUT));
                // This must be set to prevent eternal pending on IO operations
                socket.set_timeout(Some(SOCKET_IO_TIMEOUT));

                RefCell::new(socket)
            }),
            ready: Queue::new(),
            port,
        }
    }

    /// Accept the next incoming connection and wait until data is available.
    /// This function works as round-robin over the sockets in the pool.
    pub async fn acquire_next(&'a mut self) -> Option<RefMut<'a, TcpSocket<'a>>> {
        Self::collect_ready(&self.sockets, self.port, &mut self.ready).await;
        self.ready.dequeue()
    }

    /// Get the number of ready sockets
    pub fn ready(&self) -> usize {
        self.ready.len()
    }

    /// Get the capacity of the socket pool
    pub const fn capacity(&self) -> usize {
        POOL_SIZE
    }

    async fn collect_ready(
        sockets: &'a [RefCell<TcpSocket<'a>>],
        port: u16,
        ready: &mut Queue<RefMut<'a, TcpSocket<'a>>, POOL_SIZE>,
    ) {
        poll_fn(|cx| -> Poll<()> {
            let mut ready_it = sockets
                .iter()
                .filter_map(|s: &'a RefCell<TcpSocket<'a>>| s.try_borrow_mut().ok())
                .filter_map(|mut s| match s.poll_wait_next_request_once(cx, port) {
                    Poll::Ready(Ok(())) => Some(s),
                    Poll::Ready(Err(error)) => {
                        defmt::error!("SocketPool: Error while polling socket: {:?}", error);
                        None
                    }
                    Poll::Pending => None,
                });

            while let Some(socket) = ready_it.next() {
                ready.enqueue(socket).ok();
            }

            if ready.is_empty() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
    }
}

trait PollSocket {
    fn poll_accept_once(
        &mut self,
        cx: &mut Context<'_>,
        port: u16,
    ) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_read_ready_once(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_write_ready_once(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SocketPoolError>>;
    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>>;
    fn poll_wait_next_request_once(
        &mut self,
        cx: &mut Context<'_>,
        port: u16,
    ) -> Poll<Result<(), SocketPoolError>>;
}

impl PollSocket for TcpSocket<'_> {
    fn poll_accept_once(
        &mut self,
        cx: &mut Context<'_>,
        port: u16,
    ) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.accept(port))
            .as_mut()
            .poll(cx)
            .map(|res| res.map_err(|e| SocketPoolError::AcceptError(e)))
    }

    fn poll_wait_read_ready_once(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.wait_read_ready())
            .as_mut()
            .poll(cx)
            .map(|_| Ok(()))
    }

    fn poll_wait_write_ready_once(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.wait_write_ready())
            .as_mut()
            .poll(cx)
            .map(|_| Ok(()))
    }

    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SocketPoolError>> {
        pin!(self.flush())
            .as_mut()
            .poll(cx)
            .map(|res| res.map_err(|e| SocketPoolError::IOError(e)))
    }

    fn poll_wait_next_request_once(
        &mut self,
        cx: &mut Context<'_>,
        port: u16,
    ) -> Poll<Result<(), SocketPoolError>> {
        match self.state() {
            State::Established | State::SynSent | State::SynReceived => {
                defmt::trace!(
                    "SocketPool: Wait for request at socket {:?} in state {:?}",
                    self.remote_endpoint(),
                    self.state()
                );
                return self.poll_wait_read_ready_once(cx);
            }

            State::Closed | State::Listen => {
                // In this case we can safelly poll just accept
                match self.poll_accept_once(cx, port) {
                    Poll::Ready(Ok(())) => self.poll_wait_read_ready_once(cx), // We got a new connection, start waiting for data
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }

            State::TimeWait
            | State::FinWait1
            | State::Closing
            | State::LastAck
            | State::CloseWait => {
                // In this case we have to gracefully bring the socket down first.

                // Close the socket and accept a new connection
                self.close();
                defmt::trace!("SocketPool: Closed socket in state {:?}", self.state());
                // Wait for previous operations to flush
                match self.poll_flush_once(cx) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(e)) => {
                        defmt::error!(
                            "SocketPool: flush error for socket in state {:?}",
                            self.state()
                        );
                        return Poll::Ready(Err(e));
                    }
                    Poll::Pending => return Poll::Pending,
                }
                defmt::trace!(
                    "SocketPool: Accept new connection at socket in state {:?}",
                    self.state()
                );

                // Previous operations succeeded, accept new connection on this socket
                match self.poll_accept_once(cx, port) {
                    Poll::Ready(Ok(())) => {
                        defmt::debug!(
                            "SocketPool: New connection {:?} at socket",
                            self.remote_endpoint()
                        );
                        self.poll_wait_read_ready_once(cx)
                    } // We got a new connection, start waiting for data
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }

            // In the FinWait2 state this function will always return Pending, hence setting up a waker to be woken later if
            // state changed (I hope so)
            State::FinWait2 => self.poll_wait_write_ready_once(cx),
        }
    }
}
