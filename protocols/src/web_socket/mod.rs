/// WebSocket protocol header
pub mod header;

/// WebSocket header reader module.
pub mod header_reader;

/// WebSocket header writer module.
pub mod header_writer;

/// Websocket implementation.
pub mod web_socket;

/// Test utilities for WebSocket.
#[cfg(test)]
pub mod test_utils;

pub const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// Tests
#[cfg(test)]
mod tests {
    use super::test_utils::*;
    use crate::web_socket::header::*;
    use crate::web_socket::header_reader::*;
    use crate::web_socket::header_writer::*;

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

        let mut header = [0u8; MAX_WS_FRAME_HEADER_SIZE];
        let (mut writer, encoded_header_size) = WSEncodeWriter::encode_header(
            reader.payload_len(),
            &mut header,
            reader.opcode(),
            reader.fin(),
            reader.masking_key().unwrap().clone(),
        );
        encoded_frame_buf[..encoded_header_size].copy_from_slice(&header[..encoded_header_size]);

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
