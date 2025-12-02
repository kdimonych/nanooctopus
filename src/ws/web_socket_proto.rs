use core::cmp::min;

use modular_bitfield::prelude::*;

pub const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MIN_WS_FRAME_HEADER_SIZE: usize = 2;
pub const MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT: usize = 2;
pub const MAX_WS_EXTENDEDPAYLOAD_LEN_LONG: usize = 8;
pub const MAX_WS_MASKING_KEY_LEN: usize = 4;
pub const MAX_WS_FRAME_HEADER_SIZE: usize =
    MIN_WS_FRAME_HEADER_SIZE + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG + MAX_WS_MASKING_KEY_LEN; // 2 + 8 + 4
pub const MASKING_KEY_LEN: usize = 4;

pub type MaskKey = [u8; MASKING_KEY_LEN];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebSocketProtoError {
    InvalidFrame(),
}

#[derive(Specifier, Debug, Clone, Copy, PartialEq)]
#[bits = 4]
pub enum Opcode {
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
    opcode: Opcode,
    #[skip]
    __: B3,
    fin: B1,

    // Byte 1
    payload_len: B7,
    mask: B1,
}

fn write_frame_header(
    fin: u8,
    opcode: Opcode,
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
        if buffer.len() < MIN_WS_FRAME_HEADER_SIZE + MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT {
            return Err(());
        }

        header.set_payload_len(126);
        buffer[index..index + MIN_WS_FRAME_HEADER_SIZE].copy_from_slice(&header.into_bytes());
        index += MIN_WS_FRAME_HEADER_SIZE;

        let payload_len_sort = payload_len as u16;
        buffer[index..index + MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT]
            .copy_from_slice(&payload_len_sort.to_be_bytes());
        index += 2;
    } else {
        if buffer.len() < MIN_WS_FRAME_HEADER_SIZE + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG {
            return Err(());
        }

        header.set_payload_len(127);
        buffer[index..index + MIN_WS_FRAME_HEADER_SIZE].copy_from_slice(&header.into_bytes());
        index += MIN_WS_FRAME_HEADER_SIZE;

        buffer[index..index + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG]
            .copy_from_slice(&payload_len.to_be_bytes());
        index += MAX_WS_EXTENDEDPAYLOAD_LEN_LONG;
    }

    if let Some(mask) = masking_key {
        if buffer.len() < index + MAX_WS_MASKING_KEY_LEN {
            return Err(());
        }
        buffer[index..index + MAX_WS_MASKING_KEY_LEN].copy_from_slice(&mask);
        index += MAX_WS_MASKING_KEY_LEN;
    }

    Ok(index)
}

struct WebSocketFrameHeader {
    fin: u8,
    opcode: Opcode,
    payload_len: usize,
    masking_key: Option<MaskKey>,
}

fn read_frame_header(buffer: &[u8]) -> Result<(WebSocketFrameHeader, usize), WebSocketProtoError> {
    if buffer.len() < MIN_WS_FRAME_HEADER_SIZE {
        return Err(WebSocketProtoError::InvalidFrame());
    }

    let header = WebSocketFrameHeaderPacked::from_bytes([buffer[0], buffer[1]]);
    let mut index = MIN_WS_FRAME_HEADER_SIZE;

    let payload_len = match header.payload_len() {
        len @ 0..=125 => len as usize,
        126 => {
            if buffer.len() < index + MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT {
                return Err(WebSocketProtoError::InvalidFrame());
            }

            // The value is stored in network byte order (big-endian)
            let extended_length: u16 = u16::from_be_bytes(
                buffer[index..index + MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT]
                    .try_into()
                    .unwrap(),
            );
            index += MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT;
            extended_length as usize
        }
        127 => {
            if buffer.len() < index + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG {
                return Err(WebSocketProtoError::InvalidFrame());
            }
            // The value is stored in network byte order (big-endian)
            let extended_length = u64::from_be_bytes(
                buffer[index..index + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG]
                    .try_into()
                    .unwrap(),
            );
            if extended_length >> 63 != 0 {
                // Most significant bit must be 0
                return Err(WebSocketProtoError::InvalidFrame());
            }

            index += MAX_WS_EXTENDEDPAYLOAD_LEN_LONG;
            extended_length as usize
        }
        _ => return Err(WebSocketProtoError::InvalidFrame()),
    };

    let masking_key = if header.mask() == 1 {
        let Ok(res) = MaskKey::try_from(&buffer[index..index + MAX_WS_MASKING_KEY_LEN]) else {
            return Err(WebSocketProtoError::InvalidFrame());
        };
        index += MAX_WS_MASKING_KEY_LEN;

        Some(res)
    } else {
        None
    };

    let reading = WebSocketFrameHeader {
        fin: header.fin(),
        opcode: header
            .opcode_or_err()
            .map_err(|_| WebSocketProtoError::InvalidFrame())?,
        payload_len,
        masking_key,
    };

    Ok((reading, index))
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
        opcode: Opcode,
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

pub struct WSStreamReader {
    header: WebSocketFrameHeader,
    masking_idx: usize,
}

impl WSStreamReader {
    pub fn from_data(src_buf: &[u8]) -> Result<(Self, usize), ()> {
        let (header, read_size) = read_frame_header(src_buf).map_err(|_| ())?;
        Ok((
            Self {
                header,
                masking_idx: 0,
            },
            read_size,
        ))
    }

    pub fn payload_len(&self) -> usize {
        self.header.payload_len
    }

    pub fn opcode(&self) -> Opcode {
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

    pub fn decode_payload(
        &mut self,
        payload_dst: &mut [u8],
        payload_src: &[u8],
    ) -> Result<usize, ()> {
        let free_space = self.header.payload_len - self.masking_idx;
        if payload_src.len() > free_space {
            return Err(());
        }

        let size = min(min(payload_src.len(), payload_dst.len()), free_space);

        if let Some(masking_key_bytes) = self.header.masking_key {
            for i in 0..size {
                let j = (self.masking_idx + i) % masking_key_bytes.len();
                let key_byte = masking_key_bytes[j];
                payload_dst[i] = payload_src[i] ^ key_byte;
            }
        } else {
            payload_dst[..size].copy_from_slice(&payload_src[..size]);
        }

        self.masking_idx += size;
        Ok(size)
    }
}

// Tests
#[cfg(test)]
mod tests {
    use core::fmt;

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
        assert_eq!(reading.opcode, Opcode::Text);
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
        assert_eq!(reading.opcode, Opcode::ContinuationFrame);
    }

    #[test]
    fn test_header_opcode_decoding_text_h01() {
        let (reading, read_size) = read_frame_header(&[0b0000_0001, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, Opcode::Text);
    }

    #[test]
    fn test_header_opcode_decoding_binary_h02() {
        let (reading, read_size) = read_frame_header(&[0b0000_0010, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, Opcode::Binary);
    }

    #[test]
    fn test_header_opcode_decoding_close_h08() {
        let (reading, read_size) = read_frame_header(&[0b0000_1000, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, Opcode::Close);
    }

    #[test]
    fn test_header_opcode_decoding_ping_h09() {
        let (reading, read_size) = read_frame_header(&[0b0000_1001, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, Opcode::Ping);
    }

    #[test]
    fn test_header_opcode_decoding_pong_h0a() {
        let (reading, read_size) = read_frame_header(&[0b0000_1010, 0b0000_0000]).unwrap();
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE);
        assert_eq!(reading.fin, 0);
        assert_eq!(reading.opcode, Opcode::Pong);
    }

    #[test]
    fn test_header_opcode_decoding_invalid() {
        let Err(e) = read_frame_header(&[0b0000_0011, 0b0000_0000]) else {
            panic!("Expected error for invalid opcode");
        };
        assert_eq!(e, WebSocketProtoError::InvalidFrame());
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
        assert_eq!(read_size, MIN_WS_FRAME_HEADER_SIZE + MAX_WS_MASKING_KEY_LEN);
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
            MIN_WS_FRAME_HEADER_SIZE + MAX_WS_EXTENDEDPAYLOAD_LEN_SHORT
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
            MIN_WS_FRAME_HEADER_SIZE + MAX_WS_EXTENDEDPAYLOAD_LEN_LONG
        );
        assert_eq!(reading.payload_len, 0x0123456789abcdefusize);
    }

    #[test]
    fn test_header_payload_len_invalid_field_len() {
        let Err(e) =
            read_frame_header(&[0b0000_0000, 0x7F, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd])
        else {
            panic!("Expected error for invalid frame");
        };
        assert_eq!(e, WebSocketProtoError::InvalidFrame());
    }

    #[test]
    fn test_header_payload_len_invalid_value() {
        let Err(e) = read_frame_header(&[
            0b0000_0000,
            0x7F,
            0x81,
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
        assert_eq!(e, WebSocketProtoError::InvalidFrame());
    }

    #[test]
    fn test_write_and_read_frame_header() {
        let mut buffer = [0u8; MAX_WS_FRAME_HEADER_SIZE];
        let fin = 1;
        let opcode = Opcode::Text;
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
    fn test_read_real_ws_packet_with_ws_stream_reader() {
        let (mut reader, read_size) = WSStreamReader::from_data(&REAL_WS_PACKET).unwrap();
        assert_eq!(read_size, 6);
        assert_eq!(reader.payload_len(), 17);
        assert_eq!(reader.opcode(), Opcode::Text);
        assert_eq!(reader.fin(), 1);
        assert!(reader.has_masking());
        assert_eq!(
            reader.masking_key().unwrap(),
            &0b01101000_00010010_11110001_00110110u32.to_be_bytes()
        );

        let mut payload_buf = [0u8; 17];
        let decoded_size = reader
            .decode_payload(&mut payload_buf, &REAL_WS_PACKET[read_size..])
            .unwrap();
        assert_eq!(decoded_size, 17);
        assert_eq!(decoded_size, reader.payload_len());

        std::println!("{}", String::from_utf8_lossy(&payload_buf));

        let expected_payload: [u8; 17] = *b"Hello from client";

        //fmt::Debug::fmt(&payload_buf, &mut fmt::Formatter::new());
        assert_eq!(payload_buf, expected_payload);
    }

    #[test]
    fn test_read_and_write_packet() {
        let mut decoded_payload_buf = [0u8; 17];
        let mut encoded_frame_buf = [0u8; REAL_WS_PACKET.len()];

        let (mut reader, decoded_header_size) = WSStreamReader::from_data(&REAL_WS_PACKET).unwrap();
        let decoded_payload_size = reader
            .decode_payload(
                &mut decoded_payload_buf,
                &REAL_WS_PACKET[decoded_header_size..],
            )
            .unwrap();
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
