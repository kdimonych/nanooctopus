use crate::find_sequence::FindSequence;
use crate::read_stream::ReadStream;

/// Error returned by TcpSocket read/write functions.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadError<SocketReadErrorT> {
    /// The connection was reset.
    ///
    /// This can happen on receiving a RST packet, or on timeout.
    SocketReadError(SocketReadErrorT),
    TargetBufferOverflow,
}

pub trait ReadStreamExt: ReadStream {
    /// This function reads from the stream to the buffer until the stop predicate returns true.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The byte on which the stop predicate returns true is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop predicate.
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop<StopPredicate>(
        &mut self,
        buffer: &mut [u8],
        mut stop_predicate: StopPredicate,
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>>
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
                    .await
                    .map_err(|e| ReadError::SocketReadError(e))?;

                if stop_triggered {
                    return Ok(write_size);
                }
            }

            Err(ReadError::TargetBufferOverflow)
        }
    }

    /// This function reads from the stream to the buffer until the specified stop sequence is found.
    /// The read bytes are stored in the provided buffer.
    ///
    /// ## Note
    /// The stop sequence is included in the read bytes.
    ///
    /// ## Errors
    /// - Returns `ReadError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop sequence.
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop_sequence(
        &mut self,
        stop_sequence: &[u8],
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>> {
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
    /// - Returns `ReadError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop byte.
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from the stream.
    ///
    fn read_till_stop_byte(
        &mut self,
        stop_byte: u8,
        buffer: &mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>> {
        async move { self.read_till_stop(buffer, |b| b == stop_byte).await }
    }

    /// This function consumes the specified number of bytes from the stream.
    ///
    /// ## Errors
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from
    /// the stream.
    ///   
    fn consume(
        &mut self,
        size: usize,
    ) -> impl core::future::Future<Output = Result<(), ReadError<Self::Error>>> {
        async move {
            let mut bytes_to_consume = size;

            while bytes_to_consume > 0 {
                let bytes_read = self
                    .read_with(|data: &mut [u8]| {
                        let to_read = core::cmp::min(bytes_to_consume, data.len());
                        (to_read, to_read)
                    })
                    .await
                    .map_err(|e| ReadError::SocketReadError(e))?;
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
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop<Stop>(
        &mut self,
        mut stop: Stop,
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>>
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
                .await
                .map_err(|e| ReadError::SocketReadError(e))?
            {}

            return Ok(total_consumed);
        }
    }

    /// This function consumes bytes from the stream until the specified stop byte is found.
    ///
    /// ## Note: The stop byte is also consumed.
    ///
    /// ## Errors
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop_byte(
        &mut self,
        stop_byte: u8,
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>> {
        async move { self.consume_till_stop(|b| b == stop_byte).await }
    }

    /// This function consumes bytes from the stream until the specified stop sequence is found.
    ///     
    /// ## Note: The stop sequence is also consumed.
    ///     
    /// ## Errors
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from
    /// the stream.
    fn consume_till_stop_sequence(
        &mut self,
        stop_sequence: &[u8],
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>> {
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
    use crate::mocks::read_stream::*;

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

        assert!(matches!(error, ReadError::SocketReadError(_)));
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

        assert!(matches!(error, ReadError::TargetBufferOverflow));
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

        assert!(matches!(e, ReadError::SocketReadError(_)));

        let e = stream
            .read(&mut buffer)
            .await
            .expect_err("Expect EOF error");
        assert!(matches!(e, DummySocketError::ConnectionReset));
    }
}
