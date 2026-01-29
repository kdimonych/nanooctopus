use crate::write_with::WriteWith;
use embassy_net::tcp::{TcpSocket, TcpWriter};

// Embassy-net based WriteWith implementation for TcpSocket
impl<'socket> WriteWith for TcpSocket<'socket> {
    fn write_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.write_with(f)
    }
}

// Embassy-net based implementation of WriteWith for TcpWriter
impl<'socket> WriteWith for TcpWriter<'socket> {
    fn write_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        self.write_with(f)
    }
}
