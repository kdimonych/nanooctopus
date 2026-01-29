use crate::read_with::ReadWith;
use embassy_net::tcp::{TcpReader, TcpSocket};

// Embassy-net based ReadStream implementation for TcpReader
impl<'socket> ReadWith for TcpSocket<'socket> {
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }
}

// Embassy-net based implementation of ReadStream for TcpReader
impl<'socket> ReadWith for TcpReader<'socket> {
    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.read_with(f)
    }
}
