use crate::read_with_ext::ReadError;

impl From<embassy_net::tcp::Error> for ReadError<embassy_net::tcp::Error> {
    fn from(err: embassy_net::tcp::Error) -> Self {
        ReadError::SocketReadError(err)
    }
}
