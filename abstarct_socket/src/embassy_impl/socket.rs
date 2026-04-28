use crate::socket::{
    SocketClose, SocketConfig, SocketEndpoint, SocketInfo, SocketReadWith, SocketWaitReadReady, SocketWaitWriteReady,
    SocketWriteWith, State,
};

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
