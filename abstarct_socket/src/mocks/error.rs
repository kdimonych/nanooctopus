/// Error returned by TcpSocket read/write functions.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum DummySocketError {
    /// The connection was reset.
    ///
    /// This can happen on receiving a RST packet, or on timeout.
    ConnectionReset,
}

mod embedded_io_impls {
    use super::*;

    impl embedded_io_async::Error for DummySocketError {
        fn kind(&self) -> embedded_io_async::ErrorKind {
            match self {
                DummySocketError::ConnectionReset => embedded_io_async::ErrorKind::ConnectionReset,
            }
        }
    }
    
}
