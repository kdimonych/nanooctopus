/// Error returned by TcpSocket read/write functions.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum DummyReadError {
    /// The connection was reset.
    ///
    /// This can happen on receiving a RST packet, or on timeout.
    ConnectionReset,
}

mod embedded_io_impls {
    use super::*;

    impl embedded_io_async::Error for DummyReadError {
        fn kind(&self) -> embedded_io_async::ErrorKind {
            match self {
                DummyReadError::ConnectionReset => embedded_io_async::ErrorKind::ConnectionReset,
            }
        }
    }
}
