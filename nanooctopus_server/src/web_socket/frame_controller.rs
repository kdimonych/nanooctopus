use edge_ws::{Error as FrameError, FrameHeader, FrameType};

use super::masker::PayloadMasker;
use crate::socket::*;

const MAX_CONTROL_FRAME_PAYLOAD_LEN: usize = 125;

/// The message type of a WebSocket message, which can be either Text or Binary.
/// This is used to indicate the type of the message being read or written,
/// and it is derived from the frame type of the first frame of the message.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[defmt_or_log::maybe_derive_format]
pub enum MessageType {
    /// Binary data message.
    Binary,
    /// Text data message.
    Text,
}

impl MessageType {
    #[allow(dead_code)]
    pub(crate) fn to_frame_type(self, is_fragmented: bool) -> FrameType {
        match self {
            MessageType::Binary => FrameType::Binary(is_fragmented),
            MessageType::Text => FrameType::Text(is_fragmented),
        }
    }
}

#[derive(Debug)]
#[defmt_or_log::maybe_derive_format]
pub enum FrameReadError<E> {
    ClosedByPeer,
    InvalidFrame,
    InvalidControlFrame,
    Socket(E),
}

impl<E: core::fmt::Display + core::fmt::Debug> core::error::Error for FrameReadError<E> {}

impl<E: core::fmt::Display> core::fmt::Display for FrameReadError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameReadError::ClosedByPeer => write!(f, "Connection closed by peer"),
            FrameReadError::InvalidFrame => write!(f, "Invalid frame"),
            FrameReadError::InvalidControlFrame => write!(f, "Invalid control frame"),
            FrameReadError::Socket(e) => write!(f, "Socket error: {}", e),
        }
    }
}

impl<E: SocketError> SocketError for FrameReadError<E>
where
    E: SocketError,
{
    fn kind(&self) -> SocketErrorKind {
        match self {
            FrameReadError::ClosedByPeer => SocketErrorKind::ConnectionReset,
            FrameReadError::InvalidFrame => SocketErrorKind::InvalidData,
            FrameReadError::InvalidControlFrame => SocketErrorKind::InvalidData,
            FrameReadError::Socket(e) => e.kind(),
        }
    }
}

impl<E: SocketError> From<FrameError<E>> for FrameReadError<E> {
    fn from(error: FrameError<E>) -> Self {
        match error {
            FrameError::Io(e) => FrameReadError::Socket(e),
            //TODO: extend the error handling to distinguish between different frame errors and return more specific WebSocket errors.
            _ => FrameReadError::InvalidFrame, // Other frame errors are treated as invalid sequences, which will cause the connection to be reset.
        }
    }
}

impl<E: SocketError> From<E> for FrameReadError<E> {
    fn from(error: E) -> Self {
        FrameReadError::Socket(error)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
#[allow(dead_code)]
pub(crate) enum CloseCode {
    /// Normal closure; the connection successfully completed whatever purpose for which it was created.
    NormalClosure = 1000,
    /// The endpoint is going away, either because of a server failure or because the browser is navigating away from the page that opened the connection.
    GoingAway = 1001,
    /// The endpoint is terminating the connection due to a protocol error.
    ProtocolError = 1002,
    /// The endpoint is terminating the connection because it received data of a type it cannot accept.
    UnsupportedData = 1003,
    /// Reserved. The specific meaning of this code is defined in the context of its use.
    NoStatusReceived = 1005,
    /// Reserved. The specific meaning of this code is defined in the context of its use.
    AbnormalClosure = 1006,
    /// The endpoint is terminating the connection because it received a message that violates its policy. This is a generic status code that can be returned when there is no other more suitable status code (e.g., 1003 or 1009) or if there is a need to hide specific details about the policy.
    InvalidFramePayloadData = 1007,
    /// The endpoint is terminating the connection because it received a message that is too big for it to process.
    PolicyViolation = 1008,
    /// The endpoint is terminating the connection because it received a message that is too big for it to process.
    MessageTooBig = 1009,
    /// The endpoint is terminating the connection because it expected the server to negotiate one or more extension, but the server didn't return them in the response of the opening handshake.
    MandatoryExtension = 1010,
    /// The endpoint is terminating the connection because it encountered an unexpected condition that prevented it from fulfilling the request.
    InternalServerError = 1011,
    /// The endpoint is terminating the connection because it is restarting.
    TryAgainLater = 1013,
}

impl CloseCode {
    #[allow(dead_code)]
    pub fn from_be_bytes<E>(bytes: [u8; 2]) -> Result<Self, FrameReadError<E>> {
        match u16::from_be_bytes(bytes) {
            x if x == CloseCode::NormalClosure as u16 => Ok(CloseCode::NormalClosure),
            x if x == CloseCode::GoingAway as u16 => Ok(CloseCode::GoingAway),
            x if x == CloseCode::ProtocolError as u16 => Ok(CloseCode::ProtocolError),
            x if x == CloseCode::UnsupportedData as u16 => Ok(CloseCode::UnsupportedData),
            x if x == CloseCode::NoStatusReceived as u16 => Ok(CloseCode::NoStatusReceived),
            x if x == CloseCode::AbnormalClosure as u16 => Ok(CloseCode::AbnormalClosure),
            x if x == CloseCode::InvalidFramePayloadData as u16 => Ok(CloseCode::InvalidFramePayloadData),
            x if x == CloseCode::PolicyViolation as u16 => Ok(CloseCode::PolicyViolation),
            x if x == CloseCode::MessageTooBig as u16 => Ok(CloseCode::MessageTooBig),
            x if x == CloseCode::MandatoryExtension as u16 => Ok(CloseCode::MandatoryExtension),
            x if x == CloseCode::InternalServerError as u16 => Ok(CloseCode::InternalServerError),
            x if x == CloseCode::TryAgainLater as u16 => Ok(CloseCode::TryAgainLater),
            _ => Err(FrameReadError::InvalidFrame),
        }
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn to_be_bytes(self) -> [u8; 2] {
        self.as_u16().to_be_bytes()
    }
}

pub struct FrameController {
    is_wait_for_pong: bool,
    is_wait_for_close_response: bool,
}

impl FrameController {
    pub const fn new() -> Self {
        Self {
            is_wait_for_pong: false,
            is_wait_for_close_response: false,
        }
    }

    pub async fn process_control_frames<S>(&mut self, mut socket: S) -> Result<FrameHeader, FrameReadError<S::Error>>
    where
        S: SocketWrite + SocketRead,
    {
        // Process control frames until we get a data frame.
        // Control frames can be interleaved with data frames, so we need to handle them as they come.
        loop {
            let header = receive_the_header(&mut socket).await?;

            match header.frame_type {
                // New data frame, return it to the caller to be processed.
                FrameType::Text(_) | FrameType::Binary(_) | FrameType::Continue(_) => {
                    return Ok(header);
                }
                FrameType::Close => {
                    header.check_control_frame_payload_len()?;

                    if self.is_wait_for_close_response {
                        // We received the expected Close response, so we can stop waiting for a Close response and proceed to close the connection.
                        self.is_wait_for_close_response = false;
                    } else {
                        // Remote endpoint closed the connection. So no other frames are expected.
                        response_close(header, &mut socket).await?;
                    }

                    self.is_wait_for_pong = false;
                    return Err(FrameReadError::ClosedByPeer);
                }
                FrameType::Ping => {
                    header.check_control_frame_payload_len()?;

                    //Send a Pong response with the same payload
                    response_pong(header, &mut socket).await?;
                    continue;
                }

                FrameType::Pong => {
                    header.check_control_frame_payload_len()?;

                    // We can ignore Pong frames, but we can use them to detect if the remote endpoint is still responsive.
                    if self.is_wait_for_pong {
                        // We received the expected Pong response, so we can continue waiting for the next frame.
                        self.is_wait_for_pong = false;
                        continue;
                    }

                    // Unsolicited Pong frames are not expected, stop processing and return an error.
                    return Err(FrameReadError::InvalidControlFrame);
                }
            }
        }
    }

    /// This method is used to wait for a new data frame to be received, and return a FrameReader for it. Control frames are handled internally
    /// by the FrameController, so they are not returned by this method, and the caller doesn't need to worry about them.
    pub async fn wait_data_frame<S>(&mut self, mut socket: S) -> Result<FrameReader, FrameReadError<S::Error>>
    where
        S: SocketWrite + SocketRead,
    {
        // Process control frames until we get a data frame.
        // Control frames can be interleaved with data frames, so we need to handle them as they come.
        let header = self.process_control_frames(&mut socket).await?;

        let (is_fragmented, frame_type) = match header.frame_type {
            FrameType::Text(is_fragmented) => (is_fragmented, MessageType::Text),
            FrameType::Binary(is_fragmented) => (is_fragmented, MessageType::Binary),
            FrameType::Continue(_) => return Err(FrameReadError::InvalidFrame),
            FrameType::Close | FrameType::Ping | FrameType::Pong => unreachable!(),
        };

        let frame_reader = FrameReader {
            masker: header.mask_key.map(PayloadMasker::new),
            payload_len: header.payload_len as usize,
            is_final: !is_fragmented,
            frame_type,
        };
        Ok(frame_reader)
    }

    /// This method is used to send a Ping frame to the remote endpoint, and mark that we're waiting for a Pong response, so we can detect if
    /// the remote endpoint is still responsive.
    ///
    /// ### Arguments:
    /// - `socket`: The socket to send the Ping frame through.
    ///
    /// ### Returns:
    /// - `Ok(())` if the Ping frame was sent successfully, and we're now waiting for a Pong response.
    /// - `Err(FrameReaderError<S::Error>)` if there was an error sending the Ping frame.
    #[allow(dead_code)]
    pub async fn send_ping<S>(&mut self, socket: S) -> Result<(), FrameReadError<S::Error>>
    where
        S: SocketWrite,
    {
        if self.is_wait_for_pong {
            // We already sent a ping and we're waiting for a pong, do nothing;
            return Ok(());
        }

        let header = FrameHeader {
            frame_type: FrameType::Ping,
            payload_len: 0,
            mask_key: None, // Server-to-client frames are not masked
        };

        header.send(socket).await?;

        // Mark that we're waiting for a pong response, so we can detect if the remote endpoint is still responsive.
        self.is_wait_for_pong = true;

        Ok(())
    }

    /// This method is used to send a Close frame to the remote endpoint, and mark that we're waiting for a Close response, so we can detect if
    /// the remote endpoint is still responsive and properly handling the close handshake.
    ///
    /// ### Arguments:
    /// - `socket`: The socket to send the Close frame through.
    /// - `close_code`: The close code to send in the Close frame.
    ///
    /// ### Returns:
    /// - `Ok(())` if the Close frame was sent successfully, and we're now waiting for a Close response.
    /// - `Err(FrameReadError<S::Error>)` if there was an error sending the Close frame.
    pub async fn send_close<S>(&mut self, mut socket: S, close_code: CloseCode) -> Result<(), FrameReadError<S::Error>>
    where
        S: SocketWrite + SocketRead,
    {
        if self.is_wait_for_close_response {
            // We already sent a close frame and we're waiting for a close response, do nothing;
            return Ok(());
        }

        let close_code = close_code.to_be_bytes();
        let header = FrameHeader {
            frame_type: FrameType::Close,
            payload_len: close_code.len() as u64,
            mask_key: None,
        };

        header.send(&mut socket).await.map_err(FrameReadError::from)?;
        header
            .send_payload(&mut socket, &close_code)
            .await
            .map_err(FrameReadError::from)?;
        self.is_wait_for_close_response = true;

        Ok(())
    }

    /// This method is used to check if we're currently waiting for a Close response from the remote endpoint.
    ///
    /// ### Returns:
    /// - `true` if we're currently waiting for a Close response,
    /// - `false` otherwise.
    #[allow(dead_code)]
    pub const fn is_waiting_for_close_response(&self) -> bool {
        self.is_wait_for_close_response
    }

    /// This method is used to check if we're currently waiting for a Pong response from the remote endpoint.
    ///
    /// ### Returns:
    /// - `true` if we're currently waiting for a Pong response,
    /// - `false` otherwise.
    #[allow(dead_code)]
    pub const fn is_waiting_for_pong(&self) -> bool {
        self.is_wait_for_pong
    }
}

pub struct FrameReader {
    masker: Option<PayloadMasker>,
    payload_len: usize,
    is_final: bool,
    frame_type: MessageType,
}

impl FrameReader {
    /// This method is used to receive the payload of the current frame.
    ///
    /// ### Arguments:
    /// - `socket`: The socket to read the payload from.
    /// - `buf`: The buffer to read the payload into. The buffer should be large enough to hold the entire payload of the current frame,
    ///   otherwise only a part of the payload will be read, and the remaining payload will be read in the next calls to this method.
    /// ### Returns:
    /// - `Ok(usize)` if the payload was read successfully, and the number of bytes read is returned.
    /// - `Err(FrameReadError<S::Error>)` if there was an error reading the payload.
    /// ### Note:
    ///   The result Ok(0) can mean two things:
    ///   1. If the `buf` length is greater than 0, it means that the current frame fully completed, and there are no more bytes to read
    ///      for this frame, so the caller can stop reading and process the frame.
    ///   2. If the `buf` length is 0, it means that there was no space to read any payload bytes, so no bytes were read, and the caller
    ///      should call this method again with a non-empty buffer to continue reading the payload.
    pub async fn receive_frame_payload<S>(
        &mut self,
        frame_controller: &mut FrameController,
        mut socket: S,
        buf: &mut [u8],
    ) -> Result<usize, FrameReadError<S::Error>>
    where
        S: SocketRead + SocketWrite,
    {
        if buf.is_empty() {
            // No space to read the payload, return 0 to indicate that no bytes were read.
            return Ok(0);
        }

        loop {
            if self.payload_len > 0 {
                let to_read = core::cmp::min(buf.len(), self.payload_len);
                let payload_buf = &mut buf[..to_read];

                let actually_read = socket.read(payload_buf).await.map_err(FrameReadError::from)?;
                if actually_read == 0 {
                    // Unexpected EOF reached
                    return Ok(0);
                }

                self.payload_len -= actually_read;

                if let Some(masker) = self.masker.as_mut() {
                    masker.mask_chank(&mut payload_buf[..actually_read]);
                }

                return Ok(actually_read);
            }

            if self.is_final {
                // No more data to read for this frame
                return Ok(0);
            }

            let header = frame_controller.process_control_frames(&mut socket).await?;
            match header.frame_type {
                FrameType::Continue(is_final) => {
                    header.check_payload_len()?;

                    self.is_final = is_final;
                    self.payload_len = header.payload_len as usize;
                    self.masker = header.mask_key.map(PayloadMasker::new);
                }

                _ => {
                    // In the middle of frames, only Continue frames are allowed, any other frame type is not allowed.
                    return Err(FrameReadError::InvalidFrame);
                }
            }
        }
    }

    /// This method is used to get the type of the current message.
    ///
    /// ### Returns:
    /// - `MessageType::Text` if the current message is a Text message.
    /// - `MessageType::Binary` if the current message is a Binary message.
    pub const fn message_type(&self) -> MessageType {
        self.frame_type
    }

    /// Returns `true` if the current frame has been fully read and there are no more bytes to read for this frame,
    /// so the caller can stop reading and process the frame.
    pub const fn is_finished(&self) -> bool {
        self.payload_len == 0 && self.is_final
    }
}

async fn send_back<S>(
    mut header: FrameHeader,
    mut socket: S,
    as_type: FrameType,
) -> Result<(), FrameReadError<S::Error>>
where
    S: SocketWrite + SocketRead,
{
    let mut buf = [0_u8; MAX_CONTROL_FRAME_PAYLOAD_LEN];
    let payload = header
        .recv_payload(&mut socket, &mut buf)
        .await
        .map_err(FrameReadError::from)?;

    //Send a Close header with the same payload
    header.mask_key = None; // Server-to-client frames are not masked
    header.frame_type = as_type;
    header.send(&mut socket).await.map_err(FrameReadError::from)?;
    header
        .send_payload(&mut socket, payload)
        .await
        .map_err(FrameReadError::from)?;

    Ok(())
}

async fn response_close<S>(header: FrameHeader, socket: S) -> Result<(), FrameReadError<S::Error>>
where
    S: SocketWrite + SocketRead,
{
    send_back(header, socket, FrameType::Close).await
}

async fn receive_the_header<S>(socket: S) -> Result<FrameHeader, FrameReadError<S::Error>>
where
    S: SocketRead,
{
    let header = FrameHeader::recv(socket).await.map_err(FrameReadError::from)?;
    header.check_payload_len()?;
    Ok(header)
}

async fn response_pong<S>(header: FrameHeader, socket: S) -> Result<(), FrameReadError<S::Error>>
where
    S: SocketWrite + SocketRead,
{
    send_back(header, socket, FrameType::Pong).await
}

trait FrameHeaderExt {
    fn check_payload_len<E>(&self) -> Result<(), FrameReadError<E>>;
    fn check_control_frame_payload_len<E>(&self) -> Result<(), FrameReadError<E>>;
}

impl FrameHeaderExt for FrameHeader {
    #[inline]
    fn check_payload_len<E>(&self) -> Result<(), FrameReadError<E>> {
        if self.payload_len as usize > usize::MAX {
            return Err(FrameReadError::InvalidFrame);
        }
        Ok(())
    }

    #[inline]
    fn check_control_frame_payload_len<E>(&self) -> Result<(), FrameReadError<E>> {
        if self.payload_len as usize > MAX_CONTROL_FRAME_PAYLOAD_LEN {
            return Err(FrameReadError::InvalidControlFrame);
        }
        Ok(())
    }
}
