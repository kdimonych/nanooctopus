pub use embedded_io_async::Error as SocketError;
pub use embedded_io_async::ErrorKind as SocketErrorKind;
pub use embedded_io_async::ErrorType as SocketErrorType;
pub use embedded_io_async::Read as SocketRead;
pub use embedded_io_async::ReadExactError as SocketReadExactError;
pub use embedded_io_async::ReadReady as SocketReadReady;
pub use embedded_io_async::Write as SocketWrite;
pub use embedded_io_async::WriteReady as SocketWriteReady;

pub use edge_nal::Close as SocketCloseStream;
pub use edge_nal::TcpShutdown as SocketShutdown;
pub use edge_nal::TcpSplit as SocketSplit;

/// A type representing a socket endpoint, which includes an IP address and a port number.
pub type SocketEndpoint = ::core::net::SocketAddr;

/// A trait that encompasses all socket-related functionality, including information retrieval, graceful shutdown,
/// and asynchronous read/write operations with custom buffer management.
pub trait SocketStream: SocketRead + SocketReadReady + SocketWrite + SocketWriteReady {}
impl<T: ?Sized + SocketRead + SocketReadReady + SocketWrite + SocketWriteReady> SocketStream for T {}

/// A trait that encompasses all socket-related functionality, including information retrieval, graceful shutdown,
/// and asynchronous read/write operations with custom buffer management.
/// This trait is designed to be implemented by various socket types, allowing for a consistent interface for TCP
/// socket operations across different platforms and implementations. Implementers of the `Socket` trait must also
/// implement the `SocketShutdown`, `SocketRead`, `SocketReadReady`, `SocketWrite`, `SocketWriteReady`,
/// `SocketReadWith`, and `SocketWriteWith` traits.
pub trait Socket: SocketStream + SocketShutdown {}
impl<T: ?Sized + SocketStream + SocketShutdown> Socket for T {}
