use embassy_executor::Spawner;
use embassy_net::Stack;
use nanofish::{HttpServer, SocketBuffers};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    const SOCKET_RX_SIZE: usize = 1024;
    const SOCKET_TX_SIZE: usize = 1024;
    const SOCKETS: usize = 4;

    // let statck = Stack::new();

    // let socket_buffers = [SocketBuffers::<SOCKET_RX_SIZE, SOCKET_TX_SIZE>::new(); SOCKETS];
    // let mut server = HttpServer::new(socket_buffers);
}
