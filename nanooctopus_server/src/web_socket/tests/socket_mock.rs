use crate::socket::*;
use mockall::mock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockError {}
impl core::error::Error for MockError {}

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MockError")
    }
}

impl SocketError for MockError {
    fn kind(&self) -> SocketErrorKind {
        SocketErrorKind::Other
    }
}

mock! {
    pub Socket{}

    impl SocketErrorType for Socket {
        type Error = MockError;
    }

    impl SocketRead for Socket {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockError>;
    }

    impl SocketReadReady for Socket {
        fn read_ready(&mut self) -> Result<bool, MockError>;
    }

    impl SocketWrite for Socket {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError>;
        async fn flush(&mut self) -> Result<(), MockError>;
    }

    impl SocketWriteReady for Socket {
        fn write_ready(&mut self) -> Result<bool, MockError>;
    }

    impl SocketShutdown for Socket {
        async fn close(&mut self, reason: SocketCloseStream) -> Result<(), MockError>;
        async fn abort(&mut self) -> Result<(), MockError>;
    }
}
