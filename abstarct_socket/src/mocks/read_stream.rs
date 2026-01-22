pub use crate::mocks::error::DummySocketError;
pub use crate::read_stream::ReadStream;

/// A dummy read stream for testing purposes
pub struct DummyReadStream<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> DummyReadStream<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }
}

impl<'a> ReadStream for DummyReadStream<'a> {
    type Error = DummySocketError;
    async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::Error>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        if self.position >= self.buffer.len() {
            return Err(DummySocketError::ConnectionReset);
        }

        let data = &mut self.buffer[self.position..];
        let (read_bytes, res) = f(data);
        self.position += read_bytes;
        Ok(res)
    }
}
