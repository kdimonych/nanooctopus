/// Error returned by stream read functions.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum DummyStreamError {
    /// The connection was reset.
    ///
    /// This can happen on receiving a RST packet, or on timeout.
    ConnectionReset,
}

/// Alias for write errors returned by stream write functions.
pub type DummyReadError = DummyStreamError;
/// Alias for write errors returned by stream write functions.
pub type DummyWriteError = DummyStreamError;

mod embedded_io_impls {
    use super::*;

    impl embedded_io_async::Error for DummyStreamError {
        fn kind(&self) -> embedded_io_async::ErrorKind {
            match self {
                DummyStreamError::ConnectionReset => embedded_io_async::ErrorKind::ConnectionReset,
            }
        }
    }
}
