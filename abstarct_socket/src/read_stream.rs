/// Trait representing a read stream interface
pub trait ReadStream {
    type Error;
    /// Read from the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes read and a result value.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream
    /// or stream reaches EOF.
    ///
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnMut(&mut [u8]) -> (usize, R);

    /// Read data from the stream into the provided buffer
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream
    /// or stream reaches EOF.
    ///
    fn read<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>> + 's {
        self.read_with(|data: &mut [u8]| {
            let to_read = core::cmp::min(buf.len(), data.len());
            buf[..to_read].copy_from_slice(&data[..to_read]);
            (to_read, to_read)
        })
    }

    /// Read exactly `buf.len()` bytes from the stream into the provided buffer
    ///     
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream
    /// or stream reaches EOF before filling the buffer.
    ///
    fn read_exact<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + 's {
        async move {
            let mut total_read = 0;

            while total_read < buf.len() {
                let bytes_read = self.read(&mut buf[total_read..]).await?;
                total_read += bytes_read;
            }

            Ok(())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mocks::error::DummySocketError;
    use crate::mocks::read_stream::DummyReadStream;

    #[tokio::test]
    async fn test_default_read_implementation() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"Hello, World!\r\n");
    }

    #[tokio::test]
    async fn test_default_read_returns_error_if_stream_eof() {
        let mut request_data = b"".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 5];

        let e = stream
            .read(&mut buffer)
            .await
            .expect_err("Expect an error due to EOF");
        assert!(matches!(e, DummySocketError::ConnectionReset));
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
    async fn test_default_read_exact_returns_error_if_stream_eof() {
        let mut request_data = b"".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 5];

        let e = stream
            .read_exact(&mut buffer)
            .await
            .expect_err("Expect an error due to EOF");
        assert!(matches!(e, DummySocketError::ConnectionReset));
    }
}
