pub use crate::mocks::error::MockStreamError;
use crate::read_with::ReadWith;
use crate::write_with::WriteWith;
use embedded_io_async::{ErrorType, Read, Write};
use std::vec::Vec;

pub struct MockSocket {
    on_receive: Option<Box<dyn FnMut(&mut [u8]) -> Result<usize, MockStreamError>>>,
    on_send: Option<Box<dyn FnMut(&[u8]) -> Result<usize, MockStreamError>>>,
}

impl MockSocket {
    pub fn new() -> Self {
        Self {
            on_receive: None,
            on_send: None,
        }
    }

    pub fn set_on_receive<F>(&mut self, callback: F)
    where
        F: 'static + FnMut(&mut [u8]) -> Result<usize, MockStreamError>,
    {
        self.on_receive = Some(Box::new(callback));
    }

    pub fn set_on_send<F>(&mut self, callback: F)
    where
        F: 'static + FnMut(&[u8]) -> Result<usize, MockStreamError>,
    {
        self.on_send = Some(Box::new(callback));
    }
}

impl ErrorType for MockSocket {
    type Error = MockStreamError;
}

impl Read for MockSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockStreamError> {
        if let Some(receive_callback) = &mut self.on_receive {
            let size = receive_callback(buf)?;
            Ok(size)
        } else {
            Err(MockStreamError::ConnectionReset)
        }
    }
}

impl Write for MockSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, MockStreamError> {
        if let Some(send_callback) = &mut self.on_send {
            let size = send_callback(buf)?;
            Ok(size)
        } else {
            Err(MockStreamError::ConnectionReset)
        }
    }
}

impl ReadWith for MockSocket {
    async fn read_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        if let Some(receive_callback) = &mut self.on_receive {
            let mut buf = Vec::<u8>::new();
            buf.resize(1024, 0); // Temporary buffer
            let size = receive_callback(buf.as_mut_slice())?;
            let (written_size, result) = f(&mut buf[..size]);
            assert!(
                written_size <= size,
                "Read more bytes than available in buffer"
            );
            Ok(result)
        } else {
            Err(MockStreamError::ConnectionReset)
        }
    }
}

impl WriteWith for MockSocket {
    #[inline]
    async fn write_with<F, R>(&mut self, f: F) -> Result<R, Self::Error>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        if let Some(send_callback) = &mut self.on_send {
            let mut buf = Vec::<u8>::new();
            buf.resize(1024, 0); // Temporary buffer

            let (written_size, result) = f(buf.as_mut_slice());
            assert!(
                written_size <= buf.len(),
                "Wrote more bytes than available in buffer"
            );
            send_callback(&buf[..written_size])?;

            Ok(result)
        } else {
            Err(MockStreamError::ConnectionReset)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_socket_read_write() {
        let mut mock_socket = MockSocket::new();

        mock_socket.set_on_send(|data| {
            assert_eq!(data, b"Hello, World!");
            Ok(data.len())
        });

        mock_socket.set_on_receive(|buf| {
            let response = b"Response Data";
            let len = response.len().min(buf.len());
            buf[..len].copy_from_slice(&response[..len]);
            Ok(len)
        });

        let write_data = b"Hello, World!";
        let bytes_written = mock_socket.write(write_data).await.unwrap();
        assert_eq!(bytes_written, write_data.len());

        let mut read_buf = [0u8; 20];
        let bytes_read = mock_socket.read(&mut read_buf).await.unwrap();
        assert_eq!(&read_buf[..bytes_read], b"Response Data");
    }
    #[tokio::test]
    async fn test_mock_socket_no_callbacks_set() {
        let mut mock_socket = MockSocket::new();

        let write_data = b"test data";
        let write_result = mock_socket.write(write_data).await;
        assert!(write_result.is_err());

        let mut read_buf = [0u8; 10];
        let read_result = mock_socket.read(&mut read_buf).await;
        assert!(read_result.is_err());
    }

    #[tokio::test]
    async fn test_mock_socket_partial_read() {
        let mut mock_socket = MockSocket::new();

        mock_socket.set_on_receive(|buf| {
            let response = b"This is a very long response";
            let len = response.len().min(buf.len());
            buf[..len].copy_from_slice(&response[..len]);
            Ok(len)
        });

        let mut small_buf = [0u8; 10];
        let bytes_read = mock_socket.read(&mut small_buf).await.unwrap();
        assert_eq!(bytes_read, 10);
        assert_eq!(&small_buf, b"This is a ");
    }

    #[tokio::test]
    async fn test_mock_socket_error_in_callback() {
        let mut mock_socket = MockSocket::new();

        mock_socket.set_on_send(|_| Err(MockStreamError::ConnectionReset));
        mock_socket.set_on_receive(|_| Err(MockStreamError::ConnectionReset));

        let write_data = b"test";
        let write_result = mock_socket.write(write_data).await;
        assert!(write_result.is_err());

        let mut read_buf = [0u8; 10];
        let read_result = mock_socket.read(&mut read_buf).await;
        assert!(read_result.is_err());
    }

    #[tokio::test]
    async fn test_mock_socket_zero_length_operations() {
        let mut mock_socket = MockSocket::new();

        mock_socket.set_on_send(|data| Ok(data.len()));
        mock_socket.set_on_receive(|_| Ok(0));

        let empty_data = b"";
        let bytes_written = mock_socket.write(empty_data).await.unwrap();
        assert_eq!(bytes_written, 0);

        let mut read_buf = [0u8; 10];
        let bytes_read = mock_socket.read(&mut read_buf).await.unwrap();
        assert_eq!(bytes_read, 0);
    }
}
