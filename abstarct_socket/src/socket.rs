pub use embedded_io_async::{
    Error as SocketError, ErrorKind as SocketErrorKind, ErrorType as SocketErrorType, Read as SocketRead,
    ReadReady as SocketReadReady, Write as SocketWrite, WriteReady as SocketWriteReady,
};

/// Trait representing a read stream interface
pub trait SocketReadWith: SocketErrorType {
    /// Read from the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes read and a result value.
    ///
    /// ## Returns
    /// - Returns Ok(R) where R is the result returned by the function `f`.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream.
    ///
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R);
}

/// Implement ReadWith for mutable references to types that implement ReadWith
impl<T: ?Sized + SocketReadWith> SocketReadWith for &mut T {
    #[inline]
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        T::read_with(self, f)
    }
}

/// Trait representing a write stream interface
pub trait SocketWriteWith: SocketErrorType {
    /// Write to the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes written and a result value.
    ///
    /// ## Returns
    /// - Returns Ok(R) where R is the result returned by the function `f`.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while writing to the stream.
    ///
    fn write_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R);
}

/// Implement WriteWith for mutable references to types that implement WriteWith
impl<T: ?Sized + SocketWriteWith> SocketWriteWith for &mut T {
    #[inline]
    fn write_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        T::write_with(self, f)
    }
}

/// Socket close trait for TCP sockets, allowing for graceful shutdown of connections.
pub trait SocketClose {
    /// The error type that may be returned when closing the socket.
    type Error: core::fmt::Debug;

    /// Close the TCP socket gracefully, ensuring that all pending data is sent and acknowledged before closing the connection.
    /// This method should handle the TCP connection teardown process, including sending FIN packets and waiting for ACKs from the remote endpoint.
    /// ## Returns
    /// - Returns Ok(()) if the socket was closed successfully.
    /// - Returns `Self::Error` if an error occurs while closing the socket.
    fn close(&mut self) -> impl core::future::Future<Output = Result<(), Self::Error>>;
}

/// Implement SocketClose for mutable references to types that implement SocketClose
impl<T: ?Sized + SocketClose> SocketClose for &mut T {
    type Error = T::Error;

    #[inline]
    fn close(&mut self) -> impl core::future::Future<Output = Result<(), Self::Error>> {
        T::close(self)
    }
}

/// A type representing a socket endpoint, which includes an IP address and a port number.
pub type SocketEndpoint = ::core::net::SocketAddr;

/// A trait representing a TCP socket, which includes methods for retrieving socket information,
/// connecting to remote endpoints, accepting incoming connections, and performing asynchronous
/// read/write operations.
/// This trait is designed to be implemented by various socket types, allowing for a consistent
/// interface for TCP socket operations across different platforms and implementations.
/// The `Socket` trait encompasses all socket-related functionality, while the `SocketExtended`
/// trait includes additional methods for custom buffer management during read/write operations.
/// Implementers of the `Socket` trait must also implement the `SocketInfo`, `SocketClose`,
/// `SocketRead`, `SocketReadReady`, `SocketWrite`, and `SocketWriteReady` traits, while
/// implementers of the `SocketExtended` trait must also implement the `SocketReadWith` and
/// `SocketWriteWith` traits.
#[derive(Clone, Copy, PartialEq, Eq)]
#[defmt_or_log::derive_format_or_debug]
pub enum State {
    /// The socket is closed and not connected to any remote endpoint.
    Closed,
    /// The socket is in the process of being opened and is waiting for a connection to be established.
    Listen,
    /// The socket is in the process of connecting to a remote endpoint and is waiting for a response.
    SynSent,
    /// The socket has received a connection request and is waiting for the connection to be established.
    SynReceived,
    /// The socket is connected to a remote endpoint and is ready for data transfer.
    Established,
    /// The socket is in the process of closing the connection and is waiting for all pending data to be sent and acknowledged.
    FinWait1,
    /// The socket is in the process of closing the connection and is waiting for all pending data to be sent and acknowledged.
    FinWait2,
    /// The socket has received a connection close request from the remote endpoint and is waiting for the connection to be closed.
    CloseWait,
    /// The socket is in the process of closing the connection and is waiting for all pending data to be sent and acknowledged.
    Closing,
    /// The socket has sent a connection close request and is waiting for an acknowledgment from the remote endpoint.
    LastAck,
    /// The socket is in the TIME-WAIT state, waiting for enough time to pass to ensure the remote endpoint received the acknowledgment of its connection close request.
    TimeWait,
}

/// A trait representing socket information, which includes the local and remote endpoints.
pub trait SocketInfo {
    /// Get the local endpoint of the socket.
    ///
    /// Returns `None` if the socket is not bound (listening) or not connected.
    fn local_endpoint(&self) -> Option<SocketEndpoint>;

    /// Get the remote endpoint of the socket.
    ///
    /// Returns `None` if the socket is not connected.
    fn remote_endpoint(&self) -> Option<SocketEndpoint>;

    /// Get the current state of the socket, which can be used to determine if the socket is ready
    /// for accepting new connections or if it is still in the process of closing a previous
    /// connection.
    fn state(&self) -> State;
}

/// Implement SocketInfo for immutable references to types that implement SocketInfo
impl<T: ?Sized + SocketInfo> SocketInfo for &T {
    #[inline]
    fn local_endpoint(&self) -> Option<SocketEndpoint> {
        T::local_endpoint(self)
    }

    #[inline]
    fn remote_endpoint(&self) -> Option<SocketEndpoint> {
        T::remote_endpoint(self)
    }

    #[inline]
    fn state(&self) -> State {
        T::state(self)
    }
}

/// Socket connect trait for TCP sockets, allowing for asynchronous connection to remote endpoints.
pub trait SocketConnect {
    /// The error type that may be returned when connecting to a remote endpoint.
    type Error: core::fmt::Debug;

    /// Connect to a remote endpoint and return a socket reference.
    fn connect<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>;
}

/// Implement SocketConnect for mutable references to types that implement SocketConnect
impl<T: ?Sized + SocketConnect> SocketConnect for &mut T {
    type Error = T::Error;

    #[inline]
    fn connect<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>,
    {
        T::connect(self, endpoint)
    }
}

/// Socket accept trait for TCP sockets, allowing for asynchronous acceptance of incoming connections.
pub trait SocketAccept {
    /// The error type that may be returned when accepting a connection.
    type Error: core::fmt::Debug;

    /// Accept an incoming connection and return a new socket for the connection.
    /// This method should handle the TCP connection establishment process, including the three-way handshake.
    fn accept<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>;
}

/// Implement SocketAccept for mutable references to types that implement SocketAccept
impl<T: ?Sized + SocketAccept> SocketAccept for &mut T {
    type Error = T::Error;

    #[inline]
    fn accept<EP>(&mut self, endpoint: EP) -> impl core::future::Future<Output = Result<(), Self::Error>>
    where
        EP: Into<SocketEndpoint>,
    {
        T::accept(self, endpoint)
    }
}
/// Socket configuration trait for TCP sockets, allowing for setting socket options such as keep-alive and
/// timeouts.
pub trait SocketConfig {
    /// Set the TCP keep-alive option for the socket, with the specified interval for sending keep-alive
    /// probes.
    fn set_keep_alive(&mut self, interval: Option<core::time::Duration>);

    /// Set the timeout for socket operations, such as read and write timeouts.
    fn set_timeout(&mut self, duration: Option<core::time::Duration>);
}

/// A trait that provides a method for waiting until a socket is ready for reading.
pub trait SocketWaitReadReady {
    /// Wait until the socket is ready for reading, which means that there is data available to read
    /// from the socket.
    fn wait_read_ready(&self) -> impl core::future::Future<Output = ()>;
}

/// Implement SocketWaitReadReady for immutable references to types that implement SocketWaitReadReady
impl<T: ?Sized + SocketWaitReadReady> SocketWaitReadReady for &T {
    #[inline]
    fn wait_read_ready(&self) -> impl core::future::Future<Output = ()> {
        T::wait_read_ready(self)
    }
}

/// A trait that provides a method for waiting until a socket is ready for writing.
pub trait SocketWaitWriteReady {
    /// Wait until the socket is ready for writing, which means that the socket can accept data to be written
    /// without blocking.
    fn wait_write_ready(&self) -> impl core::future::Future<Output = ()>;
}

/// Implement SocketWaitWriteReady for immutable references to types that implement SocketWaitWriteReady
impl<T: ?Sized + SocketWaitWriteReady> SocketWaitWriteReady for &T {
    #[inline]
    fn wait_write_ready(&self) -> impl core::future::Future<Output = ()> {
        T::wait_write_ready(self)
    }
}
/// A trait that encompasses all socket-related functionality, including information retrieval, graceful shutdown,
/// and asynchronous read/write operations with custom buffer management.
pub trait SocketStream:
    SocketRead + SocketReadReady + SocketWrite + SocketWriteReady + SocketWaitReadReady + SocketWaitWriteReady
{
}
impl<
    T: ?Sized + SocketRead + SocketReadReady + SocketWrite + SocketWriteReady + SocketWaitReadReady + SocketWaitWriteReady,
> SocketStream for T
{
}

/// A trait that encompasses all socket-related functionality, including information retrieval, graceful shutdown,
/// and asynchronous read/write operations with custom buffer management.
/// This trait is designed to be implemented by various socket types, allowing for a consistent interface for TCP
/// socket operations across different platforms and implementations. Implementers of the `Socket` trait must also
/// implement the `SocketInfo`, `SocketClose`, `SocketRead`, `SocketReadReady`, `SocketWrite`, `SocketWriteReady`,
/// `SocketReadWith`, and `SocketWriteWith` traits.
pub trait AbstractSocket: SocketStream + SocketInfo + SocketClose + SocketConfig {}
impl<T: ?Sized + SocketStream + SocketInfo + SocketClose + SocketConfig> AbstractSocket for T {}

/// A trait that encompasses all socket-related functionality, including information retrieval, graceful shutdown,
/// and asynchronous read/write operations with custom buffer management.
/// This trait is designed to be implemented by various socket types, allowing for a consistent interface for TCP
/// socket operations across different platforms and implementations. Implementers of the `Socket` trait must also
/// implement the `SocketInfo`, `SocketClose`, `SocketRead`, `SocketReadReady`, `SocketWrite`, `SocketWriteReady`,
/// `SocketReadWith`, and `SocketWriteWith` traits.
pub trait ExtendedSoxet: AbstractSocket + SocketReadWith + SocketWriteWith {}
impl<T: ?Sized + AbstractSocket + SocketReadWith + SocketWriteWith> ExtendedSoxet for T {}

/// A builder trait for constructing socket objects in a no_std-compatible way.
///
/// The builder takes ownership of all objects required to build the socket.
/// The build() method consumes the builder and returns the constructed socket object, which may be an implementation-specific type.
///
/// ### Type Parameters
/// - `'a`: Lifetime for borrowed dependencies
/// - `Sock`: The socket type produced by the builder
///
/// ### Example
/// ```ignore
/// struct MySocketBuilder<'a> { /* ... */ }
/// impl<'a> AbstractSocketBuilder<'a, MySocket<'a>> for MySocketBuilder<'a> {
///     fn build(&'a mut self) -> Option<MySocket<'a>> { /* ... */ }
/// }
/// ```
pub trait AbstractSocketBuilder {
    /// The type of socket produced by the builder. This may be an implementation-specific type that implements the `AbstractSocket` trait.
    type Socket;
    /// Build the socket, consuming the builder and all dependencies.
    /// The returned socket may be an implementation-specific type that implements the `AbstractSocket` trait.
    ///
    /// ### Returns
    /// - `Some(Self::Socket)` if the socket was successfully built.
    /// - `None` if the socket could not be built due to missing dependencies or other issues.
    fn build(&mut self) -> Option<Self::Socket>;
}
