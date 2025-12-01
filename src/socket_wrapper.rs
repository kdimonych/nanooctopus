use embassy_net::tcp::TcpSocket;

pub(crate) struct SocketWrapperRef<'a, 'b> {
    pub(crate) socket: &'a mut TcpSocket<'b>,
}

impl SocketWrapperRef<'_, '_> {
    pub(crate) fn bind<'a, 'b>(socket: &'a mut TcpSocket<'b>) -> SocketWrapperRef<'a, 'b> {
        SocketWrapperRef { socket }
    }
}
