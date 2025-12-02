mod web_socket_proto;

use crate::error::Error;
use crate::response_builder::{HttpResponse, HttpResponseBufferRef, HttpResponseBuilder};
use embassy_net::tcp::TcpSocket;
use modular_bitfield::prelude::*;
use sha1::{Digest, Sha1};
use web_socket_proto::*;

const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Specifier)]
#[bits = 4]
enum Opcode {
    ContinuationFrame = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
    //Reserved opcodes
    Reserved3 = 0x3,
    Reserved4 = 0x4,
    Reserved5 = 0x5,
    Reserved6 = 0x6,
    Reserved7 = 0x7,
    Reserved8 = 0xB,
    ReservedC = 0xC,
    ReservedD = 0xD,
    ReservedE = 0xE,
    ReservedF = 0xF,
}

#[bitfield]
struct WebSocketFrameHeader {
    fin: B1,
    reserved123: B3,
    #[bits = 4]
    opcode: Opcode,
    mask: B1,
    payload_len: B7,
}

#[bitfield]
struct ExtendedPayloadLength16 {
    #[bits = 16]
    length: u16,
}

#[bitfield]
struct ExtendedPayloadLength64 {
    #[bits = 64]
    length: u64,
}

#[bitfield]
struct MaskKey {
    #[bits = 64]
    length: u64,
}

fn write_frame_header(
    fin: u8,
    opcode: Opcode,
    payload_len: usize,
    masking_key: Option<u32>,
    buffer: &mut [u8],
) -> usize {
    let mut index = 0;

    let reserved123 = 0;
    let mask_bit = if masking_key.is_some() { 1 } else { 0 };

    let mut header = WebSocketFrameHeader::new();
    header.set_fin(fin);
    header.set_reserved123(reserved123);
    header.set_opcode(opcode);
    header.set_mask(mask_bit);

    if payload_len <= 125 {
        header.set_payload_len(payload_len as u8);
        buffer[index..index + 2].copy_from_slice(&header.into_bytes());
        index += 2;
    } else if payload_len <= 65535 {
        header.set_payload_len(126);
        buffer[index..index + 2].copy_from_slice(&header.into_bytes());
        index += 2;

        let mut extended_length = ExtendedPayloadLength16::new();
        extended_length.set_length(payload_len as u16);
        buffer[index..index + 2].copy_from_slice(&extended_length.into_bytes());
        index += 2;
    } else {
        header.set_payload_len(127);
        buffer[index..index + 2].copy_from_slice(&header.into_bytes());
        index += 2;

        let mut extended_length = ExtendedPayloadLength64::new();
        extended_length.set_length(payload_len as u64);
        buffer[index..index + 8].copy_from_slice(&extended_length.into_bytes());
        index += 8;
    }

    if let Some(mask) = masking_key {
        let mut masking_key = MaskKey::new();
        masking_key.set_length(mask as u64);
        buffer[index..index + 4].copy_from_slice(&masking_key.into_bytes()[0..4]);
        index += 4;
    }

    index
}

struct WebSocketFrameHeaderReading {
    fin: u8,
    opcode: Opcode,
    payload_len: usize,
    masking_key: Option<u32>,
}

fn read_frame_header(
    buffer: &[u8],
) -> Result<(WebSocketFrameHeaderReading, usize), WebSocketError> {
    if buffer.len() < 2 {
        return Err(WebSocketError::InvalidFrame());
    }

    let header = WebSocketFrameHeader::from_bytes([buffer[0], buffer[1]]);
    let mut index = 2;

    let payload_len = match header.payload_len() {
        len @ 0..=125 => len as usize,
        126 => {
            if buffer.len() < index + 2 {
                return Err(WebSocketError::TcpSocketError());
            }
            let extended_length =
                ExtendedPayloadLength16::from_bytes([buffer[index], buffer[index + 1]]);
            index += 2;
            extended_length.length() as usize
        }
        127 => {
            if buffer.len() < index + 8 {
                return Err(WebSocketError::TcpSocketError());
            }
            let extended_length = ExtendedPayloadLength64::from_bytes([
                buffer[index],
                buffer[index + 1],
                buffer[index + 2],
                buffer[index + 3],
                buffer[index + 4],
                buffer[index + 5],
                buffer[index + 6],
                buffer[index + 7],
            ]);
            index += 8;
            extended_length.length() as usize
        }
        _ => return Err(WebSocketError::TcpSocketError()),
    };

    let masking_key = if header.mask() == 1 {
        if buffer.len() < index + 4 {
            return Err(WebSocketError::TcpSocketError());
        }
        let masking_key = u32::from_be_bytes([
            buffer[index],
            buffer[index + 1],
            buffer[index + 2],
            buffer[index + 3],
        ]);
        index += 4;
        Some(masking_key)
    } else {
        None
    };

    let reading = WebSocketFrameHeaderReading {
        fin: header.fin(),
        opcode: header.opcode(),
        payload_len,
        masking_key: if header.mask() == 1 {
            if buffer.len() < index + 4 {
                return Err(WebSocketError::TcpSocketError());
            }
            let masking_key = u32::from_be_bytes([
                buffer[index],
                buffer[index + 1],
                buffer[index + 2],
                buffer[index + 3],
            ]);
            index += 4;
            Some(masking_key)
        } else {
            None
        },
    };

    Ok((reading, index))
}

pub enum WebSocketError {
    TcpSocketError(),
    InvalidFrame(),
}

pub(crate) struct WebSocketState {
    is_open: bool,
}

impl WebSocketState {
    pub fn new() -> Self {
        Self { is_open: true }
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub async fn flush<'socket>(
        &mut self,
        socket: &mut TcpSocket<'socket>,
    ) -> Result<(), WebSocketError> {
        socket
            .flush()
            .await
            .map_err(|_| WebSocketError::TcpSocketError())
    }

    pub async fn close<'socket>(
        &mut self,
        socket: &mut TcpSocket<'socket>,
    ) -> Result<(), WebSocketError> {
        self.is_open = false;
        self.flush(socket).await?;
        //TODO: Implement proper WebSocket closing handshake
        self.is_open = false;
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
                .map_err(|_| Error::InvalidResponse("Failed to encode Sec-WebSocket-Accept"))?;
            Ok(encoded.len())
        })?
        .with_no_body()
}
