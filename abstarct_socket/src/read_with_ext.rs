use crate::borrowed_buffer::BorrowedBuffer;
use crate::find_sequence::FindSequence;
use crate::read_with::ReadWith;

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

pub trait ReadStreamExt: ReadWith {
    /// This function repeatadly call stop_predicate with next chank from the stream until the stop_predicate
    /// returns Some(actually processed bytes out of chank). The read bytes are stored in the provided buffer
    /// (including the last one that triggered the stop predicate).
    ///
    /// ## Returns
    /// - Returns the total number of bytes read out of stream when the stop predicate is triggered.
    ///
    /// ## Errors
    /// - Returns `ReadError::SocketReadError` if an error occurs while reading from the stream.
    /// - Returns `ReadError::TargetBufferOverflow` if the buffer is not large enough to hold the data up to the stop condition.
    ///
    fn read_untill<StopPredicate>(
        &mut self,
        buffer: &mut [u8],
        mut stop_predicate: StopPredicate,
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>>
    where
        StopPredicate: FnMut(&mut [u8]) -> Option<usize>,
    {
        async move {
            let mut read_size = 0;
            let mut result = Ok(());

            let mut buffer = BorrowedBuffer::new(buffer);

            loop {
                let stop_triggered = self
                    .read_with(|mut chank: &mut [u8]| {
                        let mut stoped = false;
                        if let Some(actuall_chank) = stop_predicate(chank) {
                            // SAFETY: actuall_chank is guaranteed to be <= chank.len()
                            chank = unsafe { chank.split_at_mut_unchecked(actuall_chank).0 };
                            stoped = true;
                        }

                        let actually_appended = buffer.append_from_slice(&mut chank);
                        if actually_appended < chank.len() {
                            result = Err(ReadError::TargetBufferOverflow);
                            stoped = true;
                        }

                        read_size += actually_appended;
                        (actually_appended, stoped)
                    })
                    .await
                    .map_err(|e| ReadError::SocketReadError(e))?;

                if stop_triggered {
                    return result.map(|_| read_size);
                }
            }
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
            let mut finder = FindSequence::new(stop_sequence);
            self.read_untill(buffer, |chank| finder.check_next_slice(chank))
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
        async move {
            self.read_untill(buffer, |chank| {
                chank
                    .iter()
                    .position(|&b| b == stop_byte)
                    .map(|pos| pos + 1)
            })
            .await
        }
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
    fn consume_till_stop<StopF>(
        &mut self,
        mut stop: StopF,
    ) -> impl core::future::Future<Output = Result<usize, ReadError<Self::Error>>>
    where
        StopF: FnMut(&[u8]) -> Option<usize>,
    {
        async move {
            let mut total_consumed = 0;

            while self
                .read_with(|data: &mut [u8]| {
                    if let Some(pos) = stop(data) {
                        // The stop point has been found; stop reading further
                        total_consumed += pos;
                        return (pos, false);
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
        async move {
            self.consume_till_stop(|chank| {
                chank
                    .iter()
                    .position(|&b| b == stop_byte)
                    .map(|pos| pos + 1)
            })
            .await
        }
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
            self.consume_till_stop(|chank| stop_sequence_finder.check_next_slice(chank))
                .await
        }
    }
}

impl<T: ReadWith + ?Sized> ReadStreamExt for T {}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mocks::mock_read_stream::*;
    use embedded_io_async::Read;

    #[tokio::test]
    async fn test_read_till_stop_sequence() {
        const STOP: &[u8] = b"\r\n";
        let mut request_data = b"Hello, World!\r\nThis is a test.\r\n".to_vec();
        let mut stream = MockReadStream::new(&mut request_data);
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
        let mut stream = MockReadStream::new(&mut request_data);
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
        let mut stream = MockReadStream::new(&mut request_data);
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
        let mut stream = MockReadStream::new(&mut request_data);
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
        let mut stream = MockReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        stream.consume(7).await.expect("Expect no error");

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b"World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_stop() {
        const STOP: u8 = b',';
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = MockReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let consumed = stream
            .consume_till_stop(|chank| chank.iter().position(|&b| b == STOP).map(|pos| pos + 1))
            .await
            .expect("Expect no error");
        assert_eq!(consumed, b"Hello,".len());

        let read_bytes = stream.read(&mut buffer).await.expect("Expect no error");
        assert_eq!(&buffer[..read_bytes], b" World!\r\n");
    }

    #[tokio::test]
    async fn test_consume_till_stop_sequence() {
        let mut request_data = b"Hello, World!\r\n".to_vec();
        let mut stream = MockReadStream::new(&mut request_data);
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
        let mut stream = MockReadStream::new(&mut request_data);
        let mut buffer = [0u8; 64];

        let e = stream
            .consume_till_stop_sequence(b"There is no such sequence")
            .await
            .expect_err("Expect error");

        assert!(matches!(e, ReadError::SocketReadError(_)));

        let res = stream
            .read(&mut buffer)
            .await
            .expect("Expect Ok(0) due to EOF");
        assert_eq!(res, 0);
    }
}
