use heapless::Vec;

use crate::socket::{
    AbstractSocketBuilder, SocketAccept, SocketClose, SocketConfig, SocketConnect, SocketEndpoint, SocketInfo,
    SocketReadWith, SocketWaitReadReady, SocketWaitWriteReady, SocketWriteWith, State,
};

//TODO: Implement tests for this object.
/// Socket trait defining the common interface for all socket implementations.
pub struct SocketBuffers<'buf> {
    /// Buffer for receiving data (read operations)
    pub rx_buffer: &'buf mut [u8],
    /// Buffer for transmitting data (write operations)
    pub tx_buffer: &'buf mut [u8],
}

impl<'buf> SocketBuffers<'buf> {
    /// Create new socket buffers
    pub fn new(rx_buffer: &'buf mut [u8], tx_buffer: &'buf mut [u8]) -> Self {
        SocketBuffers { rx_buffer, tx_buffer }
    }

    /// Create socket buffers by slicing a single buffer into RX and TX parts.
    ///
    /// ### Arguments
    /// - `buf`: A mutable byte slice that will be split into RX and TX buffers. The length of the slice must be at least `RX_SIZE + TX_SIZE`.
    ///
    /// ### Returns
    /// The tuple containing a `SocketBuffers` instance with RX and TX buffers, and the remaining unused buffer slice.
    ///
    /// ### Panics
    /// Panics if the provided buffer is smaller than the required size for RX and TX buffers.
    ///
    pub fn slice_from_buffer<const RX_SIZE: usize, const TX_SIZE: usize>(
        buf: &'buf mut [u8],
    ) -> (Self, &'buf mut [u8]) {
        assert!(
            buf.len() >= RX_SIZE + TX_SIZE,
            "Buffer size must be at least RX_SIZE + TX_SIZE"
        );
        let (rx_buffer, tail) = buf.split_at_mut(RX_SIZE);
        let (tx_buffer, remainder) = tail.split_at_mut(TX_SIZE);
        (Self { rx_buffer, tx_buffer }, remainder)
    }

    /// Fill a provided vector with socket buffers by slicing a single buffer into multiple RX and TX parts.
    /// The provided buffer is split into `N` pairs of RX and TX buffers, which are then stored in the provided vector.
    ///
    /// ### Arguments
    /// - `buf`: A mutable byte slice that will be split into multiple RX and TX buffers. The length of the slice must be at least `N * (RX_SIZE + TX_SIZE)`.
    /// - `dst`: A mutable reference to a vector that will be filled with the created `SocketBuffers` instances. The vector must have a capacity of at least `N`.
    ///
    /// ### Returns
    /// The remaining unused buffer slice after slicing out `N` pairs of RX and TX buffers.
    ///
    /// ### Panics
    /// Panics if the provided buffer has a size less than `N * (RX_SIZE + TX_SIZE)`.
    /// Panics if RX_SIZE + TX_SIZE is zero.
    ///
    pub fn slice_into_buffers<const RX_SIZE: usize, const TX_SIZE: usize, const N: usize>(
        buf: &'buf mut [u8],
        dst: &mut Vec<Self, N>,
    ) -> &'buf mut [u8] {
        assert!(
            buf.len() >= N * (RX_SIZE + TX_SIZE),
            "Buffer size must be at least N * (RX_SIZE + TX_SIZE)"
        );

        let (exact_buf, remainder) = buf.split_at_mut(N * (RX_SIZE + TX_SIZE));
        let chanks = exact_buf.chunks_exact_mut(RX_SIZE + TX_SIZE).map(|mem_chank| {
            let (rx_buffer, tx_buffer) = mem_chank.split_at_mut(RX_SIZE);
            Self { rx_buffer, tx_buffer }
        });
        dst.extend(chanks);

        remainder
    }
}

/// Embassy implementation of a socket builder with independent buffer and stack lifetimes.
pub struct EmbassyTcpSocketBuilder<'stack, const SOCKETS: usize> {
    stack: embassy_net::Stack<'stack>,
    buffers: heapless::Vec<SocketBuffers<'stack>, SOCKETS>,
    index: usize,
}

impl<'stack, const SOCKETS: usize> EmbassyTcpSocketBuilder<'stack, SOCKETS> {
    /// Create a new EmbassyTcpSocketBuilder with the given stack and buffers.
    ///
    /// ### Arguments
    /// - `stack`: The embassy-net stack to be used for creating sockets.
    /// - `buffers`: A vector of `SocketBuffers` instances that will be used for creating sockets. The length of the vector must be at least `SOCKETS`.
    ///
    /// ### Returns
    /// A new instance of `EmbassyTcpSocketBuilder` configured with the provided stack and buffers.
    ///
    /// ### Panics
    /// Panics if the length of the provided buffers vector is less than `SOCKETS`.
    ///
    pub fn new(stack: embassy_net::Stack<'stack>, buffers: heapless::Vec<SocketBuffers<'stack>, SOCKETS>) -> Self {
        if buffers.len() < SOCKETS {
            panic!("Not enough buffers provided for the number of sockets");
        }
        Self {
            stack,
            buffers,
            index: 0,
        }
    }
}

impl<'stack, const SOCKETS: usize> AbstractSocketBuilder for EmbassyTcpSocketBuilder<'stack, SOCKETS> {
    type Socket = embassy_net::tcp::TcpSocket<'stack>;
    fn build(&mut self) -> Option<embassy_net::tcp::TcpSocket<'stack>> {
        if self.index >= SOCKETS {
            return None; // No more buffers available
        }

        let buffers = self.buffers.pop().unwrap(); // SAFETY: We have already checked that index is within bounds, so this unwrap is safe.

        self.index += 1;
        Some(embassy_net::tcp::TcpSocket::new(
            self.stack,
            buffers.rx_buffer,
            buffers.tx_buffer,
        ))
    }
}

use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};

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

impl SocketConnect for TcpSocket<'_> {
    type Error = embassy_net::tcp::ConnectError;

    #[inline]
    fn connect<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>,
    {
        let endpoint = endpoint.into();
        let ep = match endpoint {
            SocketEndpoint::V4(addr) => embassy_net::IpEndpoint {
                addr: embassy_net::IpAddress::Ipv4(*addr.ip()),
                port: addr.port(),
            },
            #[cfg(feature = "proto-ipv6")]
            SocketEndpoint::V6(addr) => embassy_net::IpEndpoint {
                addr: embassy_net::IpAddress::Ipv6(*addr.ip()),
                port: addr.port(),
            },
            #[cfg(not(feature = "proto-ipv6"))]
            SocketEndpoint::V6(_) => panic!("IPv6 is not supported"),
        };
        self.connect(ep)
    }
}

impl SocketAccept for TcpSocket<'_> {
    type Error = embassy_net::tcp::AcceptError;

    /// Accept an incoming connection and return a new socket for the connection.
    /// This method should handle the TCP connection establishment process, including the three-way handshake.
    fn accept<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>,
    {
        let endpoint = endpoint.into();
        self.accept(endpoint.port())
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
