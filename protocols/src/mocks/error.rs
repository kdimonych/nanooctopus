use crate::error::Error;
use abstarct_socket::mocks::error::DummyStreamError;

impl From<DummyStreamError> for Error {
    fn from(err: DummyStreamError) -> Self {
        match err {
            DummyStreamError::ConnectionReset => {
                Error::SocketError(embassy_net::tcp::Error::ConnectionReset)
            }
        }
    }
}
