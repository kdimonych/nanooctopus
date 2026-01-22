/// Marker trait for read stream errors. Marks types that can be converted into HTTP errors.
/// Convertion are done via aggregation in the ReadStreamError::ReadError() error.
pub trait IntoReadError {}

/// Errors that can occur while reading from the stream
#[derive(Debug)]
pub enum ReadStreamError<ReadError: IntoReadError> {
    /// Target buffer overflowed while reading
    TargetBufferOverflow,
    /// Error occurred while reading from the stream
    ReadError(ReadError),
}

impl<ReadError: IntoReadError> From<ReadError> for ReadStreamError<ReadError> {
    fn from(err: ReadError) -> Self {
        ReadStreamError::ReadError(err)
    }
}

/// Trait representing a read stream interface
pub trait ReadStream {
    type ReadError: IntoReadError;

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

    impl IntoReadError for embassy_net::tcp::Error {}

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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mocks::dummy_read_stream::*;

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
}
