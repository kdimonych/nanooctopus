use crate::stream_search::StreamReadError;
use crate::tokio_impl::tokio_socket_wrapper::TokioSocketError;

impl From<TokioSocketError> for StreamReadError<TokioSocketError> {
    fn from(err: TokioSocketError) -> Self {
        StreamReadError::SocketReadError(err)
    }
}
