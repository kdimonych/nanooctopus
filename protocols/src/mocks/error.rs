use crate::error::Error;
use abstarct_socket::mocks::error::DummySocketError;

impl From<DummySocketError> for Error {
    fn from(err: DummySocketError) -> Self {
        match err {
            DummySocketError::ConnectionReset => {
                Error::SocketError(embassy_net::tcp::Error::ConnectionReset)
            }
        }
    }
}
