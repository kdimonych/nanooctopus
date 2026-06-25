mod binary_matcher;
mod frame_header_matcher;

use super::socket_mock::MockError;
use edge_ws::{Error as WsError, FrameHeader, FrameType};

use crate::web_socket::masker::PayloadMasker;
pub use binary_matcher::BinaryMatcher;
pub use frame_header_matcher::FrameHeaderMatcher;

pub struct FrameHeaderBuilder {
    frame_header: FrameHeader,
}

impl Default for FrameHeaderBuilder {
    fn default() -> FrameHeaderBuilder {
        Self::new(FrameType::Text(false))
    }
}

impl FrameHeaderBuilder {
    pub const fn new(frame_type: FrameType) -> Self {
        Self {
            frame_header: FrameHeader {
                frame_type,
                payload_len: 0,
                mask_key: None,
            },
        }
    }

    pub const fn with_mask_key(mut self, mask_key: Option<u32>) -> Self {
        self.frame_header.mask_key = mask_key;
        self
    }

    pub const fn with_payload_len(mut self, payload_len: u64) -> Self {
        self.frame_header.payload_len = payload_len;
        self
    }

    pub fn build(self) -> impl FnMut(&mut [u8]) -> Result<usize, MockError> {
        let mut header_buf = [0u8; FrameHeader::MAX_LEN];
        let serialized = self
            .frame_header
            .serialize(&mut header_buf)
            .expect("buf len must be sufficient to store the header");

        BinaryFrame::new(&header_buf[..serialized]).build()
    }
}

pub struct MaskedBinaryFrame {
    data: Vec<u8>,
    masker: Option<PayloadMasker>,
}

impl MaskedBinaryFrame {
    pub fn new<Data: Into<Vec<u8>>>(data: Data) -> Self {
        Self {
            data: data.into(),
            masker: None,
        }
    }

    pub fn with_mask_key(mut self, mask_key: Option<u32>) -> Self {
        self.masker = mask_key.map(PayloadMasker::new);
        self
    }

    pub fn build(mut self) -> impl FnMut(&mut [u8]) -> Result<usize, MockError> {
        if let Some(mut masker) = self.masker.take() {
            masker.mask_chank(self.data.as_mut_slice());
        }

        BinaryFrame::new(self.data).build()
    }
}

pub struct BinaryFrame {
    data: Vec<u8>,
}

impl BinaryFrame {
    pub fn new<Data: Into<Vec<u8>>>(data: Data) -> Self {
        Self { data: data.into() }
    }

    pub fn build(self) -> impl FnMut(&mut [u8]) -> Result<usize, MockError> {
        let mut offset = 0;
        move |buf: &mut [u8]| {
            if offset >= self.data.len() {
                return Err(MockError {});
            }
            let not_written_len = self.data.len() - offset;
            let len = not_written_len.min(buf.len());
            buf[..len].copy_from_slice(&self.data[offset..offset + len]);
            offset += len;
            Ok(len)
        }
    }
}

/// Estimate the number of read calls needed to read the frame header based on its content.
pub fn estimate_read_count(frame_header: &FrameHeader) -> usize {
    let mut buf = [0u8; FrameHeader::MAX_LEN];
    frame_header
        .serialize(&mut buf)
        .expect("buf len must be sufficient to store the header");

    let mut read_end = FrameHeader::MIN_LEN;
    let mut read_count = 0;

    loop {
        read_count += 1;
        match FrameHeader::deserialize(&buf[..read_end]) {
            Ok(_) => return read_count,
            Err(WsError::Incomplete(more)) => {
                read_end += more;
            }
            Err(e) => unreachable!("unexpected error during header serialization: {e:?}"),
        }
    }
}
