use crate::find_sequence::FindSequence;

/// Marker trait for read stream errors. Marks types that can be converted into HTTP errors.
/// Convertion are done via aggregation in the ReadStreamError::ReadError() error.
pub trait IntoHttpError {}

impl IntoHttpError for () {}

/// Errors that can occur while reading from the stream
#[derive(Debug)]
pub enum ReadStreamError<ReadError: IntoHttpError> {
    /// Target buffer overflowed while reading
    TargetBufferOverflow,
    /// Error occurred while reading from the stream
    ReadError(ReadError),
}

impl<ReadError: IntoHttpError> From<ReadError> for ReadStreamError<ReadError> {
    fn from(err: ReadError) -> Self {
        ReadStreamError::ReadError(err)
    }
}

/// Trait representing a read stream interface
pub trait ReadStream {
    type ReadError: IntoHttpError;

    /// Read data from the stream into the provided buffer
    ///
    /// ## Errors
    /// - Returns `Self::ReadError` if an error occurs while reading from the stream
    ///
    fn read(
        &mut self,
        buf: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::ReadError>> + Send {
        self.read_with(|data: &mut [u8]| {
            let to_read = core::cmp::min(buf.len(), data.len());
            buf[..to_read].copy_from_slice(&data[..to_read]);
            (to_read, to_read)
        })
    }

    /// Read from the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes read and a result value.
    ///
    /// ## Errors
    /// - Returns `Self::ReadError` if an error occurs while reading from the stream.
    ///
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::ReadError>> + Send
    where
        F: FnMut(&mut [u8]) -> (usize, R) + Send;
}

pub trait ReadStreamExt: ReadStream {
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
                    .read_with(|data: &mut [u8]| {
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
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.
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

    /// This function skips the specified number of bytes from the stream.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    ///   
    fn consume(
        &mut self,
        size: usize,
    ) -> impl core::future::Future<Output = Result<(), ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
    {
        async move {
            let mut bytes_to_consume = size;

            while bytes_to_consume > 0 {
                let bytes_read = self
                    .read_with(|data: &mut [u8]| {
                        let to_read = core::cmp::min(bytes_to_consume, data.len());
                        (to_read, to_read)
                    })
                    .await?;
                bytes_to_consume -= bytes_read;
            }

            Ok(())
        }
    }

    /// This function consumes bytes from the stream until the delimitter predicate returns true.
    ///
    /// ## Note: The byte on which the delimitter predicate returns true is also skipped.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_delimitter<Delimitter>(
        &mut self,
        mut delimitter: Delimitter,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
        Delimitter: FnMut(u8) -> bool + Send,
    {
        async move {
            let mut total_skipped = 0;

            while self
                .read_with(|data: &mut [u8]| {
                    for (i, &b) in data.iter().enumerate() {
                        if delimitter(b) {
                            // The delimitter has been found; stop reading further
                            let actually_skipped = i + 1;
                            total_skipped += actually_skipped;
                            return (actually_skipped, false);
                        }
                    }

                    total_skipped += data.len();
                    (data.len(), true)
                })
                .await?
            {}

            return Ok(total_skipped);
        }
    }

    /// This function consumes bytes from the stream until the specified delimitter byte is found.
    ///
    /// ## Note: The delimitter byte is also skipped.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_delimitter_byte(
        &mut self,
        delimitter: u8,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
    {
        async move { self.consume_till_delimitter(|b| b == delimitter).await }
    }

    /// This function consumes bytes from the stream until the specified delimitter sequence is found.
    ///     
    /// ## Note: The delimitter sequence is also skipped.
    ///     
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_delimitter_sequence(
        &mut self,
        delimitter: &[u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> + Send
    where
        Self: Send,
    {
        async move {
            let mut delimiter_finder = FindSequence::new(delimitter);
            self.consume_till_delimitter(|b| delimiter_finder.push_byte(b))
                .await
        }
    }
}

impl<T: ReadStream + ?Sized> ReadStreamExt for T {}

#[cfg(test)]
pub mod tests {
    use super::*;

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

    /// EOF error for the dummy read stream
    #[derive(Debug)]
    pub struct EOF;

    impl IntoHttpError for EOF {}

    impl<'a> ReadStream for DummyReadStream<'a> {
        type ReadError = EOF;

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&mut [u8]) -> (usize, R) + Send,
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

    /// A dummy multipart read stream for testing purposes
    pub struct DummyMultipartReadStream {
        multipart_buffer: Vec<Vec<u8>>,
        part: usize,
        position: usize,
    }

    impl DummyMultipartReadStream {
        pub fn new(multipart_buffer: &Vec<Vec<u8>>) -> Self {
            Self {
                multipart_buffer: multipart_buffer.clone(),
                part: 0,
                position: 0,
            }
        }
    }

    impl ReadStream for DummyMultipartReadStream {
        type ReadError = ();

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&mut [u8]) -> (usize, R) + Send,
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

            let data = &mut self.multipart_buffer[self.part][self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    #[tokio::test]
    async fn test_default_read_implementation() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_read() {
        let mut request_data = b"Hello, World!\r\nThis is a test.\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let bytes_read = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect("Expect no error");

        assert_eq!(&buffer[..bytes_read], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_read_delimitter_only() {
        let mut request_data = b"\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let bytes_read = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect("Expect no error");

        assert_eq!(&buffer[..bytes_read], b"\r\n");
    }

    #[tokio::test]
    async fn test_read_eof_when_no_dilimitter_found() {
        let mut request_data = b"Hello, World!".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let error = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect_err("Expect read error, due to read stream EOF");

        assert!(matches!(error, ReadStreamError::ReadError(EOF)));
    }

    #[tokio::test]
    async fn test_read_buffer_overflow() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 4];

        let error = stream
            .read_till_delimitter_sequence(b"\r\n", &mut buffer)
            .await
            .expect_err("Expect buffer overflow error");

        assert!(matches!(error, ReadStreamError::TargetBufferOverflow));
    }

    #[tokio::test]
    async fn test_skip_bytes() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        stream.consume(7).await.expect("Expect no error");

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_delimitter() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        stream
            .consume_till_delimitter(|b| b == b',')
            .await
            .expect("Expect no error");

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b" World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_till_delimitter_sequence() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        stream
            .consume_till_delimitter_sequence(b", Wo")
            .await
            .expect("Expect no error");

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"rld!\r\n");
    }
}
