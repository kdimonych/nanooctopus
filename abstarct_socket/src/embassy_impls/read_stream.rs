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
}
