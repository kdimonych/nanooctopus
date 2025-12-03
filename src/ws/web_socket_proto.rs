use core::cmp::min;

use modular_bitfield::prelude::*;

pub const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MIN_WS_FRAME_HEADER_SIZE: usize = 2;
pub const WS_EXTENDEDPAYLOAD_LEN_SHORT: usize = 2;
pub const WS_EXTENDEDPAYLOAD_LEN_LONG: usize = 8;
pub const WS_MASKING_KEY_LEN: usize = 4;
pub const MAX_WS_FRAME_HEADER_SIZE: usize =
    MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_LONG + WS_MASKING_KEY_LEN; // 2 + 8 + 4
pub const MASKING_KEY_LEN: usize = 4;

pub type MaskKey = [u8; MASKING_KEY_LEN];

pub type WSRequiredSizeHint = usize;
pub type WSReadSize = usize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebSocketProtoError {
    /// Not enough data available to read the complete frame header.
    /// Contains the number hint of bytes needed.
    NotEnoughData(WSRequiredSizeHint),
    InvalidFrame,
}

#[derive(Specifier, Debug, Clone, Copy, PartialEq)]
#[bits = 4]
pub enum WSOpcode {
    ContinuationFrame = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
    //Reserved opcodes
    // Reserved3 = 0x3,
    // Reserved4 = 0x4,
    // Reserved5 = 0x5,
    // Reserved6 = 0x6,
    // Reserved7 = 0x7,
    // Reserved8 = 0xB,
    // ReservedC = 0xC,
    // ReservedD = 0xD,
    // ReservedE = 0xE,
    // ReservedF = 0xF,
}

#[bitfield]
struct WebSocketFrameHeaderPacked {
    // Byte 0
    #[bits = 4]
    opcode: WSOpcode,
    #[skip]
    __: B3,
    fin: B1,

    // Byte 1
    payload_len: B7,
    mask: B1,
}

fn write_frame_header(
    fin: u8,
    opcode: WSOpcode,
    payload_len: usize,
    masking_key: Option<MaskKey>,
    buffer: &mut [u8],
) -> Result<usize, ()> {
    let mut index = 0;

    let mask_bit = if masking_key.is_some() { 1 } else { 0 };

    let mut header = WebSocketFrameHeaderPacked::new();
    header.set_fin(fin);
    header.set_opcode(opcode);
    header.set_mask(mask_bit);

    if payload_len <= 125 {
        if buffer.len() < MIN_WS_FRAME_HEADER_SIZE {
            return Err(());
        }

        header.set_payload_len(payload_len as u8);
        buffer[index..index + MIN_WS_FRAME_HEADER_SIZE].copy_from_slice(&header.into_bytes());
        index += MIN_WS_FRAME_HEADER_SIZE;
    } else if payload_len <= 65535 {
        if buffer.len() < MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_SHORT {
            return Err(());
        }

        header.set_payload_len(126);
        buffer[index..index + MIN_WS_FRAME_HEADER_SIZE].copy_from_slice(&header.into_bytes());
        index += MIN_WS_FRAME_HEADER_SIZE;

        let payload_len_sort = payload_len as u16;
        buffer[index..index + WS_EXTENDEDPAYLOAD_LEN_SHORT]
            .copy_from_slice(&payload_len_sort.to_be_bytes());
        index += 2;
    } else {
        if buffer.len() < MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_LONG {
            return Err(());
        }

        header.set_payload_len(127);
        buffer[index..index + MIN_WS_FRAME_HEADER_SIZE].copy_from_slice(&header.into_bytes());
        index += MIN_WS_FRAME_HEADER_SIZE;

        buffer[index..index + WS_EXTENDEDPAYLOAD_LEN_LONG]
            .copy_from_slice(&payload_len.to_be_bytes());
        index += WS_EXTENDEDPAYLOAD_LEN_LONG;
    }

    if let Some(mask) = masking_key {
        if buffer.len() < index + WS_MASKING_KEY_LEN {
            return Err(());
        }
        buffer[index..index + WS_MASKING_KEY_LEN].copy_from_slice(&mask);
        index += WS_MASKING_KEY_LEN;
    }

    Ok(index)
}

pub struct WSFrameHeader {
    fin: u8,
    opcode: WSOpcode,
    payload_len: usize,
    masking_key: Option<MaskKey>,
}

impl WSFrameHeader {
    pub fn fin(&self) -> u8 {
        self.fin
    }

    pub fn opcode(&self) -> WSOpcode {
        self.opcode
    }

    pub fn payload_len(&self) -> usize {
        self.payload_len
    }

    pub fn masking_key(&self) -> Option<MaskKey> {
        self.masking_key
    }
}

fn read_frame_header(buffer: &[u8]) -> Result<(WSFrameHeader, usize), WebSocketProtoError> {
    let mut expected_size = MIN_WS_FRAME_HEADER_SIZE;
    if buffer.len() < expected_size {
        return Err(WebSocketProtoError::NotEnoughData(
            expected_size, // Need at least 2 bytes
        ));
    }

    let header = WebSocketFrameHeaderPacked::from_bytes([buffer[0], buffer[1]]);
    let opcode = header
        .opcode_or_err()
        .map_err(|_| WebSocketProtoError::InvalidFrame)?;

    expected_size += if header.mask() == 1 {
        WS_MASKING_KEY_LEN
    } else {
        0
    };

    let payload_len = match header.payload_len() {
        len @ 0..=125 => {
            if buffer.len() < expected_size {
                return Err(WebSocketProtoError::NotEnoughData(
                    expected_size, // Need at least 2 bytes + 2 bytes of masking key if present
                ));
            }

            len as usize
        }
        126 => {
            expected_size += WS_EXTENDEDPAYLOAD_LEN_SHORT;

            if buffer.len() < expected_size {
                return Err(WebSocketProtoError::NotEnoughData(
                    expected_size, // Need at least 4 bytes + 2 bytes of masking key if present
                ));
            }

            // The value is stored in network byte order (big-endian)
            const VALUE_START: usize = MIN_WS_FRAME_HEADER_SIZE;
            const VALUE_END: usize = MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_SHORT;
            let extended_length: u16 =
                u16::from_be_bytes(buffer[VALUE_START..VALUE_END].try_into().unwrap());
            extended_length as usize
        }
        127 => {
            expected_size += WS_EXTENDEDPAYLOAD_LEN_LONG;

            if buffer.len() < expected_size {
                return Err(WebSocketProtoError::NotEnoughData(
                    expected_size, // Need at least 4 bytes + 2 bytes of masking key if present
                ));
            }
            // The value is stored in network byte order (big-endian)
            const VALUE_START: usize = MIN_WS_FRAME_HEADER_SIZE;
            const VALUE_END: usize = MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_LONG;
            let extended_length =
                u64::from_be_bytes(buffer[VALUE_START..VALUE_END].try_into().unwrap());
            if extended_length >> 63 != 0 {
                // Most significant bit must be 0
                return Err(WebSocketProtoError::InvalidFrame);
            }
            extended_length as usize
        }
        _ => return Err(WebSocketProtoError::InvalidFrame),
    };

    let masking_key = if header.mask() == 1 {
        MaskKey::try_from(&buffer[expected_size - WS_MASKING_KEY_LEN..expected_size]).ok()
    } else {
        None
    };

    let reading = WSFrameHeader {
        fin: header.fin(),
        opcode,
        payload_len,
        masking_key,
    };

    Ok((reading, expected_size))
}

pub trait WSMaskKeyProvider {
    fn masking_key(&self) -> Option<MaskKey>;
}

pub struct WSUnmasked;
pub struct WSMasked {
    idx: usize,
    masking_key: MaskKey,
}
impl WSMasked {
    pub const fn with(masking_key: MaskKey) -> Self {
        Self {
            masking_key,
            idx: 0,
        }
    }
}

impl WSMaskKeyProvider for WSUnmasked {
    fn masking_key(&self) -> Option<MaskKey> {
        None
    }
}

impl WSMaskKeyProvider for WSMasked {
    fn masking_key(&self) -> Option<MaskKey> {
        Some(self.masking_key)
    }
}

pub struct WSStreamWriter<MaskingType> {
    free_space: usize,
    masking_type: MaskingType,
}

impl<MaskingType> WSStreamWriter<MaskingType> {
    pub fn new(
        out_buf: &mut [u8],
        fin: u8,
        opcode: WSOpcode,
        payload_len: usize,
        masking_type: MaskingType,
    ) -> Result<(Self, usize), ()>
    where
        MaskingType: WSMaskKeyProvider,
    {
        let result = Self {
            free_space: payload_len,
            masking_type,
        };

        let header_size = write_frame_header(
            fin,
            opcode,
            payload_len,
            result.masking_type.masking_key(),
            out_buf,
        )?;

        Ok((result, header_size))
    }
}

pub trait WSStreamPayloadEncoder {
    /// Writes payload to the destination buffer, applying masking if necessary.
    /// This function assumes that the overall encoded_payload length does not exceed the allocated payload length in the header.
    /// Returns the number of bytes written to the payload_dst.
    /// # Errors
    /// Returns Err(()) if the payload_src length is greater than the allocated payload length that left.
    ///
    fn encode_payload(&mut self, payload_dst: &mut [u8], payload_src: &[u8]) -> Result<usize, ()>;
}

impl WSStreamPayloadEncoder for WSStreamWriter<WSUnmasked> {
    fn encode_payload(&mut self, payload_dst: &mut [u8], payload_src: &[u8]) -> Result<usize, ()> {
        if payload_src.len() > self.free_space || payload_dst.len() < payload_src.len() {
            return Err(());
        }

        let size = min(payload_src.len(), payload_dst.len());
        payload_dst[..size].copy_from_slice(&payload_src[..size]);
        self.free_space -= size;
        Ok(size)
    }
}

impl WSStreamPayloadEncoder for WSStreamWriter<WSMasked> {
    fn encode_payload(&mut self, payload_dst: &mut [u8], payload_src: &[u8]) -> Result<usize, ()> {
        if payload_src.len() > self.free_space {
            return Err(());
        }

        let mut dst = payload_dst[..payload_src.len()].iter_mut();
        let mut src = payload_src[..payload_src.len()].iter();
        let masking_key = self.masking_type.masking_key;
        let mut transferred: usize = 0;

        while let Some(src_byte) = src.next()
            && let Some(dst_byte) = dst.next()
        {
            let j = self.masking_type.idx % masking_key.len();
            let key_byte = masking_key[j];
            *dst_byte = *src_byte ^ key_byte;
            self.masking_type.idx += 1;
            transferred += 1;
        }

        self.free_space -= transferred;
        Ok(transferred)
    }
}

pub enum WSHeaderState<E> {
    /// Not enough data to read the complete header. Contains the number of bytes read from provided buffer.
    PendingData(usize),
    /// Successfully read the header. Contains the header and number of bytes read.
    Ready(WSFrameHeader, usize),
    Error(E),
}

impl<E> WSHeaderState<E> {
    pub fn is_ready(&self) -> bool {
        matches!(self, WSHeaderState::Ready(_, _))
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, WSHeaderState::PendingData(_))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, WSHeaderState::Error(_))
    }

    pub fn unwrap_ready(self) -> (WSFrameHeader, usize) {
        match self {
            WSHeaderState::Ready(header, size) => (header, size),
            _ => panic!("Called unwrap_ready on a non-ready state"),
        }
    }

    pub fn unwrap_pending(self) -> usize {
        match self {
            WSHeaderState::PendingData(size) => size,
            _ => panic!("Called unwrap_pending on a non-pending state"),
        }
    }

    pub fn unwrap_error(self) -> E {
        match self {
            WSHeaderState::Error(e) => e,
            _ => panic!("Called unwrap_error on a non-error state"),
        }
    }
}

pub struct WSHeaderReader {
    header_buf: heapless::Vec<u8, MAX_WS_FRAME_HEADER_SIZE>,
    expected_bytes: usize,
}

impl WSHeaderReader {
    /// Creates a new WebSocket frame header reader.
    pub const fn new() -> Self {
        Self {
            header_buf: heapless::Vec::new(),
            expected_bytes: MIN_WS_FRAME_HEADER_SIZE,
        }
    }

    /// Checks if the reader is currently reading a header.
    pub fn is_reading(&self) -> bool {
        !self.header_buf.is_empty()
    }

    /// Tries to read the WebSocket frame header from the provided source buffer.
    ///
    /// Note: This function may be called multiple times as more data becomes available.
    /// It accumulates data internally until the complete header is read.
    /// Returns the state of the read operation.
    /// # Returns
    /// - `WSHeaderReadState::Ready(WebSocketFrameHeader, usize)`: Successfully read the header.
    /// Contains the header and number of bytes read during the last call.
    /// - `WSHeaderReadState::PendingData(usize)`: Not enough data to read the complete header.
    /// Contains the number of bytes read during the last call.
    /// - `WSHeaderReadState::Error(WebSocketProtoError)`: Invalid header or other error.
    /// # Errors
    /// Returns `WSHeaderReadState::Error(WebSocketProtoError)` if an error occurs while reading the header.
    ///
    /// # Examples
    /// ```rust
    /// let mut reader = WSHeaderReader::new();
    /// let data = [0b10000001, 0b10010001, 0b01101000, 0b00010010, 0b11110001, 0b00110110];
    /// loop{
    ///     // Emulate a stream of incoming data
    ///     let mut src_buf = &data[..];
    ///
    ///     match reader.try_read_header(src_buf) {
    ///         WSHeaderReadState::Ready(header, read_size) => {
    ///             // Process the header
    ///         },
    ///         WSHeaderReadState::PendingData(read_size) => {
    ///             // Need more data
    ///         },
    ///         WSHeaderReadState::Error(e) => {
    ///             // Handle error
    ///         },
    ///     }
    /// }
    /// ```
    pub fn try_read_header(&mut self, mut src_buf: &[u8]) -> WSHeaderState<WebSocketProtoError> {
        // Try to reade as many bytes as needed
        loop {
            if self.header_buf.is_empty() {
                // Fast path: try to read directly from src_buf if we have enough data
                match read_frame_header(src_buf) {
                    Ok((header, read_size)) => return WSHeaderState::Ready(header, read_size),
                    Err(WebSocketProtoError::NotEnoughData(required_size)) => {
                        // Not enough data, copy what we have and update expected bytes
                        self.header_buf.extend_from_slice(src_buf).unwrap();
                        self.expected_bytes = required_size - src_buf.len();
                        return WSHeaderState::PendingData(src_buf.len());
                    }
                    Err(e) => return WSHeaderState::Error(e),
                }
            } else if self.header_buf.len() + src_buf.len() < self.expected_bytes {
                // Stilll not enough data, copy all available data to internal buffer
                self.header_buf.extend_from_slice(src_buf).unwrap();
                // Update expected bytes
                self.expected_bytes -= src_buf.len();
                return WSHeaderState::PendingData(src_buf.len());
            } else {
                // We probably have enough data to complete the header
                self.header_buf
                    .extend_from_slice(&src_buf[..self.expected_bytes])
                    .unwrap();
                // Remove consumed data from src_buf
                src_buf = &src_buf[self.expected_bytes..];

                // No more data need to read
                match read_frame_header(self.header_buf.as_slice()) {
                    Ok((header, read_size)) => {
                        // Reset for next read
                        self.header_buf.clear();
                        self.expected_bytes = MIN_WS_FRAME_HEADER_SIZE;
                        // Return the read header
                        return WSHeaderState::Ready(header, read_size);
                    }

                    Err(WebSocketProtoError::NotEnoughData(new_required_size)) => {
                        // Still not enough data, update expected bytes
                        self.expected_bytes = new_required_size - self.header_buf.len();
                        // And make another try
                        continue;
                    }
                    Err(e) => return WSHeaderState::Error(e),
                }
            }
        }
    }
}

pub struct WSPayloadReader {
    header: WSFrameHeader,
    read_idx: usize,
}

impl WSPayloadReader {
    pub fn from_header(header: WSFrameHeader) -> Self {
        Self {
            header,
            read_idx: 0,
        }
    }

    pub fn payload_len(&self) -> usize {
        self.header.payload_len
    }

    pub fn opcode(&self) -> WSOpcode {
        self.header.opcode
    }

    pub fn fin(&self) -> u8 {
        self.header.fin
    }

    pub fn has_masking(&self) -> bool {
        self.header.masking_key.is_some()
    }

    pub fn masking_key(&self) -> Option<&[u8; 4]> {
        self.header.masking_key.as_ref()
    }

    pub fn read_bytes_remaining(&self) -> usize {
        self.header.payload_len - self.read_idx
    }

    pub fn read_bytes(&self) -> usize {
        self.read_idx
    }

    pub fn is_complete(&self) -> bool {
        self.read_idx >= self.header.payload_len
    }

    pub fn decode_payload(&mut self, payload_dst: &mut [u8], payload_src: &[u8]) -> usize {
        let payload_rest = self.header.payload_len - self.read_idx;

        let size = min(min(payload_src.len(), payload_dst.len()), payload_rest);

        if let Some(masking_key_bytes) = self.header.masking_key {
            for i in 0..size {
                let j = (self.read_idx + i) % masking_key_bytes.len();
                let key_byte = masking_key_bytes[j];
                payload_dst[i] = payload_src[i] ^ key_byte;
            }
        } else {
            payload_dst[..size].copy_from_slice(&payload_src[..size]);
        }

        self.read_idx += size;
        size
    }

    pub fn decode_payload_in_place(&mut self, payload_src: &mut [u8]) -> usize {
        let payload_rest = self.header.payload_len - self.read_idx;
        let size = min(payload_src.len(), payload_rest);

        if let Some(masking_key_bytes) = self.header.masking_key {
            for i in 0..size {
                let j = (self.read_idx + i) % masking_key_bytes.len();
                let key_byte = masking_key_bytes[j];
                payload_src[i] ^= key_byte;
            }
        }

        self.read_idx += size;
        size
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;
    const REAL_WS_PACKET: [u8; 23] = [
        0b10000001, 0b10010001, 0b01101000, 0b00010010, 0b11110001, 0b00110110, 0b00100000,
        0b01110111, 0b10011101, 0b01011010, 0b00000111, 0b00110010, 0b10010111, 0b01000100,
        0b00000111, 0b01111111, 0b11010001, 0b01010101, 0b00000100, 0b01111011, 0b10010100,
        0b01011000, 0b00011100,
    ];

    #[test]
    fn test_read_real_ws_packet() {
        let (reading, read_size) = read_frame_header(&REAL_WS_PACKET).unwrap();
        assert_eq!(read_size, 6);
        assert_eq!(reading.fin, 1);
        assert_eq!(reading.opcode, WSOpcode::Text);
        assert_eq!(reading.payload_len, 17);
        assert_eq!(
            reading.masking_key,
            Some(0b01101000_00010010_11110001_00110110u32.to_be_bytes())
        );
    }

    // ContinuationFrame = 0x0,
    // Text = 0x1,
    // Binary = 0x2,
    // Close = 0x8,
    // Ping = 0x9,
    // Pong = 0xA,
    #[test]
    fn test_header_opcode_decoding_continuation_frame_h00() {
        let (reading, read_size) = read_frame_header(&[0b0000_0000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::ContinuationFrame);
    }

    #[test]
    fn test_header_opcode_decoding_text_h01() {
        let (reading, read_size) = read_frame_header(&[0b0000_0001, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::Text);
    }

    #[test]
    fn test_header_opcode_decoding_binary_h02() {
        let (reading, read_size) = read_frame_header(&[0b0000_0010, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::Binary);
    }

    #[test]
    fn test_header_opcode_decoding_close_h08() {
        let (reading, read_size) = read_frame_header(&[0b0000_1000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::Close);
    }

    #[test]
    fn test_header_opcode_decoding_ping_h09() {
        let (reading, read_size) = read_frame_header(&[0b0000_1001, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::Ping);
    }

    #[test]
    fn test_header_opcode_decoding_pong_h0a() {
        let (reading, read_size) = read_frame_header(&[0b0000_1010, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, WSOpcode::Pong);
    }

    #[test]
    fn test_header_opcode_decoding_invalid() {
        let Err(e) = read_frame_header(&[0b0000_0011, 0b0000_0000]) else {
            panic!("Expected error for invalid opcode");
        };
        assert_eq!(e, WebSocketProtoError::InvalidFrame);
    }

    #[test]
    fn test_header_fin_decoding_1() {
        let (reading, read_size) = read_frame_header(&[0b1000_0000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 1);
    }

    #[test]
    fn test_header_fin_decoding_0() {
        let (reading, read_size) = read_frame_header(&[0b0000_0000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
    }

    #[test]
    fn test_header_masking_key_decoding_none() {
        let (reading, read_size) = read_frame_header(&[0b0000_0000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.masking_key, None);
    }

    #[test]
    fn test_header_masking_key_decoding_some() {
        let (reading, read_size) =
            read_frame_header(&[0b0000_0000, 0b1000_0000, 0x12, 0x34, 0x56, 0x78]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE + WS_MASKING_KEY_LEN);
        assert_eq!(reading.masking_key.unwrap(), 0x12345678u32.to_be_bytes());
    }

    #[test]
    fn test_header_payload_len_decoding_le125() {
        let (reading, read_size) = read_frame_header(&[0b0000_0000, 0x7D]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.payload_len, 0x7Dusize);
    }

    #[test]
    fn test_header_payload_len_decoding_short() {
        let (reading, read_size) = read_frame_header(&[0b0000_0000, 0x7E, 0x01, 0x2C]).unwrap();
        assert_eq!(
            read_size,
            MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_SHORT
        );
        assert_eq!(reading.payload_len, 300usize);
    }

    #[test]
    fn test_header_payload_len_decoding_long() {
        let (reading, read_size) = read_frame_header(&[
            0b0000_0000,
            0x7F,
            0x01,
            0x23,
            0x45,
            0x67,
            0x89,
            0xab,
            0xcd,
            0xef,
        ])
        .unwrap();
        assert_eq!(
            read_size,
            MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_LONG
        );
        assert_eq!(reading.payload_len, 0x0123456789abcdefusize);
    }

    #[test]
    fn test_header_payload_short_len_not_enough_data() {
        let data = [0b0000_0000, 0x7E, 0x01 /*, 0x2C */];
        let Err(e) = read_frame_header(&data) else {
            panic!("Expected error for invalid frame");
        };
        assert_eq!(
            e,
            WebSocketProtoError::NotEnoughData(
                MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_SHORT
            )
        );
    }

    #[test]
    fn test_header_payload_long_len_not_enough_data() {
        let data = [
            0b0000_0000,
            0x7F,
            0x01,
            0x23,
            0x45,
            0x67,
            0x89,
            0xab,
            0xcd, /*0xef*/
        ];
        let Err(e) = read_frame_header(&data) else {
            panic!("Expected error for invalid frame");
        };
        assert_eq!(
            e,
            WebSocketProtoError::NotEnoughData(
                MIN_WS_FRAME_HEADER_SIZE + WS_EXTENDEDPAYLOAD_LEN_LONG
            )
        );
    }

    #[test]
    fn test_header_payload_len_invalid_value() {
        let Err(e) = read_frame_header(&[
            0b0000_0000,
            0x7F,
            0x81, // Most significant bit set
            0x23,
            0x45,
            0x67,
            0x89,
            0xab,
            0xcd,
            0xef,
        ]) else {
            panic!("Expected error for invalid frame");
        };
        assert_eq!(e, WebSocketProtoError::InvalidFrame);
    }

    #[test]
    fn test_read_frame_header_returns_not_enough_data_when_size_0() {
        let Err(e) = read_frame_header(&[0b0000_0000]) else {
            panic!("Expected NotEnoughData error");
        };
        assert_eq!(
            e,
            WebSocketProtoError::NotEnoughData(MIN_WS_FRAME_HEADER_SIZE)
        );
    }

    #[test]
    fn test_read_frame_header_returns_not_enough_data_when_size_1() {
        let Err(e) = read_frame_header(&[]) else {
            panic!("Expected NotEnoughData error");
        };
        assert_eq!(
            e,
            WebSocketProtoError::NotEnoughData(MIN_WS_FRAME_HEADER_SIZE)
        );
    }

    #[test]
    fn test_read_frame_header_returns_not_enough_data_when_masking_key_not_enough() {
        let Err(e) = read_frame_header(&[0b0000_0000, 0b1000_0000, 0x12, 0x34, 0x56 /*0x78*/])
        else {
            panic!("Expected NotEnoughData error");
        };
        assert_eq!(
            e,
            WebSocketProtoError::NotEnoughData(MIN_WS_FRAME_HEADER_SIZE + WS_MASKING_KEY_LEN)
        );
    }

    #[test]
    fn test_write_and_read_frame_header() {
        let mut buffer = [0u8; MAX_WS_FRAME_HEADER_SIZE];
        let fin = 1;
        let opcode = WSOpcode::Text;
        let payload_len = 300;
        let masking_key = Some(0xAABBCCDDu32.to_be_bytes());

        let header_size =
            write_frame_header(fin, opcode, payload_len, masking_key, &mut buffer).unwrap();

        let (reading, read_size) = read_frame_header(&buffer).unwrap();

        assert_eq!(header_size, read_size);
        assert_eq!(reading.fin, fin);
        assert_eq!(reading.opcode as u8, opcode as u8);
        assert_eq!(reading.payload_len, payload_len);
        assert_eq!(reading.masking_key, masking_key);
    }

    #[test]
    fn test_heder_reder_create() {
        let reader = WSHeaderReader::new();
        assert_eq!(reader.header_buf.len(), 0);
        assert_eq!(reader.expected_bytes, MIN_WS_FRAME_HEADER_SIZE);
    }

    #[test]
    fn test_header_reader_try_read_header_valid_full() {
        let data = [
            0b10000001, 0xFF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56,
            0x78,
        ];
        let mut reader = WSHeaderReader::new();

        let (header, read_size) = reader.try_read_header(&data).unwrap_ready();
        assert_eq!(read_size, data.len());
        assert_eq!(header.fin, 1);
        assert_eq!(header.opcode, WSOpcode::Text);
        assert_eq!(header.payload_len, 0x0123456789abcdefusize);
        assert_eq!(header.masking_key, Some(0x12345678u32.to_be_bytes()));
    }

    #[test]
    fn test_header_reader_try_read_header_valid_partial() {
        let data = [
            0b10000001, 0xFF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56,
            0x78,
        ];
        let mut reader = WSHeaderReader::new();

        let mut it = data.into_iter();
        for _ in 0..data.len() - 1 {
            let byte = [it.next().unwrap()];
            let read_bytes = reader.try_read_header(&byte).unwrap_pending();
            assert_eq!(read_bytes, 1);
        }

        // Now read the last byte
        let byte = [it.next().unwrap()];
        let (header, read_size) = reader.try_read_header(&byte).unwrap_ready();

        assert_eq!(read_size, data.len());
        assert_eq!(header.fin, 1);
        assert_eq!(header.opcode, WSOpcode::Text);
        assert_eq!(header.payload_len, 0x0123456789abcdefusize);
        assert_eq!(header.masking_key, Some(0x12345678u32.to_be_bytes()));
    }

    #[test]
    fn test_read_real_ws_packet_with_ws_stream_reader() {
        let mut src_buf = &REAL_WS_PACKET[..];

        let mut header_reader = WSHeaderReader::new();
        let (header, read_size) = header_reader.try_read_header(src_buf).unwrap_ready();
        assert_eq!(read_size, 6);
        assert_eq!(header.fin, 1);
        assert_eq!(header.opcode, WSOpcode::Text);
        assert_eq!(header.payload_len, 17);
        assert_eq!(
            header.masking_key.unwrap().as_slice(),
            [0b01101000u8, 0b00010010u8, 0b11110001u8, 0b00110110u8].as_slice()
        );
        // Remove consumed data from src_buf
        src_buf = &src_buf[read_size..];

        let mut reader = WSPayloadReader::from_header(header);
        assert_eq!(reader.payload_len(), 17);
        assert_eq!(reader.opcode(), WSOpcode::Text);
        assert_eq!(reader.fin(), 1);
        assert_eq!(reader.read_bytes(), 0);
        assert_eq!(reader.read_bytes_remaining(), 17);
        assert!(reader.has_masking());
        assert_eq!(
            reader.masking_key().unwrap().as_slice(),
            [0b01101000u8, 0b00010010u8, 0b11110001u8, 0b00110110u8].as_slice()
        );

        let mut payload_buf = [0u8; 17];
        let decoded_size = reader.decode_payload(&mut payload_buf, src_buf);
        assert!(reader.is_complete());
        assert_eq!(decoded_size, 17);
        assert_eq!(decoded_size, reader.payload_len());
        assert_eq!(reader.read_bytes(), 17);
        assert_eq!(reader.read_bytes_remaining(), 0);

        std::println!("{}", String::from_utf8_lossy(&payload_buf));

        let expected_payload: [u8; 17] = *b"Hello from client";

        //fmt::Debug::fmt(&payload_buf, &mut fmt::Formatter::new());
        assert_eq!(payload_buf, expected_payload);
    }

    #[test]
    fn test_read_and_write_packet() {
        let mut src_buf = &REAL_WS_PACKET[..];
        let mut decoded_payload_buf = [0u8; 17];
        let mut encoded_frame_buf = [0u8; REAL_WS_PACKET.len()];

        let mut header_reader = WSHeaderReader::new();
        let (header, decoded_header_size) = header_reader.try_read_header(src_buf).unwrap_ready();
        src_buf = &src_buf[decoded_header_size..];

        let mut reader = WSPayloadReader::from_header(header);
        let decoded_payload_size = reader.decode_payload(&mut decoded_payload_buf, src_buf);
        assert!(reader.is_complete());
        assert_eq!(decoded_payload_size, 17);
        assert_eq!(decoded_payload_size, reader.payload_len());

        let (mut writer, encoded_header_size) = WSStreamWriter::new(
            &mut encoded_frame_buf,
            reader.fin(),
            reader.opcode(),
            reader.payload_len(),
            WSMasked::with(reader.masking_key().unwrap().clone()),
        )
        .unwrap();

        let encoded_payload_size = writer
            .encode_payload(
                &mut encoded_frame_buf[encoded_header_size..],
                &decoded_payload_buf,
            )
            .unwrap();

        assert_eq!(encoded_header_size, decoded_header_size);
        assert_eq!(encoded_payload_size, decoded_payload_size);

        assert_eq!(
            &encoded_frame_buf[..encoded_header_size + encoded_payload_size],
            &REAL_WS_PACKET[..]
        );
    }
}
