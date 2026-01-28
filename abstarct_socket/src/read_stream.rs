/// Trait representing a read stream interface
pub trait ReadStream {
    type Error;
    /// Read from the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes read and a result value.
    ///
    /// ## Returns
    /// - Returns Ok(R) where R is the result returned by the function `f`.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream.
    ///
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnMut(&mut [u8]) -> (usize, R);

    /// Read data from the stream into the provided buffer
    ///
    /// ## Returns
    /// - Returns Ok(n) where n is the number of bytes read into buf.
    /// - Returns Ok(0) immediately if the stream has reached EOF or buf is empty.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream.
    ///
    fn read<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>> + 's;

    /// Read exactly `buf.len()` bytes from the stream into the provided buffer
    ///
    /// ## Returns
    /// - Returns Ok(n) where n < buf.len() if the stream has reached EOF before filling the buffer.
    /// - Returns Ok(0) immediately if buf is empty.
    ///     
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading.
    ///
    fn read_exact<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>> + 's {
        async move {
            let mut total_read = 0;

            while total_read < buf.len() {
                let bytes_read = self.read(&mut buf[total_read..]).await?;
                if bytes_read == 0 {
                    // EOF reached before filling the buffer
                    break;
                }
                total_read += bytes_read;
            }

            Ok(total_read)
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
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
    async fn test_default_read_returns_ok_with_zero_size_if_has_stream_reached_eof_and_has_no_data()
    {
        let mut request_data = b"".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 5];

        let res = stream
            .read(&mut buffer)
            .await
            .expect("Expect Ok(0) due to EOF");
        assert_eq!(res, 0);
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
    async fn test_default_read_exact_returns_actually_read_size_if_stream_eof() {
        let mut request_data = b"1234".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 5];

        let res = stream
            .read_exact(&mut buffer)
            .await
            .expect("Expect Ok(4) due to EOF");
        assert_eq!(res, request_data.len());
    }

    #[tokio::test]
    async fn test_default_read_exact_returns_zero_size_if_buf_is_empty() {
        let mut request_data = b"1234".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let mut buffer = [0u8; 0];

        let res = stream
            .read_exact(&mut buffer)
            .await
            .expect("Expect Ok(0) due to empty buffer");
        assert_eq!(res, 0);
    }
}
