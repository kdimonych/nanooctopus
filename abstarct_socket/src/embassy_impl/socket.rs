use heapless::spsc::Queue;

use crate::socket::{
    AbstractSocketBuilder, SocketClose, SocketConfig, SocketEndpoint, SocketInfo, SocketReadWith, SocketWaitReadReady,
    SocketWaitWriteReady, SocketWriteWith, State,
};

use core::pin::pin;
use defmt_or_log as log;
use embassy_futures::select::*;
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};

const KEEP_ALIVE_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(3);
const SOCKET_IO_TIMEOUT: embassy_time::Duration = embassy_time::Duration::from_secs(5);

// Embassy-net based ReadStream implementation for TcpReader
impl<'stack> SocketReadWith for TcpSocket<'stack> {
    #[inline]
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }
}

// Embassy-net based implementation of ReadStream for TcpReader
impl<'stack> SocketReadWith for TcpReader<'stack> {
    #[inline]
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }
}

// Embassy-net based WriteWith implementation for TcpSocket
impl<'stack> SocketWriteWith for TcpSocket<'stack> {
    #[inline]
    fn write_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.write_with(f)
    }
}

// Embassy-net based implementation of WriteWith for TcpWriter
impl<'stack> SocketWriteWith for TcpWriter<'stack> {
    #[inline]
    fn write_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.write_with(f)
    }
}

impl From<embassy_net::tcp::State> for State {
    fn from(state: embassy_net::tcp::State) -> Self {
        match state {
            embassy_net::tcp::State::Closed => State::Closed,
            embassy_net::tcp::State::Listen => State::Listen,
            embassy_net::tcp::State::SynSent => State::SynSent,
            embassy_net::tcp::State::SynReceived => State::SynReceived,
            embassy_net::tcp::State::Established => State::Established,
            embassy_net::tcp::State::FinWait1 => State::FinWait1,
            embassy_net::tcp::State::FinWait2 => State::FinWait2,
            embassy_net::tcp::State::CloseWait => State::CloseWait,
            embassy_net::tcp::State::Closing => State::Closing,
            embassy_net::tcp::State::LastAck => State::LastAck,
            embassy_net::tcp::State::TimeWait => State::TimeWait,
        }
    }
}

impl From<State> for embassy_net::tcp::State {
    fn from(state: State) -> Self {
        match state {
            State::Closed => embassy_net::tcp::State::Closed,
            State::Listen => embassy_net::tcp::State::Listen,
            State::SynSent => embassy_net::tcp::State::SynSent,
            State::SynReceived => embassy_net::tcp::State::SynReceived,
            State::Established => embassy_net::tcp::State::Established,
            State::FinWait1 => embassy_net::tcp::State::FinWait1,
            State::FinWait2 => embassy_net::tcp::State::FinWait2,
            State::CloseWait => embassy_net::tcp::State::CloseWait,
            State::Closing => embassy_net::tcp::State::Closing,
            State::LastAck => embassy_net::tcp::State::LastAck,
            State::TimeWait => embassy_net::tcp::State::TimeWait,
        }
    }
}

fn from_embassy_endpoint(endpoint: embassy_net::IpEndpoint) -> SocketEndpoint {
    match endpoint.addr {
        embassy_net::IpAddress::Ipv4(addr) => SocketEndpoint::V4(core::net::SocketAddrV4::new(addr, endpoint.port)),
        #[cfg(feature = "proto-ipv6")]
        embassy_net::IpAddress::Ipv6(addr) => {
            SocketEndpoint::V6(core::net::SocketAddrV6::new(addr, endpoint.port, 0, 0))
        }
    }
}

impl SocketInfo for TcpSocket<'_> {
    #[inline]
    fn local_endpoint(&self) -> Option<SocketEndpoint> {
        self.local_endpoint().map(from_embassy_endpoint)
    }

    #[inline]
    fn remote_endpoint(&self) -> Option<SocketEndpoint> {
        self.remote_endpoint().map(from_embassy_endpoint)
    }

    #[inline]
    fn state(&self) -> State {
        State::from(self.state())
    }
}

impl SocketClose for TcpSocket<'_> {
    type Error = embassy_net::tcp::Error;
    #[inline]
    async fn close(&mut self) -> Result<(), Self::Error> {
        // Close the write side of the connection
        self.close();
        // Ensure all pending data is sent
        self.flush().await?;
        // Close the socket
        self.abort();
        // Ensure the RST is sent
        self.flush().await?;
        Result::<_, Self::Error>::Ok(())
    }
}

impl SocketConfig for TcpSocket<'_> {
    #[inline]
    fn set_keep_alive(&mut self, interval: Option<core::time::Duration>) {
        self.set_keep_alive(interval.map(|duration| embassy_time::Duration::from_millis(duration.as_millis() as u64)));
    }

    #[inline]
    fn set_timeout(&mut self, duration: Option<core::time::Duration>) {
        self.set_timeout(duration.map(|duration| embassy_time::Duration::from_millis(duration.as_millis() as u64)));
    }
}

impl SocketWaitReadReady for TcpSocket<'_> {
    async fn wait_read_ready(&self) -> () {
        self.wait_read_ready().await;
    }
}

impl SocketWaitReadReady for TcpReader<'_> {
    async fn wait_read_ready(&self) -> () {
        self.wait_read_ready().await;
    }
}

impl SocketWaitWriteReady for TcpSocket<'_> {
    async fn wait_write_ready(&self) -> () {
        self.wait_write_ready().await;
    }
}

impl SocketWaitWriteReady for TcpWriter<'_> {
    async fn wait_write_ready(&self) -> () {
        self.wait_write_ready().await;
    }
}

use embassy_sync::blocking_mutex::raw::NoopRawMutex;

type Mutex<T> = embassy_sync::mutex::Mutex<NoopRawMutex, T>;
type MutexGuard<'a, T> = embassy_sync::mutex::MutexGuard<'a, NoopRawMutex, T>;

/// Type alias for a mutex guard that provides access to a TcpSocket with the appropriate lifetime.
pub type SocketGuard<'a, 'stack> = MutexGuard<'a, TcpSocket<'stack>>;
type Socket<'a> = Mutex<TcpSocket<'a>>;

/// Embassy-net based implementation of AbstractSocketBuilder for TCP sockets. This builder manages a pool of
/// TCP sockets and accepts incoming connections on a specified endpoint.
pub struct EmbassyTcpSocketBuilder<'stack, const SOCKETS: usize> {
    sockets: [Socket<'stack>; SOCKETS],
    ready: Mutex<Queue<usize, SOCKETS>>,
    endpoint: SocketEndpoint,
}

impl<'stack, const SOCKETS: usize> EmbassyTcpSocketBuilder<'stack, SOCKETS> {
    /// Create a new EmbassyTcpSocketBuilder with the given stack and buffers.
    ///
    /// ### Arguments
    /// - `stack`: The embassy-net stack to be used for creating sockets.
    /// - `endpoint`: The socket endpoint that the builder will bind to and listen on.
    /// - `buffers`: A vector of `SocketBuffers` instances that will be used for creating sockets. The length of the vector
    /// must be at least `SOCKETS`.
    ///
    /// ### Returns
    /// A new instance of `EmbassyTcpSocketBuilder` configured with the provided stack and buffers.
    ///
    /// ### Panics
    /// Panics if the length of the `buffers` vector is less than `SOCKETS`, or if any of the buffers do not meet the
    /// required size for RX and TX buffers.
    ///
    pub fn new<'buf, const RX_SIZE: usize, const TX_SIZE: usize>(
        stack: embassy_net::Stack<'stack>,
        endpoint: SocketEndpoint,
        buffer: &'buf mut [u8],
    ) -> Self
    where
        'buf: 'stack,
    {
        const {
            assert!(SOCKETS > 0, "Socket pool size must be greater than zero");
        };

        log::assert!(
            buffer.len() >= SOCKETS * (RX_SIZE + TX_SIZE),
            "SocketBuilder: Buffer size must be at least SOCKETS * (RX_SIZE + TX_SIZE)"
        );

        let mut chanks = buffer.chunks_exact_mut(RX_SIZE + TX_SIZE).map(|mem_chank| {
            let (rx_buffer, tx_buffer) = mem_chank.split_at_mut(RX_SIZE);
            (rx_buffer, tx_buffer)
        });

        let sockets = {
            core::array::from_fn::<_, SOCKETS, _>(|_| {
                // SAFETY: We have already checked that the buffer has enough space for SOCKETS pairs of RX and TX buffers, so this unwrap is safe.
                let (rx_buffer, tx_buffer) = unsafe { chanks.next().unwrap_unchecked() };
                let mut socket = TcpSocket::new(stack, rx_buffer, tx_buffer);

                // Set keep alive options (This must be set to prevent connections from being closed by NATs)
                socket.set_keep_alive(Some(KEEP_ALIVE_TIMEOUT));
                // This must be set to prevent eternal pending on IO operations
                socket.set_timeout(Some(SOCKET_IO_TIMEOUT));

                Mutex::new(socket)
            })
        };

        EmbassyTcpSocketBuilder::<'stack, SOCKETS> {
            sockets,
            ready: Mutex::new(Queue::new()),
            endpoint,
        }
    }
}

impl<'stack, const SOCKETS: usize> AbstractSocketBuilder for EmbassyTcpSocketBuilder<'stack, SOCKETS> {
    type Socket<'a>
        = SocketGuard<'a, 'stack>
    where
        Self: 'a;

    async fn accept(&self) -> Self::Socket<'_> {
        let sockets = &self.sockets;

        {
            let endpoint = self.endpoint;
            let pending_sockets = pin!(core::array::from_fn::<_, SOCKETS, _>(|idx| accept_loop(
                &sockets[idx],
                endpoint
            )));
            let (socket, ready_idx) = select_slice(pending_sockets).await;

            // Put the index of ready socket into the ready queue so that it can be picked up by the next acquire_next_request call if needed
            self.ready.lock().await.enqueue(ready_idx).unwrap();

            // Note: this line is crucial. It keeps the socket locked until the socket is enqueued
            // into the ready queue. This ensures that the socket is not released before it is marked
            // as ready, which could lead to a race condition where the socket is released and acquired
            // by another task before it is marked as ready, causing the first task to wait indefinitely
            // for a socket that is already in use.
            let end_point = socket.remote_endpoint();

            log::info!(
                "SocketBuilders: Socket[{:?}] {:?} is ready on port {}",
                ready_idx,
                end_point,
                endpoint
            );
        }

        // Get the next ready socket index from the ready queue and return the socket reference
        let idx = self.ready.lock().await.dequeue().unwrap();
        sockets[idx].lock().await
    }

    fn endpoint(&self) -> SocketEndpoint {
        self.endpoint
    }
}

async fn accept_loop<'a, 'stack>(
    socket: &'a Mutex<embassy_net::tcp::TcpSocket<'stack>>,
    endpoint: SocketEndpoint,
) -> SocketGuard<'a, 'stack> {
    let mut socket = socket.lock().await;
    log::debug!(
        "SocketBuilder: Socket {:?} released. Current state: {:?}",
        socket.remote_endpoint(),
        socket.state()
    );
    while wait_for_socket_ready(&mut socket, endpoint).await.is_err() {}

    socket
}

async fn wait_for_socket_ready(socket: &mut SocketGuard<'_, '_>, endpoint: SocketEndpoint) -> Result<(), ()> {
    loop {
        use embassy_net::tcp::State;
        log::debug!(
            "SocketBuilder: Waiting for socket {:?} to be ready. Current state: {:?}",
            socket.remote_endpoint(),
            socket.state()
        );
        match socket.state() {
            State::Established | State::SynSent | State::SynReceived => {
                socket.flush().await.map_err(|e| {
                    log::error!(
                        "SocketBuilder: Error while flushing socket: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::Closed | State::Listen => {
                socket.accept(endpoint.port()).await.map_err(|e| {
                    log::error!(
                        "SocketBuilder: Error while accepting connection: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                socket.wait_read_ready().await;
                return Ok(());
            }
            State::TimeWait | State::FinWait1 | State::Closing | State::LastAck | State::CloseWait => {
                // The socket is in a state where it is either closing or waiting for the remote to close,
                // we need to gracefully bring it down first before accepting a new connection on it.
                // Close the write side of the connection
                socket.close();
                // Ensure all pending data is sent
                socket.flush().await.map_err(|e| {
                    log::error!(
                        "SocketBuilder: Error while flushing socket: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                // Close the socket
                socket.abort();
                // Ensure the RST is sent
                socket.flush().await.map_err(|e| {
                    log::error!(
                        "SocketBuilder: Error while flushing socket: {:?}",
                        log::Debug2Format(&e)
                    );
                })?;
                socket.accept(endpoint.port()).await.map_err(|e| {
                    log::error!(
                        "SocketBuilder: Error while accepting connection: {:?}",
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
