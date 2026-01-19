use crate::find_sequence::FindSequence;

pub trait HttpReadStreamError {}

impl HttpReadStreamError for () {}

/// Errors that can occur while reading from the stream
#[derive(Debug)]
pub enum ReadStreamError<ReadError: HttpReadStreamError> {
    /// Target buffer overflowed while reading
    TargetBufferOverflow,
    /// Error occurred while reading from the stream
    ReadError(ReadError),
}

impl<ReadError: HttpReadStreamError> From<ReadError> for ReadStreamError<ReadError> {
    fn from(err: ReadError) -> Self {
        ReadStreamError::ReadError(err)
    }
}

pub trait ReadStream {
    type ReadError: HttpReadStreamError;

    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::ReadError>> + Send
    where
        F: FnMut(&[u8]) -> (usize, R) + Send;

    /// This function reads from the stream to the buffer until the delimiter predicate returns true.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The byte on which the delimiter predicate returns true is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the delimitter.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.s
    ///
    fn read_till_delimitter<Delimitter>(
        &mut self,
        buffer: &mut [u8],
        mut delimitter: Delimitter,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
        Delimitter: FnMut(u8) -> bool + Send,
    {
        async move {
            let mut write_size = 0;

            while write_size < buffer.len() {
                let delimitter_found = self
                    .read_with(|data: &[u8]| {
                        let to_read = core::cmp::min(buffer.len() - write_size, data.len());

                        let mut actual_read: usize = 0;

                        for &b in &data[..to_read] {
                            actual_read += 1;
                            buffer[write_size] = b;
                            write_size += 1;

                            if delimitter(b) {
                                // The delimitter has been found; stop reading further

                                return (actual_read, true);
                            }
                        }

                        (to_read, false)
                    })
                    .await?;

                if delimitter_found {
                    return Ok(write_size);
                }
            }

            Err(ReadStreamError::TargetBufferOverflow)
        }
    }

    /// This function reads from the stream to the buffer until the specified delimitter sequence is found.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The delimiter is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the delimiter.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.s
    ///
    fn read_till_delimitter_sequence(
        &mut self,
        delimitter: &[u8],
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
    {
        async move {
            let mut delimiter_finder = FindSequence::new(delimitter);
            self.read_till_delimitter(buffer, |b| delimiter_finder.push_byte(b))
                .await
        }
    }

    /// This function reads from the stream to the buffer until the specified delimitter byte is found.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The delimiter byte is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the delimiter.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.s
    ///
    fn read_till_delimitter_byte(
        &mut self,
        delimitter: u8,
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
    {
        async move { self.read_till_delimitter(buffer, |b| b == delimitter).await }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    pub struct DummyReadStream<'a> {
        buffer: &'a [u8],
        position: usize,
    }

    impl<'a> DummyReadStream<'a> {
        pub fn new(buffer: &'a [u8]) -> Self {
            Self {
                buffer,
                position: 0,
            }
        }
    }

    #[derive(Debug)]
    pub struct EOF;

    impl HttpReadStreamError for EOF {}

    impl<'a> ReadStream for DummyReadStream<'a> {
        type ReadError = EOF;

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&[u8]) -> (usize, R) + Send,
        {
            let size = self.buffer.len() - self.position;
            if self.position >= size {
                return Err(EOF);
            }

            let data = &self.buffer[self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    pub struct DummyMultipartReadStream<'a> {
        multipart_buffer: &'a [&'a [u8]],
        part: usize,
        position: usize,
    }

    impl<'a> DummyMultipartReadStream<'a> {
        pub fn new(multipart_buffer: &'a [&'a [u8]]) -> Self {
            Self {
                multipart_buffer,
                part: 0,
                position: 0,
            }
        }
    }

    impl<'a> ReadStream for DummyMultipartReadStream<'a> {
        type ReadError = ();

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&[u8]) -> (usize, R) + Send,
        {
            if self.part >= self.multipart_buffer.len() {
                return Err(());
            }

            if self.position >= self.multipart_buffer[self.part].len() {
                self.part += 1;
                self.position = 0;
                if self.part >= self.multipart_buffer.len() {
                    return Err(());
                }
            }

            let data = &self.multipart_buffer[self.part][self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    #[tokio::test]
    async fn test_read() {
        let request_data = b"Hello, World!\r\nThis is a test.\r\n";
        let mut stream = DummyReadStream::new(request_data);
        let mut buffer = [0u8; 64];

        let bytes_read = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect("Expect no error");

        assert_eq!(&buffer[..bytes_read], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_read_eof_when_no_dilimitter_found() {
        let request_data = b"Hello, World!";
        let mut stream = DummyReadStream::new(request_data);
        let mut buffer = [0u8; 64];

        let error = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect_err("Expect read error, due to read stream EOF");

        assert!(matches!(error, ReadStreamError::ReadError(EOF)));
    }

    #[tokio::test]
    async fn test_read_buffer_overflow() {
        let request_data = b"Hello, World!\r\n";
        let mut stream = DummyReadStream::new(request_data);
        let mut buffer = [0u8; 4];

        let error = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect_err("Expect buffer overflow error");

        assert!(matches!(error, ReadStreamError::TargetBufferOverflow));
    }
}
