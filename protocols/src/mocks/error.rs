use crate::error::Error;
use abstarct_socket::mocks::error::MockStreamError;

impl From<MockStreamError> for Error {
    fn from(err: MockStreamError) -> Self {
        match err {
            MockStreamError::ConnectionReset => Error::SocketError(embassy_net::tcp::Error::ConnectionReset),
        }
    }
}
