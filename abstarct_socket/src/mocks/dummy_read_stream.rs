pub use crate::mocks::eof::EOF;
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
    type ReadError = EOF;

    async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        if self.position >= self.buffer.len() {
            return Err(EOF);
        }

        let data = &mut self.buffer[self.position..];
        let (read_bytes, res) = f(data);
        self.position += read_bytes;
        Ok(res)
    }
}

//TODO: Add tests for DummyReadStream
