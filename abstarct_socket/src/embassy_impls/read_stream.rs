use crate::read_stream::ReadStream;
use embassy_net::tcp::{TcpReader, TcpSocket};

// Embassy-net based ReadStream implementation for TcpReader
impl<'socket> ReadStream for TcpSocket<'socket> {
    type Error = embassy_net::tcp::Error;

    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }

    fn read<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>> + 's {
        self.read(buf)
    }

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

// Embassy-net based implementation of ReadStream for TcpReader
impl<'socket> ReadStream for TcpReader<'socket> {
    type Error = embassy_net::tcp::Error;
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnMut(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }

    fn read<'s>(
        &'s mut self,
        buf: &'s mut [u8],
    ) -> impl core::future::Future<Output = Result<usize, Self::Error>> + 's {
        self.read(buf)
    }

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
