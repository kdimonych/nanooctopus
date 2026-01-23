use core::cmp::min;

use crate::response_builder::{HttpResponse, HttpResponseBufferRef, HttpResponseBuilder};
use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;
use protocols::error::Error;
#[cfg(feature = "ws")]
use protocols::web_socket_proto::*;
use sha1::{Digest, Sha1};
/// WebSocket-related errors.
pub enum WebSocketError {
    /// TCP socket error.
    TcpSocketError,
    /// Invalid WebSocket frame.
    InvalidFrame,
    Closed,
}

enum Reader {
    ReadingHeader(WSHeaderReader),
    ReadingPayload(WSPayloadReader),
}

impl From<WSHeaderReader> for Reader {
    fn from(reader: WSHeaderReader) -> Self {
        Reader::ReadingHeader(reader)
    }
}

impl From<WSPayloadReader> for Reader {
    fn from(reader: WSPayloadReader) -> Self {
        Reader::ReadingPayload(reader)
    }
}

impl Reader {
    fn is_reading_header(&self) -> bool {
        matches!(self, Reader::ReadingHeader(_))
    }
    fn is_reading_payload(&self) -> bool {
        matches!(self, Reader::ReadingPayload(_))
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum WSConnectionState {
    Open,
    Closing,
    ClosedRemotely,
    Closed,
}
pub(crate) struct WebSocketState {
    connection_state: WSConnectionState,

    active_reader: Reader,
}

impl WebSocketState {
    pub fn new() -> Self {
        Self {
            connection_state: WSConnectionState::Open,
            active_reader: Reader::ReadingHeader(WSHeaderReader::new()),
        }
    }

    pub fn is_open(&self) -> bool {
        self.connection_state == WSConnectionState::Open
    }

    pub async fn flush<'socket>(
        &mut self,
        socket: &mut TcpSocket<'socket>,
    ) -> Result<(), WebSocketError> {
        socket
            .flush()
            .await
            .map_err(|_| WebSocketError::TcpSocketError)?;
        Ok(())
    }

    pub async fn close<'socket>(
        &mut self,
        socket: &mut TcpSocket<'socket>,
    ) -> Result<(), WebSocketError> {
        self.flush(socket).await?;

        if self.connection_state == WSConnectionState::Open {
            // Send close frame
            let mut header_buffer = [0u8; MAX_WS_FRAME_HEADER_SIZE];
            let header_size =
                write_frame_header(1, WSOpcode::Close, 0, Option::None, &mut header_buffer)
                    .map_err(|_| WebSocketError::InvalidFrame)?;

            socket
                .write_all(&header_buffer[..header_size])
                .await
                .map_err(|_| WebSocketError::TcpSocketError)?;
        }

        //TODO: Implement proper WebSocket closing handshake
        self.connection_state = WSConnectionState::Closed;
        Ok(())
    }
}

/// Represents a WebSocket connection.
pub struct WebSocket<'state, 'socket> {
    socket: &'state mut TcpSocket<'socket>,
    state: &'state mut WebSocketState,
}

impl<'state, 'socket> WebSocket<'state, 'socket> {
    pub(crate) fn new(
        socket: &'state mut TcpSocket<'socket>,
        state: &'state mut WebSocketState,
    ) -> Self {
        Self { socket, state }
    }

    async fn wait_header_ready(&mut self) -> Result<(), WebSocketError> {
        if self.state.connection_state != WSConnectionState::Open {
            return Err(WebSocketError::Closed);
        }
        if self.state.active_reader.is_reading_payload() {
            return Ok(());
        }
        //Wait for header
        let Reader::ReadingHeader(reader) = &mut self.state.active_reader else {
            unreachable!();
        };

        let header = loop {
            let read_result = self
                .socket
                .read_with(|src_buf| {
                    let res = reader.try_read_header(src_buf);
                    let read_size = match &res {
                        WSHeaderState::Error(_) => {
                            // Force close on error
                            self.state.connection_state = WSConnectionState::Closing;
                            0
                        }
                        WSHeaderState::PendingData(read_size) => *read_size,
                        WSHeaderState::Ready(_, read_size) => *read_size,
                    };
                    (read_size, res)
                })
                .await
                .map_err(|_| {
                    self.state.connection_state = WSConnectionState::ClosedRemotely;
                    WebSocketError::Closed
                })?;
            match read_result {
                WSHeaderState::Error(_) => {
                    self.state.connection_state = WSConnectionState::Closing;
                    return Err(WebSocketError::InvalidFrame);
                }
                WSHeaderState::PendingData(_) => {
                    // Need more data, continue reading
                    continue;
                }
                WSHeaderState::Ready(header, _) => break header,
            }
        };

        if header.opcode() == WSOpcode::Close {
            if header.payload_len() == 0 {
                // No payload to read, we're done
                self.state.connection_state = WSConnectionState::ClosedRemotely;
                return Err(WebSocketError::Closed);
            } else {
                self.state.connection_state = WSConnectionState::Closing;
            }
            // Continue to read close frame payload
        }
        self.state.active_reader = Reader::ReadingPayload(WSPayloadReader::from_header(header));
        Ok(())
    }

    /// Reads the WebSocket frame payload using the provided closure.
    pub async fn read_with<F, R>(&mut self, limit: usize, f: F) -> Result<R, WebSocketError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        self.wait_header_ready().await?;

        let Reader::ReadingPayload(reader) = &mut self.state.active_reader else {
            unreachable!();
        };

        let res = self
            .socket
            .read_with(|src_buf| {
                let buf_limit = min(limit, src_buf.len());
                let src_buf = &mut src_buf[..buf_limit];
                let read_size = reader.decode_payload_in_place(src_buf);
                let result = f(&mut src_buf[..read_size]);
                (read_size, result)
            })
            .await
            .map_err(|_| {
                self.state.connection_state = WSConnectionState::ClosedRemotely;
                WebSocketError::Closed
            })?;

        if reader.is_complete() {
            // Move back to reading header
            self.state.active_reader = Reader::ReadingHeader(WSHeaderReader::new());
        }

        Ok(res)
    }

    /// Writes a binary frame to the WebSocket.
    /// The `fin` parameter indicates whether this is the final fragment in a message.
    /// # Errors
    /// Returns `WebSocketError::Closed` if the connection is not open.
    /// Returns `WebSocketError::InvalidFrame` if there is an error writing the frame header.
    /// Returns `WebSocketError::TcpSocketError` if there is an error writing to the socket.
    ///
    pub async fn write_binary_frame<'a>(
        &mut self,
        payload: &'a [u8],
        fin: bool,
    ) -> Result<(), WebSocketError> {
        let mut header_buffer = [0u8; MAX_WS_FRAME_HEADER_SIZE];

        if self.state.connection_state != WSConnectionState::Open {
            return Err(WebSocketError::Closed);
        }

        let header_size = write_frame_header(
            fin as u8,
            WSOpcode::Binary,
            payload.len(),
            Option::None,
            &mut header_buffer,
        )
        .map_err(|_| WebSocketError::InvalidFrame)?;

        self.socket
            .write_all(&header_buffer[..header_size])
            .await
            .map_err(|_| WebSocketError::TcpSocketError)?;

        // Write header
        self.socket
            .write_all(payload)
            .await
            .map_err(|_| WebSocketError::TcpSocketError)?;

        Ok(())
    }

    /// Writes a binary frame to the WebSocket, encoding the payload in place with masking.
    /// The provided payload buffer will be modified in place.
    /// The `fin` parameter indicates whether this is the final fragment in a message.
    ///
    /// # Errors
    /// Returns `WebSocketError::Closed` if the connection is not open.
    /// Returns `WebSocketError::InvalidFrame` if there is an error encoding the frame.
    /// Returns `WebSocketError::TcpSocketError` if there is an error writing to the socket.
    ///
    pub async fn write_binary_frame_with_encode<'a>(
        &mut self,
        mut payload: &'a mut [u8],
        fin: bool,
        masking_key: [u8; 4],
    ) -> Result<(), WebSocketError> {
        let mut header_buffer = [0u8; MAX_WS_FRAME_HEADER_SIZE];

        if self.state.connection_state != WSConnectionState::Open {
            return Err(WebSocketError::Closed);
        }

        let (mut writer, header_size) = WSEncodeWriter::new(
            &mut header_buffer,
            fin as u8,
            WSOpcode::Binary,
            payload.len(),
            masking_key,
        )
        .map_err(|_| WebSocketError::InvalidFrame)?;

        self.socket
            .write_all(&header_buffer[..header_size])
            .await
            .map_err(|_| WebSocketError::TcpSocketError)?;

        writer
            .encode_payload_in_place(&mut payload)
            .map_err(|_| WebSocketError::InvalidFrame)?;

        // Write header
        self.socket
            .write_all(payload)
            .await
            .map_err(|_| WebSocketError::TcpSocketError)?;

        Ok(())
    }

    /// Reborrows the WebSocket for further operations.
    pub fn reborrow(&mut self) -> WebSocket<'_, 'socket> {
        WebSocket {
            socket: &mut *self.socket,
            state: &mut *self.state,
        }
    }

    /// Flushes the WebSocket connection.
    pub async fn flush(&mut self) -> Result<(), WebSocketError> {
        self.state.flush(self.socket).await
    }

    /// Closes the WebSocket connection.
    pub async fn close(&mut self) -> Result<(), WebSocketError> {
        self.state.close(self.socket).await
    }
}

/// Handles the WebSocket handshake process.
pub(crate) fn try_handle_websocket_handshake<'a>(
    mut response_buffer: HttpResponseBufferRef<'a>,
    web_socket_key: &'a str,
) -> Result<HttpResponse, Error> {
    // Compute the Sec-WebSocket-Accept value
    let key_bytes = web_socket_key.as_bytes();
    let mut hasher = Sha1::new();
    hasher.update(key_bytes);
    hasher.update(WS_GUID);
    let hash = hasher.finalize();

    HttpResponseBuilder::new(response_buffer.reborrow())
        .with_status(crate::StatusCode::SwitchingProtocols)?
        .with_header("Upgrade", "websocket")?
        .with_header("Connection", "Upgrade")?
        .with_header_value_from_filler("Sec-WebSocket-Accept", |buf| {
            // Encode the hash in base64 directly into the provided response buffer
            let encoded = binascii::b64encode(&hash, buf)
                .map_err(|_| Error::InvalidData("Failed to encode Sec-WebSocket-Accept"))?;
            Ok(encoded.len())
        })?
        .with_no_body()
}
