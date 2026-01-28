pub use crate::mocks::error::DummySocketError;
pub use crate::read_stream::ReadStream;

extern crate std;

/// A dummy multipart read stream for testing purposes
/// The stream simulates reading from multiple portions (buffers) sequentially like a
/// concatenated stream.
pub struct DummyMultipartReadStream {
    multipart_buffer: std::vec::Vec<std::vec::Vec<u8>>,
    part: usize,
    position: usize,
}

impl DummyMultipartReadStream {
    /// Create a new DummyMultipartReadStream with the given multipart buffer
    pub fn new(multipart_buffer: &std::vec::Vec<std::vec::Vec<u8>>) -> Self {
        Self {
            multipart_buffer: multipart_buffer.clone(),
            part: 0,
            position: 0,
        }
    }
}

impl ReadStream for DummyMultipartReadStream {
    type Error = DummySocketError;
    async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::Error>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        if self.part >= self.multipart_buffer.len() {
            return Err(DummySocketError::ConnectionReset);
        }

        if self.position >= self.multipart_buffer[self.part].len() {
            self.part += 1;
            self.position = 0;
            if self.part >= self.multipart_buffer.len() {
                return Err(DummySocketError::ConnectionReset);
            }
        }

        let data = &mut self.multipart_buffer[self.part][self.position..];
        let (read_bytes, res) = f(data);
        self.position += read_bytes;
        Ok(res)
    }

    async fn read<'s>(&'s mut self, buf: &'s mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() || self.part >= self.multipart_buffer.len() {
            // EOF reached
            return Ok(0);
        }

        let to_read = core::cmp::min(
            buf.len(),
            self.multipart_buffer[self.part].len() - self.position,
        );

        buf[..to_read].copy_from_slice(
            &self.multipart_buffer[self.part][self.position..self.position + to_read],
        );
        self.position += to_read;

        if self.position >= self.multipart_buffer[self.part].len() {
            self.part += 1;
            self.position = 0;
        }

        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_multipart_buffer_is_empty() {
        let multipart_buffer = vec![];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [0u8; 10];

        let result = stream
            .read(&mut buf)
            .await
            .expect("Failed to read from stream");
        assert_eq!(result, 0);
    }

    #[tokio::test]
    async fn test_single_part_read() {
        let multipart_buffer = vec![vec![1, 2, 3, 4, 5]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [0u8; 10];

        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 5);
        assert_eq!(&buf[..5], &[1, 2, 3, 4, 5]);

        // Second read should return 0 (EOF)
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 0);
    }

    #[tokio::test]
    async fn test_multipart_sequential_read() {
        let multipart_buffer = vec![vec![1, 2, 3], vec![4, 5], vec![6, 7, 8, 9]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [0u8; 2];

        // Read first portion partially: [1, 2]
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 2);
        assert_eq!(&buf, &[1, 2]);

        // Continue reading next portion of first part: [3]
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 1);
        assert_eq!(&buf[..bytes_read], &[3]);

        // Read second part: [4, 5]
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 2);
        assert_eq!(&buf, &[4, 5]);

        // Read the first portion of third part: [6, 7]
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 2);
        assert_eq!(&buf, &[6, 7]);

        // Read the second portion of third part: [8, 9]
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 2);
        assert_eq!(&buf, &[8, 9]);

        // EOF
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 0);
    }

    #[tokio::test]
    async fn test_read_with_function() {
        let multipart_buffer = vec![vec![1, 2, 3, 4]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);

        let result = stream
            .read_with(|data| {
                let sum: u32 = data.iter().take(3).map(|&x| x as u32).sum();
                (3, sum)
            })
            .await
            .unwrap();

        assert_eq!(result, 6); // 1 + 2 + 3
    }

    #[tokio::test]
    async fn test_read_with_empty_buffer() {
        let multipart_buffer = vec![];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);

        let result = stream.read_with(|_data| (0, 42)).await;
        assert!(matches!(result, Err(DummySocketError::ConnectionReset)));
    }

    #[tokio::test]
    async fn test_buffer_boundary_transitions() {
        let multipart_buffer = vec![vec![1], vec![2], vec![3]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [0u8; 1];

        // Read each part
        for expected in [1, 2, 3] {
            let bytes_read = stream.read(&mut buf).await.unwrap();
            assert_eq!(bytes_read, 1);
            assert_eq!(buf[0], expected);
        }

        // EOF
        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 0);
    }

    #[tokio::test]
    async fn test_read_larger_than_available() {
        let multipart_buffer = vec![vec![1, 2, 3]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [0u8; 10];

        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 3);
        assert_eq!(&buf[..3], &[1, 2, 3]);
    }

    #[tokio::test]
    async fn test_empty_buffer_read() {
        let multipart_buffer = vec![vec![1, 2, 3]];
        let mut stream = DummyMultipartReadStream::new(&multipart_buffer);
        let mut buf = [];

        let bytes_read = stream.read(&mut buf).await.unwrap();
        assert_eq!(bytes_read, 0);
    }
}
