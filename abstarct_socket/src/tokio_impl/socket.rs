//! Tokio-specific socket exports.
pub use crate::tokio_impl::tokio_socket_wrapper::{
    TokioSocketOwnedReadHalfWrapper, TokioSocketOwnedWriteHalfWrapper, TokioSocketReadHalfWrapper, TokioSocketWrapper,
    TokioSocketWriteHalfWrapper,
};

use crate::socket::{AbstractSocketBuilder, SocketEndpoint};
use defmt_or_log as log;
use tokio::net::TcpListener;

/// Tokio implementation of a socket builder that borrows the TcpSocket for its lifetime.
pub struct TokioTcpSocketBuilder<'stack> {
    listener: TcpListener,
    _marker: core::marker::PhantomData<&'stack ()>,
}

impl<'stack> TokioTcpSocketBuilder<'stack> {
    /// Create a new TokioTcpSocketBuilder with the specified endpoint.
    /// The endpoint can be a socket address or a string that can be parsed into one.
    /// The builder will bind to the endpoint and be ready to accept connections.
    /// ### Arguments
    /// * `endpoint` - The socket endpoint to bind to, which can be a SocketEndpoint or a type that can be converted into one.
    /// ### Results
    /// Returns a new instance of TokioTcpSocketBuilder that is bound to the specified endpoint and ready to accept connections.
    /// ### Panics
    /// This function will panic if binding to the endpoint fails, which can happen if the address is invalid or already in use.
    pub async fn new(endpoint: impl Into<SocketEndpoint>) -> Self {
        let addr = endpoint.into();
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e: std::io::Error| log::error!("Failed to bind to endpoint: {:?}", e))
            .unwrap();

        Self {
            listener,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<'stack> AbstractSocketBuilder for TokioTcpSocketBuilder<'stack> {
    type Socket<'a>
        = TokioSocketWrapper
    where
        Self: 'a;

    async fn accept(&self) -> Self::Socket<'_> {
        self.listener
            .accept()
            .await
            .inspect_err(|e| log::error!("Failed to accept connection: {:?}", e))
            .map(|(soc, _)| TokioSocketWrapper::new_stream(soc))
            .unwrap()
    }

    fn endpoint(&self) -> SocketEndpoint {
        // Convert the listener's local address to a SocketEndpoint
        self.listener
            .local_addr()
            .map(|addr| SocketEndpoint::from(addr))
            .inspect_err(|e| log::error!("Failed to get local address: {:?}", e))
            .unwrap()
    }
}

extern crate alloc;

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use embedded_io_async::Read;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpSocket, TcpStream};

    use super::{TokioSocketReadHalfWrapper, TokioSocketWrapper};
    use crate::socket::{SocketAccept, SocketClose, SocketConfig, SocketConnect, SocketInfo, SocketReadWith, State};

    #[tokio::test]
    async fn test_stream_read_with_preserves_unconsumed_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let (mut peer, _) = listener.accept().await.unwrap();
            peer.write_all(b"abcdef").await.unwrap();
        });

        let stream = TcpStream::connect(endpoint).await.unwrap();
        let mut socket = TokioSocketWrapper::new_stream(stream);

        let prefix = socket
            .read_with(|buf| {
                assert_eq!(&buf[..6], b"abcdef");
                (2, [buf[0], buf[1]])
            })
            .await
            .unwrap();
        assert_eq!(prefix, *b"ab");

        let mut tail = [0u8; 8];
        let read = socket.read(&mut tail).await.unwrap();
        assert_eq!(&tail[..read], b"cdef");

        writer.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_half_read_with_preserves_unconsumed_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();

        let writer = tokio::spawn(async move {
            let (mut peer, _) = listener.accept().await.unwrap();
            peer.write_all(b"abcdef").await.unwrap();
        });

        let mut stream = TcpStream::connect(endpoint).await.unwrap();
        let (read_half, _) = stream.split();
        let mut socket = TokioSocketReadHalfWrapper::new(read_half);

        let prefix = socket
            .read_with(|buf| {
                assert_eq!(&buf[..6], b"abcdef");
                (2, [buf[0], buf[1]])
            })
            .await
            .unwrap();
        assert_eq!(prefix, *b"ab");

        let mut tail = [0u8; 8];
        let read = socket.read(&mut tail).await.unwrap();
        assert_eq!(&tail[..read], b"cdef");

        writer.await.unwrap();
    }

    #[tokio::test]
    async fn test_connect_promotes_socket_to_stream_and_sets_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_endpoint = listener.local_addr().unwrap();

        let accept_task = tokio::spawn(async move {
            let (stream, remote_addr) = listener.accept().await.unwrap();
            (stream.local_addr().unwrap(), remote_addr)
        });

        let mut socket = TokioSocketWrapper::new_socket(TcpSocket::new_v4().unwrap());
        assert_eq!(socket.remote_endpoint(), None);

        socket.connect(server_endpoint).await.unwrap();

        let client_local = socket.local_endpoint().unwrap();
        let client_remote = socket.remote_endpoint().unwrap();
        assert_eq!(client_remote, server_endpoint);

        let (server_local, server_remote) = accept_task.await.unwrap();
        assert_eq!(client_local, server_remote);
        assert_eq!(server_local, server_endpoint);
    }

    #[tokio::test]
    async fn test_close_makes_peer_observe_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_endpoint = listener.local_addr().unwrap();

        let peer_task = tokio::spawn(async move {
            let (mut peer_stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8];
            peer_stream.read(&mut buf).await.unwrap()
        });

        let stream = TcpStream::connect(server_endpoint).await.unwrap();
        let mut socket = TokioSocketWrapper::new_stream(stream);
        socket.close().await.unwrap();

        assert_eq!(peer_task.await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_listener_exposes_local_endpoint_and_accepts_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_endpoint = listener.local_addr().unwrap();
        let mut socket = TokioSocketWrapper::new_listener(listener);

        assert_eq!(socket.local_endpoint(), Some(listen_endpoint));
        assert_eq!(socket.remote_endpoint(), None);
        assert_eq!(socket.state(), State::Closed);

        let client = tokio::spawn(async move { TcpStream::connect(listen_endpoint).await.unwrap() });

        socket.accept(listen_endpoint).await.unwrap();

        let client = client.await.unwrap();
        assert_eq!(socket.state(), State::Established);
        assert_eq!(socket.local_endpoint(), Some(listen_endpoint));
        assert_eq!(socket.remote_endpoint(), Some(client.local_addr().unwrap()));
    }

    #[tokio::test]
    async fn test_unconnected_socket_exposes_bound_local_endpoint() {
        let tcp_socket = TcpSocket::new_v4().unwrap();
        let bound_endpoint = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        tcp_socket.bind(bound_endpoint).unwrap();
        let expected = tcp_socket.local_addr().unwrap();

        let socket = TokioSocketWrapper::new_socket(tcp_socket);

        assert_eq!(socket.local_endpoint(), Some(expected));
        assert_eq!(socket.remote_endpoint(), None);
        assert_eq!(socket.state(), State::Closed);
    }

    #[tokio::test]
    async fn test_socket_config_methods_are_safe_on_all_wrapper_variants() {
        let mut socket = TokioSocketWrapper::new_socket(TcpSocket::new_v4().unwrap());
        socket.set_keep_alive(Some(Duration::from_secs(1)));
        socket.set_timeout(Some(Duration::from_secs(1)));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut listener_wrapper = TokioSocketWrapper::new_listener(listener);
        listener_wrapper.set_keep_alive(Some(Duration::from_secs(1)));
        listener_wrapper.set_timeout(Some(Duration::from_secs(1)));

        let stream = TcpStream::connect(endpoint).await.unwrap();
        let mut stream_wrapper = TokioSocketWrapper::new_stream(stream);
        stream_wrapper.set_keep_alive(Some(Duration::from_secs(1)));
        stream_wrapper.set_timeout(Some(Duration::from_secs(1)));
    }

    mod tokio_builder {
        use crate::socket::AbstractSocketBuilder;
        use crate::tokio_impl::socket::{IpVersion, TokioTcpSocketBuilder};

        #[test]
        fn builder_creates_socket() {
            let mut builder = TokioTcpSocketBuilder::new(IpVersion::V4);
            let _wrapper = builder.build();
        }
    }
}
