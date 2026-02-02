use crate::web_socket::header::*;

pub struct WSEncodeWriter {
    free_space: usize,
    idx: usize,
    masking_key: MaskKey,
}

impl WSEncodeWriter {
    pub fn encode_header(
        out_buf: &mut [u8; MAX_WS_FRAME_HEADER_SIZE],
        fin: u8,
        opcode: WSOpcode,
        payload_len: usize,
        masking_key: [u8; 4],
    ) -> (Self, usize) {
        let header_size: usize =
            write_frame_header_with_mask_key(payload_len, out_buf, opcode, fin, masking_key);

        let result = Self {
            free_space: payload_len,
            idx: 0,
            masking_key,
        };

        (result, header_size)
    }

    /// Writes payload to the destination buffer, applying masking if necessary.
    /// This function assumes that the overall encoded_payload length does not exceed the allocated payload length in the header.
    /// Returns the number of bytes written to the payload_dst.
    /// ## Errors
    /// Returns Err(()) if the payload_src length is greater than the allocated payload length that left.
    ///
    pub fn encode_payload(
        &mut self,
        payload_dst: &mut [u8],
        payload_src: &[u8],
    ) -> Result<usize, ()> {
        if payload_src.len() > self.free_space {
            return Err(());
        }

        let mut dst = payload_dst[..payload_src.len()].iter_mut();
        let mut src = payload_src[..payload_src.len()].iter();
        let masking_key = self.masking_key;
        let mut transferred: usize = 0;

        while let Some(src_byte) = src.next()
            && let Some(dst_byte) = dst.next()
        {
            let j = self.idx % masking_key.len();
            let key_byte = masking_key[j];
            *dst_byte = *src_byte ^ key_byte;
            self.idx += 1;
            transferred += 1;
        }

        self.free_space -= transferred;
        Ok(transferred)
    }

    /// Writes payload to the provided buffer in place, applying masking if necessary.
    /// This function assumes that the overall encoded_payload length does not exceed the allocated payload length in the header.
    /// Returns the number of bytes written to the payload_dst.
    /// ## Errors
    /// Returns Err(()) if the payload_src length is greater than the allocated payload length that left.
    ///
    pub fn encode_payload_in_place(&mut self, payload_buf: &mut [u8]) -> Result<usize, ()> {
        if payload_buf.len() > self.free_space {
            return Err(());
        }

        let mut buf = payload_buf.iter_mut();
        let masking_key = self.masking_key;
        let mut transferred: usize = 0;

        while let Some(buf_byte) = buf.next() {
            let j = self.idx % masking_key.len();
            let key_byte = masking_key[j];
            *buf_byte = *buf_byte ^ key_byte;
            self.idx += 1;
            transferred += 1;
        }

        self.free_space -= transferred;
        Ok(transferred)
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;
}
