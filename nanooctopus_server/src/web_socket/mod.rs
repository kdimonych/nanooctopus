#![allow(async_fn_in_trait)]

mod frame_controller;
mod masker;

#[cfg(feature = "bench_masker")]
pub use masker::PayloadMasker;

use crate::{socket::*, web_socket::frame_controller::FrameController};
pub use edge_ws::Fragmented;
use edge_ws::{Error as FrameError, FrameHeader, FrameType};
use frame_controller::*;

pub use frame_controller::MessageType;

/// Errors that can occur during WebSocket operations, including connection issues, frame parsing errors, and underlying socket errors.
#[derive(Debug)]
#[defmt_or_log::maybe_derive_format]
pub enum WebSocketError<E> {
    /// The connection has been reset by the peer or already closed by us.
    ConnectionReset,
    /// The WebSocket frame is invalid.
    InvalidFrame,
    /// The connection is in an invalid state (e.g., we have failed a previous close handshake and are now in the closing state,
    /// which means the connection is effectively closed but we haven't been able to free up resources yet).
    ConnectionAborted,
    /// An error occurred in the underlying socket.
    Socket(E),
}

impl<E: SocketError> WebSocketError<E> {
    /// Erase the underlying socket error to a more general `SocketErrorKind`.
    /// This is useful for cases where we don't want to expose the specific socket error to the caller (e.g., when
    /// the error is caused by a control frame payload being too large, which is not really a socket error).
    pub fn erase(self) -> WebSocketError<SocketErrorKind> {
        match self {
            WebSocketError::ConnectionReset => WebSocketError::ConnectionReset,
            WebSocketError::InvalidFrame => WebSocketError::InvalidFrame,
            WebSocketError::ConnectionAborted => WebSocketError::ConnectionAborted,
            WebSocketError::Socket(e) => WebSocketError::Socket(e.kind()),
        }
    }
}

impl<E: core::fmt::Display + core::fmt::Debug> core::error::Error for WebSocketError<E> {}

impl<E: core::fmt::Display> core::fmt::Display for WebSocketError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WebSocketError::ConnectionReset => write!(f, "Connection reset"),
            WebSocketError::InvalidFrame => write!(f, "Invalid frame"),
            WebSocketError::ConnectionAborted => write!(f, "Connection aborted"),
            WebSocketError::Socket(e) => write!(f, "Socket error: {}", e),
        }
    }
}

impl<E> From<E> for WebSocketError<E> {
    fn from(error: E) -> Self {
        WebSocketError::Socket(error)
    }
}

impl<E: SocketError> From<FrameError<E>> for WebSocketError<E> {
    fn from(error: FrameError<E>) -> Self {
        match error {
            FrameError::Io(e) => WebSocketError::Socket(e),
            //TODO: extend the error handling to distinguish between different frame errors and return more specific WebSocket errors.
            _ => WebSocketError::InvalidFrame, // Other frame errors are treated as invalid sequences, which will cause the connection to be reset.
        }
    }
}

impl<E: SocketError> From<FrameReadError<E>> for WebSocketError<E> {
    fn from(error: FrameReadError<E>) -> Self {
        match error {
            FrameReadError::Socket(se) => WebSocketError::Socket(se),
            FrameReadError::ClosedByPeer => WebSocketError::ConnectionReset,
            _ => WebSocketError::InvalidFrame,
        }
    }
}

impl<E: SocketError> SocketError for WebSocketError<E>
where
    E: SocketError,
{
    fn kind(&self) -> SocketErrorKind {
        match self {
            WebSocketError::ConnectionReset => SocketErrorKind::ConnectionReset,
            WebSocketError::InvalidFrame => SocketErrorKind::InvalidData,
            WebSocketError::ConnectionAborted => SocketErrorKind::ConnectionAborted,
            WebSocketError::Socket(e) => e.kind(),
        }
    }
}

/// A trait that defines specific methods for reading WebSocket messages.
///
/// WebSocket is designed to be a message-oriented protocol, where each message can be fragmented into multiple frames.
/// This trait provides methods to handle the reading of these messages, including waiting for new messages, checking for pending messages,
/// and retrieving the type of the current message being read.
pub trait FrameRead: SocketErrorType {
    /// Wait for the next incoming data message and return its frame type. This method will automatically handle any incoming control frames
    /// (e.g., Ping, Pong, Close) and only return when a data frame like `Text` or `Binary` is received.
    /// If there are unfinished frames from previous read, this method will return the frame type of that active frame instead of waiting
    /// for a new frame.
    /// If the connection is closed by the peer or already closed by us, this method will return a `ConnectionReset` error. If any other error
    /// occurs while waiting for the next message (e.g., an invalid frame is received or an underlying socket error occurs), this method will return
    /// an appropriate `WebSocketError`.
    ///
    /// ### Returns:
    /// - `Ok(FrameType)`: The frame type of the next incoming message or current active message. The caller can then decide how to handle the message
    ///   based on its type;
    /// - `Err(WebSocketError)`: An error occurred while waiting for the next message. This could be due to a connection reset, an invalid message, or
    ///   an underlying socket error.
    async fn wait_message(&mut self) -> Result<MessageType, Self::Error>;

    /// Check whether there is an active frame reader for an incoming message, which indicates that there are unfinished messages from previous reads
    /// that we need to continue reading from.
    fn is_message_pending(&self) -> bool;

    /// Get the frame type for the currently active incoming message, if any.
    fn get_read_message_type(&self) -> Option<MessageType>;
}

impl<T> FrameRead for &mut T
where
    T: FrameRead,
{
    async fn wait_message(&mut self) -> Result<MessageType, Self::Error> {
        (**self).wait_message().await
    }

    fn is_message_pending(&self) -> bool {
        (**self).is_message_pending()
    }

    fn get_read_message_type(&self) -> Option<MessageType> {
        (**self).get_read_message_type()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum SocketState {
    /// The WebSocket connection is ready for reading and writing.
    Ready,
    /// The WebSocket connection has received a close frame from the peer, indicating that the close handshake is done.
    /// ### Note:
    /// This state indicates that the underlying socket is still open, but the WebSocket connection is effectively closed.
    /// Any further read or write operations will result in a `ConnectionReset` error. The WebSocket still require call
    /// close() or abort() to free up the underlying socket resources.
    WsClosed,
    /// The WebSocket connection is fully closed as long as the underlying socket.
    Closed,
}

/// This struct represents a WebSocket connection, providing methods to send and receive messages, handle control frames, and manage
/// the connection state.
pub struct WebSocket<S> {
    inner: S,
    frame_controller: FrameController,
    active_frame_reader: Option<FrameReader>,
    send_frame_type: MessageType,
    socket_state: SocketState,
}

impl<S> SocketErrorType for WebSocket<S>
where
    S: SocketErrorType,
    <S as SocketErrorType>::Error: SocketError,
{
    type Error = WebSocketError<S::Error>;
}

impl<S> WebSocket<S> {
    /// Create a new WebSocket instance with the given transport socket.
    pub const fn new(socket: S) -> Self {
        Self {
            inner: socket,
            frame_controller: FrameController::new(),
            active_frame_reader: None,
            send_frame_type: MessageType::Binary,
            socket_state: SocketState::Ready,
        }
    }

    /// Set the frame type for outgoing frames.
    /// By default, the frame type is Binary, but it can be changed to Text if needed.
    pub const fn set_send_frame_type(&mut self, frame_type: MessageType) {
        self.send_frame_type = frame_type;
    }

    /// Get the current frame type for outgoing frames.
    pub const fn get_send_frame_type(&self) -> MessageType {
        self.send_frame_type
    }

    /// Perform the WebSocket close handshake by sending a close frame to the peer and waiting for a close response.
    /// If the close handshake is successful, the WebSocket can be considered closed.
    ///
    /// ### Note:
    /// This method does not close the underlying socket. The caller is responsible for calling `close()` or `abort()`
    /// on the WebSocket to free up the underlying socket resources.
    pub async fn close_handshake(&mut self) -> Result<(), WebSocketError<S::Error>>
    where
        S: SocketErrorType + SocketRead + SocketWrite,
        <S as SocketErrorType>::Error: SocketError,
    {
        self.frame_controller
            .send_close(&mut self.inner, CloseCode::NormalClosure)
            .await
            .map_err(WebSocketError::from)?;

        self.inner.flush().await.map_err(WebSocketError::from)?;

        if let Err(e) = self.frame_controller.process_control_frames(&mut self.inner).await {
            if matches!(e, FrameReadError::ClosedByPeer) {
                // We have successfully received the close response and can gracefully close the connection.
                // Mark the socket state as WsClosed to indicate that the close handshake has been completed.
                self.socket_state = SocketState::WsClosed;
                return Ok(());
            }
            Err(WebSocketError::from(e))
        } else {
            Err(WebSocketError::InvalidFrame)
        }
    }
}

impl<S> FrameRead for WebSocket<S>
where
    S: SocketErrorType + SocketRead + SocketWrite,
    <S as SocketErrorType>::Error: SocketError,
{
    async fn wait_message(&mut self) -> Result<MessageType, Self::Error>
    where
        S: SocketErrorType + SocketRead + SocketWrite,
        <S as SocketErrorType>::Error: SocketError,
    {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        let frame_reader = if let Some(frame_reader) = self.active_frame_reader.as_ref() {
            // We have unfinished frame from previous read, so we will continue reading from that frame instead of waiting for a new frame.
            frame_reader
        } else {
            // Fresh new frame, we need to wait for it to arrive.
            let frame_reader = self
                .frame_controller
                .wait_data_frame(&mut self.inner)
                .await
                .map_err(|e| {
                    if matches!(e, FrameReadError::ClosedByPeer) {
                        // Mark socket as closed
                        self.socket_state = SocketState::WsClosed;
                    }
                    WebSocketError::from(e)
                })?;

            self.active_frame_reader.insert(frame_reader)
        };

        Ok(frame_reader.message_type())
    }

    fn is_message_pending(&self) -> bool {
        self.active_frame_reader.is_some()
    }

    fn get_read_message_type(&self) -> Option<MessageType> {
        self.active_frame_reader.as_ref().map(|reader| reader.message_type())
    }
}

impl<S> SocketRead for WebSocket<S>
where
    S: SocketErrorType + SocketRead + SocketWrite,
    <S as SocketErrorType>::Error: SocketError,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        let frame_reader = if let Some(frame_reader) = self.active_frame_reader.as_mut() {
            // We have unfinished frame from previous read, so we will continue reading from that frame instead of waiting for a new frame.
            frame_reader
        } else {
            // Receive new frame, we need to wait for it to arrive.
            let frame_reader = self
                .frame_controller
                .wait_data_frame(&mut self.inner)
                .await
                .map_err(|e| {
                    if matches!(e, FrameReadError::ClosedByPeer) {
                        // Mark socket as closed
                        self.socket_state = SocketState::WsClosed;
                    }
                    WebSocketError::from(e)
                })?;

            self.active_frame_reader.insert(frame_reader)
        };

        let read = frame_reader
            .receive_frame_payload(&mut self.frame_controller, &mut self.inner, buf)
            .await
            .map_err(WebSocketError::from)?;

        if frame_reader.is_finished() {
            // We have finished reading the current frame, so we can clear the active frame reader to indicate that there is no
            // pending message and we can wait for the next message in the next read or wait_message call.
            self.active_frame_reader = None;
        }

        Ok(read)
    }
}

impl<S> SocketWrite for WebSocket<S>
where
    S: SocketErrorType + SocketRead + SocketWrite,
    <S as SocketErrorType>::Error: SocketError,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        let header = FrameHeader {
            frame_type: match self.send_frame_type {
                MessageType::Binary => FrameType::Binary(false),
                MessageType::Text => FrameType::Text(false),
            },
            payload_len: buf.len() as u64,
            mask_key: None,
        };

        header.send(&mut self.inner).await.map_err(WebSocketError::from)?;
        header
            .send_payload(&mut self.inner, buf)
            .await
            .map_err(WebSocketError::from)?;

        Ok(buf.len())
    }

    /// Flush this output stream, ensuring that all intermediately buffered contents reach their destination.
    async fn flush(&mut self) -> Result<(), Self::Error> {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        self.inner.flush().await.map_err(WebSocketError::Socket)
    }
}

impl<S> SocketShutdown for WebSocket<S>
where
    S: SocketErrorType + SocketRead + SocketWrite + SocketShutdown,
    <S as SocketErrorType>::Error: SocketError,
{
    async fn close(&mut self, _: SocketCloseStream) -> Result<(), Self::Error> {
        if self.socket_state == SocketState::Closed {
            return Ok(());
        }

        let Err(err) = self.close_handshake().await else {
            // We have successfully completed the close handshake, so we can gracefully close the connection.
            return self
                .inner
                .close(SocketCloseStream::Both)
                .await
                .inspect(|_| self.socket_state = SocketState::Closed) //Mark socket as closed
                .map_err(WebSocketError::Socket);
        };

        //TODO: Maybe it is better to log an original error from the close_handshake() call.

        // Failed to perform close handshake, which means the connection is in an unrecoverable state.
        // Abort the connection to free up resources and return a connection reset error to the caller.
        self.inner.abort().await.map_err(WebSocketError::Socket)?;
        self.socket_state = SocketState::Closed;
        Err(err)
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        if self.socket_state == SocketState::Closed {
            return Ok(());
        }

        self.inner.abort().await.map_err(WebSocketError::Socket)?;
        self.socket_state = SocketState::Closed;

        Ok(())
    }
}

impl<S> SocketReadReady for WebSocket<S>
where
    S: SocketErrorType + SocketReadReady,
    <S as SocketErrorType>::Error: SocketError,
{
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        self.inner.read_ready().map_err(WebSocketError::Socket)
    }
}

impl<S> SocketWriteReady for WebSocket<S>
where
    S: SocketErrorType + SocketWriteReady,
    <S as SocketErrorType>::Error: SocketError,
{
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        if self.socket_state != SocketState::Ready {
            return Err(WebSocketError::ConnectionReset);
        }

        self.inner.write_ready().map_err(WebSocketError::Socket)
    }
}

#[cfg(test)]
mod tests;
