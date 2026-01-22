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
    fn read<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::ReadError>> + 's {
        self.read_with(|data: &mut [u8]| {
            let to_read = core::cmp::min(buf.len(), data.len());
            buf[..to_read].copy_from_slice(&data[..to_read]);
            (to_read, to_read)
        })
    }

    /// Read exactly `buf.len()` bytes from the stream into the provided buffer
    ///     
    /// ## Errors
    /// - Returns `Self::ReadError` if an error occurs while reading from the stream
    ///
    fn read_exact<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<(), Self::ReadError>> + 's {
        async move {
            let mut total_read = 0;

            while total_read < buf.len() {
                let bytes_read = self.read(&mut buf[total_read..]).await?;
                total_read += bytes_read;
            }

            Ok(())
        }
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
    ) -> impl core::future::Future<Output = Result<R, Self::ReadError>>
    where
        F: FnMut(&mut [u8]) -> (usize, R);
}

/// Embassy-net based implementation of ReadStream for TcpSocket
//#[cfg(all(target_arch = "arm", target_os = "none", feature = "embassy_net"))]
#[cfg(feature = "embassy_net")]
mod embassy_impl {
    use super::*;
    use embassy_net::tcp::{TcpReader, TcpSocket};

    impl IntoHttpError for embassy_net::tcp::Error {}

    // Embassy-net based ReadStream implementation for TcpReader
    impl<'socket> ReadStream for TcpSocket<'socket> {
        type ReadError = embassy_net::tcp::Error;

        fn read_with<F, R>(
            &mut self,
            f: F,
        ) -> impl core::future::Future<Output = Result<R, Self::ReadError>>
        where
            F: FnMut(&mut [u8]) -> (usize, R),
        {
            self.read_with(f)
        }

        fn read<'s>(
            &'s mut self,
            buf: &'s mut [u8],
        ) -> impl core::future::Future<Output = Result<usize, Self::ReadError>> + 's {
            self.read(buf)
        }
    }

    // Embassy-net based implementation of ReadStream for TcpReader
    impl<'socket> ReadStream for TcpReader<'socket> {
        type ReadError = embassy_net::tcp::Error;

        fn read_with<F, R>(
            &mut self,
            f: F,
        ) -> impl core::future::Future<Output = Result<R, Self::ReadError>>
        where
            F: FnMut(&mut [u8]) -> (usize, R),
        {
            self.read_with(f)
        }

        fn read<'s>(
            &'s mut self,
            buf: &'s mut [u8],
        ) -> impl core::future::Future<Output = Result<usize, Self::ReadError>> + 's {
            self.read(buf)
        }
    }
}

pub trait ReadStreamExt: ReadStream {
    /// This function reads from the stream to the buffer until the stop predicate returns true.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The byte on which the stop predicate returns true is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop predicate.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop<StopPredicate>(
        &mut self,
        buffer: &mut [u8],
        mut stop_predicate: StopPredicate,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>>
    where
        StopPredicate: FnMut(u8) -> bool,
    {
        async move {
            let mut write_size = 0;

            while write_size < buffer.len() {
                let stop_triggered = self
                    .read_with(|data: &mut [u8]| {
                        let to_read = core::cmp::min(buffer.len() - write_size, data.len());

                        let mut actual_read: usize = 0;

                        for &b in &data[..to_read] {
                            actual_read += 1;
                            buffer[write_size] = b;
                            write_size += 1;

                            if stop_predicate(b) {
                                // The stop predicate has been triggered; stop reading further
                                return (actual_read, true);
                            }
                        }

                        (to_read, false)
                    })
                    .await?;

                if stop_triggered {
                    return Ok(write_size);
                }
            }

            Err(ReadStreamError::TargetBufferOverflow)
        }
    }

    /// This function reads from the stream to the buffer until the specified stop sequence is found.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The stop sequence is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop sequence.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop_sequence(
        &mut self,
        stop_sequence: &[u8],
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> {
        async move {
            let mut stop_sequence_finder = FindSequence::new(stop_sequence);
            self.read_till_stop(buffer, |b| stop_sequence_finder.push_byte(b))
                .await
        }
    }

    /// This function reads from the stream to the buffer until the specified stop byte is found.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The stop byte is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop byte.
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop_byte(
        &mut self,
        stop_byte: u8,
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> {
        async move { self.read_till_stop(buffer, |b| b == stop_byte).await }
    }

    /// This function consumes the specified number of bytes from the stream.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    ///   
    fn consume(
        &mut self,
        size: usize,
    ) -> impl core::future::Future<Output = Result<(), ReadStreamError<Self::ReadError>>> {
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

    /// This function consumes bytes from the stream until the stop predicate returns true.
    ///
    /// ## Note: The byte on which the stop predicate returns true is also consumed.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop<Stop>(
        &mut self,
        mut stop: Stop,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>>
    where
        Stop: FnMut(u8) -> bool,
    {
        async move {
            let mut total_consumed = 0;

            while self
                .read_with(|data: &mut [u8]| {
                    for (i, &b) in data.iter().enumerate() {
                        if stop(b) {
                            // The stop point has been found; stop reading further
                            let actually_consumed = i + 1;
                            total_consumed += actually_consumed;
                            return (actually_consumed, false);
                        }
                    }

                    total_consumed += data.len();
                    (data.len(), true)
                })
                .await?
            {}

            return Ok(total_consumed);
        }
    }

    /// This function consumes bytes from the stream until the specified stop byte is found.
    ///
    /// ## Note: The stop byte is also consumed.
    ///
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop_byte(
        &mut self,
        stop_byte: u8,
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> {
        async move { self.consume_till_stop(|b| b == stop_byte).await }
    }

    /// This function consumes bytes from the stream until the specified stop sequence is found.
    ///     
    /// ## Note: The stop sequence is also consumed.
    ///     
    /// ## Errors
    /// - Returns `ReadStreamError::ReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop_sequence(
        &mut self,
        stop_sequence: &[u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadStreamError<Self::ReadError>>> {
        async move {
            let mut stop_sequence_finder = FindSequence::new(stop_sequence);
            self.consume_till_stop(|b| stop_sequence_finder.push_byte(b))
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
        type ReadError = EOF;

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&mut [u8]) -> (usize, R),
        {
            if self.part >= self.multipart_buffer.len() {
                return Err(EOF);
            }

            if self.position >= self.multipart_buffer[self.part].len() {
                self.part += 1;
                self.position = 0;
                if self.part >= self.multipart_buffer.len() {
                    return Err(EOF);
                }
            }

            let data = &mut self.multipart_buffer[self.part][self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    //TODO: Add tests for DummyMultipartReadStream and DummyReadStream

    #[tokio::test]
    async fn test_default_read_implementation() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_default_read_exact_implementation() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 5];

        stream
            .read_exact(&mut buffer)
            .await
            .expect("Expect no error");
        assert_eq!(&buffer, b"Hello");
    }

    #[tokio::test]
    async fn test_read_till_stop_sequence() {
        const STOP: &[u8] = b"\r\n";
        let mut request_data = b"Hello, World!\r\nThis is a test.\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let bytes_read = stream
            .read_till_stop_sequence(STOP, &mut buffer)
            .await
            .expect("Expect no error");

        assert_eq!(bytes_read, b"Hello, World!".len() + STOP.len());
        assert_eq!(&buffer[..bytes_read], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_read_stop_sequence_only() {
        const STOP: &[u8] = b"\r\n";
        let mut request_data = b"\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let bytes_read = stream
            .read_till_stop_sequence(STOP, &mut buffer)
            .await
            .expect("Expect no error");

        assert_eq!(bytes_read, STOP.len());
        assert_eq!(&buffer[..bytes_read], STOP);
    }

    #[tokio::test]
    async fn test_read_eof_when_no_stop_found() {
        const STOP: &[u8] = b"\r\n";
        let mut request_data = b"Hello, World!".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let error = stream
            .read_till_stop_sequence(STOP, &mut buffer)
            .await
            .expect_err("Expect read error, due to read stream EOF");

        assert!(matches!(error, ReadStreamError::ReadError(EOF)));
    }

    #[tokio::test]
    async fn test_read_buffer_overflow() {
        const STOP: &[u8] = b"\r\n";
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 4];

        let error = stream
            .read_till_stop_sequence(STOP, &mut buffer)
            .await
            .expect_err("Expect buffer overflow error");

        assert!(matches!(error, ReadStreamError::TargetBufferOverflow));
    }

    #[tokio::test]
    async fn test_consume_bytes() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        stream.consume(7).await.expect("Expect no error");

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_stop() {
        const STOP: u8 = b',';
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let consumed = stream
            .consume_till_stop(|b| b == STOP)
            .await
            .expect("Expect no error");
        assert_eq!(consumed, b"Hello,".len());

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b" World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_till_stop_sequence() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let consumed = stream
            .consume_till_stop_sequence(b", Wo")
            .await
            .expect("Expect no error");
        assert_eq!(consumed, b"Hello, Wo".len());

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"rld!\r\n");
    }

    #[tokio::test]
    async fn test_consume_all_data_if_no_sequence_found() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let e = stream
            .consume_till_stop_sequence(b"There is no such sequence")
            .await
            .expect_err("Expect error");

        assert!(matches!(e, ReadStreamError::ReadError(EOF)));

        let e = stream
            .read(&mut buffer)
            .await
            .expect_err("Expect EOF error");
        assert!(matches!(e, EOF));
    }
}
