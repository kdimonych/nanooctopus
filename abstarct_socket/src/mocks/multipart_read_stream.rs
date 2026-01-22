pub use crate::mocks::error::DummySocketError;
pub use crate::read_stream::ReadStream;

extern crate std;

/// A dummy multipart read stream for testing purposes
pub struct DummyMultipartReadStream {
    multipart_buffer: std::vec::Vec<std::vec::Vec<u8>>,
    part: usize,
    position: usize,
}
impl DummyMultipartReadStream {
    pub fn new(multipart_buffer: &std::vec::Vec<std::vec::Vec<u8>>) -> Self {
        Self {
            multipart_buffer: multipart_buffer.clone(),
            part: 0,
            position: 0,
        }
    }
}

impl ReadStream for DummyMultipartReadStream {
    type Error = DummySocketError;
    async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::Error>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        if self.part >= self.multipart_buffer.len() {
            return Err(DummySocketError::ConnectionReset);
        }

        if self.position >= self.multipart_buffer[self.part].len() {
            self.part += 1;
            self.position = 0;
            if self.part >= self.multipart_buffer.len() {
                return Err(DummySocketError::ConnectionReset);
            }
        }

        let data = &mut self.multipart_buffer[self.part][self.position..];
        let (read_bytes, res) = f(data);
        self.position += read_bytes;
        Ok(res)
    }
}

//TODO: Add tests for DummyMultipartReadStream
