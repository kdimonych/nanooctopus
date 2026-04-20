extern crate alloc;

use alloc::vec::Vec;

use embedded_io_async::{ErrorType, Read, ReadReady, Write, WriteReady};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpSocket, TcpStream};

use crate::socket::{
    ReadWith, SocketAccept, SocketClose, SocketConfig, SocketConnect, SocketEndpoint, SocketInfo, WriteWith,
};

/// Error type used by the Tokio adapters in this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokioSocketError(pub embedded_io_async::ErrorKind);

const READ_BUFFER_SIZE: usize = 1024;
const WRITE_BUFFER_SIZE: usize = 1024;

impl ReadWith for TokioSocketWrapper {
    async fn read_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        match self {
            TokioSocketWrapper::Stream(stream) => {
                stream.readable().await?;
                let mut buf = [0u8; READ_BUFFER_SIZE];
                let peeked = stream.peek(&mut buf).await?;
                let (consumed, result) = f(&mut buf[..peeked]);
                assert!(consumed <= peeked, "Read more bytes than available in buffer");
                if consumed > 0 {
                    stream.read_exact(&mut buf[..consumed]).await?;
                }
                Ok(result)
            }
            TokioSocketWrapper::Socket(_) => {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Tokio socket is not connected").into())
            }
            TokioSocketWrapper::Listener(_) => {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Tokio listener cannot be read from").into())
            }
        }
    }
}

impl<'socket> ReadWith for TokioSocketReadHalfWrapper<'socket> {
    async fn read_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        if self.pending_mut().is_empty() {
            let mut buf = [0u8; READ_BUFFER_SIZE];
            let read = self.inner_mut().read(&mut buf).await?;
            self.pending_mut().extend_from_slice(&buf[..read]);
        }

        Ok(apply_read_with(self.pending_mut(), f))
    }
}

impl ReadWith for TokioSocketOwnedReadHalfWrapper {
    async fn read_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        if self.pending_mut().is_empty() {
            let mut buf = [0u8; READ_BUFFER_SIZE];
            let read = self.inner_mut().read(&mut buf).await?;
            self.pending_mut().extend_from_slice(&buf[..read]);
        }

        Ok(apply_read_with(self.pending_mut(), f))
    }
}

impl WriteWith for TokioSocketWrapper {
    async fn write_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        match self {
            TokioSocketWrapper::Stream(stream) => {
                stream.writable().await?;
                let mut buf = [0u8; WRITE_BUFFER_SIZE];
                let (written, result) = f(&mut buf);
                assert!(written <= buf.len(), "Wrote more bytes than available in buffer");
                stream.write_all(&buf[..written]).await?;
                Ok(result)
            }
            TokioSocketWrapper::Socket(_) => {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Tokio socket is not connected").into())
            }
            TokioSocketWrapper::Listener(_) => {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Tokio listener cannot be written to").into())
            }
        }
    }
}

impl<'socket> WriteWith for TokioSocketWriteHalfWrapper<'socket> {
    async fn write_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        let mut buf = [0u8; WRITE_BUFFER_SIZE];
        let (written, result) = f(&mut buf);
        assert!(written <= buf.len(), "Wrote more bytes than available in buffer");
        self.inner_mut().write_all(&buf[..written]).await?;
        Ok(result)
    }
}

impl WriteWith for TokioSocketOwnedWriteHalfWrapper {
    async fn write_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        let mut buf = [0u8; WRITE_BUFFER_SIZE];
        let (written, result) = f(&mut buf);
        assert!(written <= buf.len(), "Wrote more bytes than available in buffer");
        self.inner_mut().write_all(&buf[..written]).await?;
        Ok(result)
    }
}

impl From<std::io::Error> for TokioSocketError {
    fn from(value: std::io::Error) -> Self {
        Self(match value.kind() {
            std::io::ErrorKind::NotFound => embedded_io_async::ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => embedded_io_async::ErrorKind::PermissionDenied,
            std::io::ErrorKind::ConnectionRefused => embedded_io_async::ErrorKind::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset => embedded_io_async::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted => embedded_io_async::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::NotConnected => embedded_io_async::ErrorKind::NotConnected,
            std::io::ErrorKind::AddrInUse => embedded_io_async::ErrorKind::AddrInUse,
            std::io::ErrorKind::AddrNotAvailable => embedded_io_async::ErrorKind::AddrNotAvailable,
            std::io::ErrorKind::BrokenPipe => embedded_io_async::ErrorKind::BrokenPipe,
            std::io::ErrorKind::AlreadyExists => embedded_io_async::ErrorKind::AlreadyExists,
            std::io::ErrorKind::InvalidInput => embedded_io_async::ErrorKind::InvalidInput,
            std::io::ErrorKind::InvalidData => embedded_io_async::ErrorKind::InvalidData,
            std::io::ErrorKind::TimedOut => embedded_io_async::ErrorKind::TimedOut,
            std::io::ErrorKind::Interrupted => embedded_io_async::ErrorKind::Interrupted,
            std::io::ErrorKind::Unsupported => embedded_io_async::ErrorKind::Unsupported,
            std::io::ErrorKind::OutOfMemory => embedded_io_async::ErrorKind::OutOfMemory,
            std::io::ErrorKind::WriteZero => embedded_io_async::ErrorKind::WriteZero,
            _ => embedded_io_async::ErrorKind::Other,
        })
    }
}

impl embedded_io_async::Error for TokioSocketError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        self.0
    }
}

/// A Tokio-backed wrapper around TCP sockets, listeners, and connected streams.
pub enum TokioSocketWrapper {
    /// A configurable socket that has not connected yet.
    Socket(TcpSocket),
    /// A listening socket that can accept incoming connections.
    Listener(TcpListener),
    /// A connected TCP stream.
    Stream(TcpStream),
}

impl TokioSocketWrapper {
    /// Creates a wrapper around an unconnected Tokio socket.
    pub const fn new_socket(socket: TcpSocket) -> Self {
        Self::Socket(socket)
    }

    /// Creates a wrapper around a Tokio listener.
    pub fn new_listener(listener: TcpListener) -> Self {
        Self::Listener(listener)
    }

    /// Creates a wrapper around a connected Tokio stream.
    pub fn new_stream(stream: TcpStream) -> Self {
        Self::Stream(stream)
    }
}

impl ErrorType for TokioSocketWrapper {
    type Error = TokioSocketError;
}

impl Read for TokioSocketWrapper {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Stream(stream) => stream.read(buf).await.map_err(Into::into),
            Self::Socket(_) => Err(invalid_input_error("Tokio socket is not connected").into()),
            Self::Listener(_) => Err(invalid_input_error("Tokio listener cannot be read from").into()),
        }
    }
}

impl Write for TokioSocketWrapper {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Stream(stream) => stream.write(buf).await.map_err(Into::into),
            Self::Socket(_) => Err(invalid_input_error("Tokio socket is not connected").into()),
            Self::Listener(_) => Err(invalid_input_error("Tokio listener cannot be written to").into()),
        }
    }
}

impl ReadReady for TokioSocketWrapper {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        match self {
            Self::Stream(_) => Ok(true),
            Self::Socket(_) | Self::Listener(_) => Ok(false),
        }
    }
}

impl WriteReady for TokioSocketWrapper {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        match self {
            Self::Stream(stream) => match stream.try_write(&[]) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
                Err(error) => Err(error.into()),
            },
            Self::Socket(_) | Self::Listener(_) => Ok(false),
        }
    }
}

impl SocketInfo for TokioSocketWrapper {
    fn local_endpoint(&self) -> Option<SocketEndpoint> {
        match self {
            Self::Socket(socket) => {
                let local_addr = socket.local_addr().ok()?;
                let port = local_addr.port();
                Some(SocketEndpoint::new(local_addr.ip(), port))
            }
            Self::Listener(listener) => {
                let local_addr = listener.local_addr().ok()?;
                let port = local_addr.port();
                Some(SocketEndpoint::new(local_addr.ip(), port))
            }
            Self::Stream(stream) => {
                let local_addr = stream.local_addr().ok()?;
                let port = local_addr.port();
                Some(SocketEndpoint::new(local_addr.ip(), port))
            }
        }
    }

    fn remote_endpoint(&self) -> Option<SocketEndpoint> {
        match self {
            Self::Stream(stream) => {
                let remote_addr = stream.peer_addr().ok()?;
                let port = remote_addr.port();
                Some(SocketEndpoint::new(remote_addr.ip(), port))
            }
            Self::Socket(_) | Self::Listener(_) => None,
        }
    }

    fn state(&self) -> crate::socket::State {
        match self {
            Self::Stream(_) => crate::socket::State::Established,
            Self::Socket(_) | Self::Listener(_) => crate::socket::State::Closed,
        }
    }
}

impl SocketClose for TokioSocketWrapper {
    type Error = TokioSocketError;

    async fn close(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Stream(stream) => stream.shutdown().await.map_err(Into::into),
            Self::Socket(_) | Self::Listener(_) => Ok(()),
        }
    }
}

impl SocketConnect for TokioSocketWrapper {
    type Error = TokioSocketError;

    async fn connect<T>(&mut self, endpoint: T) -> Result<(), Self::Error>
    where
        T: Into<SocketEndpoint>,
    {
        let endpoint = endpoint.into();
        match self {
            Self::Socket(socket) => {
                let replacement = if endpoint.is_ipv4() {
                    TcpSocket::new_v4().map_err(TokioSocketError::from)?
                } else {
                    TcpSocket::new_v6().map_err(TokioSocketError::from)?
                };
                let socket = core::mem::replace(socket, replacement);
                let stream = socket.connect(endpoint).await.map_err(TokioSocketError::from)?;
                *self = Self::Stream(stream);
                Ok(())
            }
            Self::Stream(_) => Err(invalid_input_error("Tokio stream is already connected").into()),
            Self::Listener(_) => Err(invalid_input_error("Tokio listener cannot initiate connections").into()),
        }
    }
}

impl SocketAccept for TokioSocketWrapper {
    type Error = TokioSocketError;

    async fn accept<EP>(&mut self, endpoint: EP) -> Result<(), Self::Error>
    where
        EP: Into<SocketEndpoint>,
    {
        let endpoint = endpoint.into();
        loop {
            match self {
                Self::Listener(listener) => {
                    let (stream, _) = listener.accept().await?;
                    *self = Self::Stream(stream);
                    return Ok(());
                }
                Self::Stream(s) => {
                    s.shutdown().await?;
                    if endpoint.is_ipv4() {
                        *self = Self::Socket(TcpSocket::new_v4()?);
                    } else {
                        *self = Self::Socket(TcpSocket::new_v6()?);
                    }
                }
                Self::Socket(_) => {
                    let listener = TcpListener::bind(endpoint).await?;
                    *self = Self::Listener(listener);
                }
            }
        }
    }
}

impl SocketConfig for TokioSocketWrapper {
    /// Set the TCP keep-alive option for the socket, with the specified interval for sending keep-alive
    /// probes.
    fn set_keep_alive(&mut self, interval: Option<core::time::Duration>) {
        match self {
            Self::Socket(s) => {
                // Tokio exposes keep-alive as a boolean on TcpSocket, so enabling it uses the OS default interval.
                s.set_keepalive(interval.is_some()).ok();
            }
            Self::Listener(_) | Self::Stream(_) => {}
        }
    }

    /// Set the timeout for socket operations, such as read and write timeouts.
    fn set_timeout(&mut self, _: Option<core::time::Duration>) {
        // Tokio does not have built-in support for socket timeouts, so this method is a no-op.
    }
}

fn invalid_input_error(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

fn drain_pending_bytes(pending: &mut Vec<u8>, buf: &mut [u8]) -> usize {
    let copied = core::cmp::min(pending.len(), buf.len());
    buf[..copied].copy_from_slice(&pending[..copied]);
    pending.drain(..copied);
    copied
}

fn apply_read_with<F, R>(pending: &mut Vec<u8>, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> (usize, R),
{
    let (consumed, result) = f(pending.as_mut_slice());
    assert!(consumed <= pending.len(), "Read more bytes than available in buffer");
    pending.drain(..consumed);
    result
}

/// A `ReadWith`-compatible wrapper around a borrowed Tokio read half.
pub struct TokioSocketReadHalfWrapper<'socket> {
    inner: ReadHalf<'socket>,
    pending: Vec<u8>,
}

impl<'socket> TokioSocketReadHalfWrapper<'socket> {
    /// Creates a wrapper around a borrowed Tokio read half.
    pub fn new(read_half: ReadHalf<'socket>) -> Self {
        Self {
            inner: read_half,
            pending: Vec::new(),
        }
    }

    pub(crate) fn inner_mut(&mut self) -> &mut ReadHalf<'socket> {
        &mut self.inner
    }

    pub(crate) fn pending_mut(&mut self) -> &mut Vec<u8> {
        &mut self.pending
    }
}

impl ErrorType for TokioSocketReadHalfWrapper<'_> {
    type Error = TokioSocketError;
}

impl Read for TokioSocketReadHalfWrapper<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.pending.is_empty() {
            return Ok(drain_pending_bytes(&mut self.pending, buf));
        }
        self.inner.read(buf).await.map_err(Into::into)
    }
}

impl ReadReady for TokioSocketReadHalfWrapper<'_> {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// A `ReadWith`-compatible wrapper around an owned Tokio read half.
pub struct TokioSocketOwnedReadHalfWrapper {
    inner: OwnedReadHalf,
    pending: Vec<u8>,
}

impl TokioSocketOwnedReadHalfWrapper {
    /// Creates a wrapper around an owned Tokio read half.
    pub fn new(read_half: OwnedReadHalf) -> Self {
        Self {
            inner: read_half,
            pending: Vec::new(),
        }
    }

    pub(crate) fn inner_mut(&mut self) -> &mut OwnedReadHalf {
        &mut self.inner
    }

    pub(crate) fn pending_mut(&mut self) -> &mut Vec<u8> {
        &mut self.pending
    }
}

impl ErrorType for TokioSocketOwnedReadHalfWrapper {
    type Error = TokioSocketError;
}

impl Read for TokioSocketOwnedReadHalfWrapper {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.pending.is_empty() {
            return Ok(drain_pending_bytes(&mut self.pending, buf));
        }
        self.inner.read(buf).await.map_err(Into::into)
    }
}

impl ReadReady for TokioSocketOwnedReadHalfWrapper {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// A `WriteWith`-compatible wrapper around a borrowed Tokio write half.
pub struct TokioSocketWriteHalfWrapper<'socket>(WriteHalf<'socket>);

impl<'socket> TokioSocketWriteHalfWrapper<'socket> {
    /// Creates a wrapper around a borrowed Tokio write half.
    pub const fn new(write_half: WriteHalf<'socket>) -> Self {
        Self(write_half)
    }

    pub(crate) fn inner_mut(&mut self) -> &mut WriteHalf<'socket> {
        &mut self.0
    }
}

impl ErrorType for TokioSocketWriteHalfWrapper<'_> {
    type Error = TokioSocketError;
}

impl Write for TokioSocketWriteHalfWrapper<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await.map_err(Into::into)
    }
}

impl WriteReady for TokioSocketWriteHalfWrapper<'_> {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// A `WriteWith`-compatible wrapper around an owned Tokio write half.
pub struct TokioSocketOwnedWriteHalfWrapper(OwnedWriteHalf);

impl TokioSocketOwnedWriteHalfWrapper {
    /// Creates a wrapper around an owned Tokio write half.
    pub const fn new(write_half: OwnedWriteHalf) -> Self {
        Self(write_half)
    }

    pub(crate) fn inner_mut(&mut self) -> &mut OwnedWriteHalf {
        &mut self.0
    }
}

impl ErrorType for TokioSocketOwnedWriteHalfWrapper {
    type Error = TokioSocketError;
}

impl Write for TokioSocketOwnedWriteHalfWrapper {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await.map_err(Into::into)
    }
}

impl WriteReady for TokioSocketOwnedWriteHalfWrapper {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}
