use abstarct_socket::read_with_ext::ReadError;

/// Errors that can occur during HTTP operations
///
/// This enum represents all possible errors that can be returned by the HTTP client
/// during various stages of request processing, from URL parsing to connection
/// establishment and response handling.
#[derive(Debug)]
/// All possible errors returned by the HTTP client.
pub enum Error {
    /// The provided URL was invalid or malformed
    InvalidUrl,
    /// TCP read/write error
    SocketError,
    /// Read buffer overflowed
    ReadBufferOverflow,
    /// No response was received from the server
    ServerError,
    /// The response/request could not be parsed
    InvalidData(&'static str),
    /// This error occurs when there is an issue with the TLS handshake or communication.
    /// Scheme not supported
    UnsupportedScheme(&'static str),
    /// Header error, e.g. too long name or value
    HeaderError(&'static str),
    /// Invalid status code received from the server
    InvalidStatusCode,
}

#[cfg(feature = "defmt")]
impl defmt::Format for Error {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{:?}", defmt::Debug2Format(self));
    }
}

impl From<embassy_net::tcp::Error> for Error {
    fn from(_: embassy_net::tcp::Error) -> Self {
        Error::SocketError
    }
}

impl<SocketReadErrorT> From<ReadError<SocketReadErrorT>> for Error
where
    Error: From<SocketReadErrorT>,
{
    fn from(err: ReadError<SocketReadErrorT>) -> Self {
        match err {
            ReadError::SocketReadError(e) => Error::from(e),
            ReadError::TargetBufferOverflow => Error::ReadBufferOverflow,
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidUrl => write!(f, "Invalid URL"),
            Error::SocketError => write!(f, "TCP communication error"),
            Error::ReadBufferOverflow => write!(f, "Read buffer overflowed"),
            Error::ServerError => write!(f, "Server error"),
            Error::InvalidData(msg) => write!(f, "Invalid response: {msg}"),
            #[cfg(feature = "tls")]
            Error::TlsError(_) => write!(f, "TLS error occurred"),
            Error::UnsupportedScheme(scheme) => write!(f, "Unsupported scheme: {scheme}"),
            Error::HeaderError(msg) => write!(f, "Header error: {msg}"),
            Error::InvalidStatusCode => write!(f, "Invalid status code"),
        }
    }
}

impl<ErrorT> From<embedded_io_async::ReadExactError<ErrorT>> for Error
where
    ErrorT: embedded_io_async::Error,
    Error: From<ErrorT>,
{
    fn from(err: embedded_io_async::ReadExactError<ErrorT>) -> Self {
        match err {
            embedded_io_async::ReadExactError::UnexpectedEof => Error::SocketError,
            embedded_io_async::ReadExactError::Other(e) => Error::from(e),
        }
    }
}

// mod embedded_io_impls {
//     use super::*;
// }

#[cfg(test)]
mod tests {
    use super::*;
    use embassy_net::dns;

    #[test]
    fn test_from_read_error() {
        let mut read_error = ReadError::SocketReadError(embassy_net::tcp::Error::ConnectionReset);
        let mut err: Error = read_error.into();
        assert!(matches!(err, Error::SocketError));

        read_error = ReadError::TargetBufferOverflow;
        err = read_error.into();
        assert!(matches!(err, Error::ReadBufferOverflow));
    }

    #[test]
    fn test_error_display() {
        let e = Error::InvalidUrl;
        assert_eq!(format!("{e}"), "Invalid URL");
        let e = Error::IpAddressEmpty;
        assert_eq!(format!("{e}"), "No IP addresses returned by DNS");
        let e = Error::ServerError;
        assert_eq!(format!("{e}"), "No response received from server");
        let e = Error::InvalidData("bad");
        assert_eq!(format!("{e}"), "Invalid response: bad");
        let e = Error::UnsupportedScheme("ftp");
        assert_eq!(format!("{e}"), "Unsupported scheme: ftp");
        let e = Error::HeaderError("too long");
        assert_eq!(format!("{e}"), "Header error: too long");
        let e = Error::InvalidStatusCode;
        assert_eq!(format!("{e}"), "Invalid status code");
    }

    #[test]
    fn test_from_dns_error() {
        let dns_err = dns::Error::InvalidName;
        let err: Error = dns_err.into();
        match err {
            Error::DnsError(_) => {}
            _ => panic!("Expected DnsError variant"),
        }
    }

    #[test]
    fn test_from_socket_error() {
        let tcp_err = embassy_net::tcp::Error::ConnectionReset;
        let err: Error = tcp_err.into();
        match err {
            Error::SocketError => {}
            _ => panic!("Expected SocketError variant"),
        }
    }
}
