use crate::error::Error;
use abstarct_socket::mocks::error::DummyReadError;

impl From<DummyReadError> for Error {
    fn from(err: DummyReadError) -> Self {
        match err {
            DummyReadError::ConnectionReset => {
                Error::SocketError(embassy_net::tcp::Error::ConnectionReset)
            }
        }
    }
}
