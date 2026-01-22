use crate::{
    HttpResponse, HttpResponseBufferRef, HttpResponseBuilder,
    handler::HttpHandler,
    request::HttpRequest,
    socket_pool::{RoundRobinSocketPoolBuilder, SocketBuffers},
};
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::Write as EmbeddedWrite;
use heapless::spsc::Queue;
use protocols::error::Error;
use protocols::status_code::StatusCode;

#[cfg(feature = "ws")]
use crate::ws::*;

const MAX_REQUEST_SIZE: usize = 4096;
const DEFAULT_MAX_RESPONSE_SIZE: usize = 4096;

/// HTTP server timeout configuration
#[derive(Debug, Clone, Copy)]
pub struct ServerTimeouts {
    /// Socket accept timeout in seconds
    pub accept_timeout: u64,
    /// Socket read timeout in seconds  
    pub read_timeout: u64,
    /// Request handler timeout in seconds
    pub handler_timeout: u64,
}

impl Default for ServerTimeouts {
    fn default() -> Self {
        Self {
            accept_timeout: 10,
            read_timeout: 30,
            handler_timeout: 60,
        }
    }
}

impl ServerTimeouts {
    /// Create new server timeouts with custom values
    #[must_use]
    pub fn new(accept_timeout: u64, read_timeout: u64, handler_timeout: u64) -> Self {
        Self {
            accept_timeout,
            read_timeout,
            handler_timeout,
        }
    }
}
enum HttpProcessResult<'a> {
    Done,
    WebSocketRequest(HttpRequest<'a>, &'a str),
}

/// Simple HTTP server implementation
///
/// **Note**: This server only supports HTTP connections, not HTTPS/TLS.
/// For secure connections, consider using a reverse proxy or load balancer
/// that handles TLS termination.
pub struct HttpServer {
    port: u16,
    timeouts: ServerTimeouts,
    auto_close_connection: bool,
}

/// Resources required for the HTTP server
pub struct HttpServerBuffers<
    const SOCKETS: usize,
    const RX_SIZE: usize,
    const TX_SIZE: usize,
    const REQ_SIZE: usize,
    const MAX_RESPONSE_SIZE: usize,
> {
    socket_buffers: [SocketBuffers<RX_SIZE, TX_SIZE>; SOCKETS],
    request_buf: [u8; REQ_SIZE],
    response_buf: [u8; MAX_RESPONSE_SIZE],
}

impl<
    const SOCKETS: usize,
    const RX_SIZE: usize,
    const TX_SIZE: usize,
    const REQ_SIZE: usize,
    const MAX_RESPONSE_SIZE: usize,
> HttpServerBuffers<SOCKETS, RX_SIZE, TX_SIZE, REQ_SIZE, MAX_RESPONSE_SIZE>
{
    /// Create new HTTP server resources
    pub const fn new() -> Self {
        Self {
            socket_buffers: [const { SocketBuffers::<RX_SIZE, TX_SIZE>::new() }; SOCKETS],
            request_buf: [0; REQ_SIZE],
            response_buf: [0; MAX_RESPONSE_SIZE],
        }
    }
}

impl HttpServer {
    /// Create a new HTTP server with default timeouts
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self {
            port,
            timeouts: ServerTimeouts::default(),
            auto_close_connection: false,
        }
    }

    /// Set custom timeouts
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: ServerTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Set whether to automatically close the connection after each response
    #[must_use]
    pub fn with_auto_close_connection(mut self, auto_close: bool) -> Self {
        // Currently no-op, placeholder for future functionality
        self.auto_close_connection = auto_close;
        self
    }

    /// Start the HTTP server and handle incoming connections
    ///
    /// **Important**: This server only accepts plain HTTP connections.
    /// HTTPS/TLS is not supported by the server (only by the client).
    pub async fn serve<
        'stack,
        const SOCKETS: usize,
        const RX_SIZE: usize,
        const TX_SIZE: usize,
        const REQ_SIZE: usize,
        const MAX_RESPONSE_SIZE: usize,
        H,
    >(
        &mut self,
        stack: Stack<'stack>,
        buffers: &mut HttpServerBuffers<SOCKETS, RX_SIZE, TX_SIZE, REQ_SIZE, MAX_RESPONSE_SIZE>,
        mut handler: H,
    ) -> !
    where
        H: HttpHandler,
    {
        defmt::info!("HTTP server started on port {}", self.port);

        //The tcp socket life cycle
        let socket_pool = RoundRobinSocketPoolBuilder::new(self.port)
            .with_socket_io_timeout(Duration::from_secs(self.timeouts.accept_timeout))
            .with_keep_alive_timeout(Duration::from_secs(5))
            .build(&mut buffers.socket_buffers, stack);

        defmt::debug!("HTTP server started listening");

        if self.auto_close_connection {
            defmt::info!("Auto-close connection is enabled");
        } else {
            defmt::info!("Auto-close connection is disabled");
        }

        let mut ready: Queue<_, SOCKETS> = Queue::new();

        loop {
            socket_pool.acquire_next_request(&mut ready).await;
            if let Some(mut socket) = ready.dequeue() {
                defmt::info!(
                    "New connection/request {:?}, {:?}",
                    socket.remote_endpoint(),
                    self.auto_close_connection
                );

                let n = match with_timeout(
                    Duration::from_secs(self.timeouts.read_timeout),
                    socket.read(&mut buffers.request_buf),
                )
                .await
                {
                    Ok(Ok(0)) => {
                        // Connection closed
                        defmt::info!(
                            "Remote side has closed the connection, {:?}",
                            socket.remote_endpoint()
                        );
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => {
                        defmt::warn!("Read error: {:?}, {:?}", e, socket.remote_endpoint());
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                    Err(_) => {
                        defmt::warn!("Socket read timeout, {:?}", socket.remote_endpoint());
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                };

                defmt::info!("Process the request of, {:?}", socket.remote_endpoint());

                // Parse the request
                let request = match HttpRequest::try_from(&buffers.request_buf[..n]) {
                    Ok(req) => req,
                    Err(e) => {
                        defmt::error!(
                            "Unable to parse HTTP request: {:?}, {:?}",
                            e,
                            socket.remote_endpoint()
                        );
                        // Send a 500 error response
                        Self::send_server_internal_error(&mut socket).await.ok();
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                };

                match self
                    .handle_connection(
                        &request,
                        HttpResponseBufferRef::bind(
                            &mut buffers.response_buf,
                            self.auto_close_connection,
                        ),
                        &mut handler,
                    )
                    .await
                {
                    Ok(response) => {
                        if Self::send_response(&mut socket, &buffers.response_buf[..response.len()])
                            .await
                            .is_err()
                        {
                            // Failed to send response, close the connection
                            defmt::debug!("Failed to send response, closing connection");
                            Self::close_connection(&mut socket).await;
                            continue;
                        }
                    }
                    Err(e) => {
                        defmt::error!("Error handling request: {:?}", e);
                        // Send a 500 error response
                        if Self::send_server_internal_error(&mut socket).await.is_err() {
                            // Failed to send error response, close the connection
                            defmt::error!("Failed to send internal server error response");
                        }
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                }

                defmt::debug!(
                    "It is about to process following request... {:?}",
                    socket.remote_endpoint()
                );
                // // Close the connection after handling
                // Self::close_connection(&mut socket).await;
            } else {
                defmt::warn!("No available sockets in the pool, retrying...");
                Timer::after(Duration::from_millis(10)).await;
            }
        }

        // loop {
        //     // Opened socket lifecycle
        //     if let Some(mut socket) = socket_pool.acquire_next().await {
        //         defmt::info!(
        //             "New connection/request {:?}, {:?}",
        //             socket.remote_endpoint(),
        //             self.auto_close_connection
        //         );

        //         // // The transaction life cycle
        //         // loop {
        //         //     if (!socket.may_recv() || socket.remote_endpoint().is_none())
        //         //         && socket.state() != State::Closed
        //         //     {
        //         //         // The connection is half-closed or not established
        //         //         defmt::info!("The half-closed connection detected, closing socket");
        //         //         break;
        //         //     }

        //         //     // Receive request
        //         //     let mut request_buf = [0; REQ_SIZE];
        //         //     let n = match with_timeout(
        //         //         Duration::from_secs(self.timeouts.read_timeout),
        //         //         socket.read(&mut request_buf),
        //         //     )
        //         //     .await
        //         //     {
        //         //         Ok(Ok(0)) => {
        //         //             // Connection closed
        //         //             defmt::info!(
        //         //                 "Remote side has closed the connection, {:?}",
        //         //                 socket.remote_endpoint()
        //         //             );
        //         //             break;
        //         //         }
        //         //         Ok(Ok(n)) => n,
        //         //         Ok(Err(e)) => {
        //         //             defmt::warn!("Read error: {:?}, {:?}", e, socket.remote_endpoint());
        //         //             break;
        //         //         }
        //         //         Err(_) => {
        //         //             defmt::warn!("Socket read timeout, {:?}", socket.remote_endpoint());
        //         //             break;
        //         //         }
        //         //     };

        //         //     defmt::info!("Process the request of, {:?}", socket.remote_endpoint());

        //         //     // Parse the request
        //         //     let request = match HttpRequest::try_from(&request_buf[..n]) {
        //         //         Ok(req) => req,
        //         //         Err(e) => {
        //         //             defmt::error!(
        //         //                 "Unable to parse HTTP request: {:?}, {:?}",
        //         //                 e,
        //         //                 socket.remote_endpoint()
        //         //             );
        //         //             // Send a 500 error response
        //         //             Self::send_server_internal_error(&mut socket).await.ok();
        //         //             break;
        //         //         }
        //         //     };

        //         //     #[cfg(feature = "ws")]
        //         //     if let Some(web_socket_key) = request.web_socket_key {
        //         //         if Self::web_socket_handshake(web_socket_key, &mut socket)
        //         //             .await
        //         //             .is_err()
        //         //         {
        //         //             // Handshake failed, close the connection
        //         //             break;
        //         //         }
        //         //         // Here we would normally transition to WebSocket handling
        //         //         let mut web_socket_state = WebSocketState::new();

        //         //         // Test code
        //         //         {
        //         //             defmt::info!("WebSocket connection established");
        //         //             let header_buf = &mut [0; 100];
        //         //             let packet_len = socket.read(header_buf).await.ok();
        //         //             if let Some(packet_len) = packet_len {
        //         //                 defmt::info!("WebSocket header received {} bytes", packet_len);
        //         //                 // Trace the raw data in bites
        //         //                 for byte in &header_buf[..packet_len] {
        //         //                     defmt::info!("0b{:08b}", byte);
        //         //                 }
        //         //             }
        //         //         }

        //         //         if handler
        //         //             .handle_websocket_connection(
        //         //                 &request,
        //         //                 WebSocket::new(&mut socket, &mut web_socket_state),
        //         //             )
        //         //             .await
        //         //             .is_err()
        //         //         {
        //         //             //
        //         //             web_socket_state.close(&mut socket).await.ok();
        //         //             break;
        //         //         }

        //         //         // After WebSocket handling is done, close the connection
        //         //         if web_socket_state.close(&mut socket).await.is_err() {
        //         //             break;
        //         //         }
        //         //         if !self.auto_close_connection {
        //         //             // Web Socket if properly closed, proceed to next request
        //         //             continue;
        //         //         } else {
        //         //             // Auto-close connection is enabled, break the transaction cycle
        //         //             break;
        //         //         }
        //         //     }

        //         //     // Handle the regular request
        //         //     let mut response_buffer = [0; MAX_RESPONSE_SIZE];

        //         //     match self
        //         //         .handle_connection(
        //         //             &request,
        //         //             HttpResponseBufferRef::bind(
        //         //                 &mut response_buffer,
        //         //                 self.auto_close_connection,
        //         //             ),
        //         //             &mut handler,
        //         //         )
        //         //         .await
        //         //     {
        //         //         Ok(response) => {
        //         //             if Self::send_response(&mut socket, &response_buffer[..response.len()])
        //         //                 .await
        //         //                 .is_err()
        //         //             {
        //         //                 // Failed to send response, close the connection
        //         //                 defmt::debug!("Failed to send response, closing connection");
        //         //                 break;
        //         //             }
        //         //         }
        //         //         Err(e) => {
        //         //             defmt::error!("Error handling request: {:?}", e);
        //         //             // Send a 500 error response
        //         //             if Self::send_server_internal_error(&mut socket).await.is_err() {
        //         //                 // Failed to send error response, close the connection
        //         //                 defmt::error!("Failed to send internal server error response");
        //         //             }
        //         //             break;
        //         //         }
        //         //     }

        //         //     defmt::debug!(
        //         //         "It is about to process following request... {:?}",
        //         //         socket.remote_endpoint()
        //         //     );
        //         //     if self.auto_close_connection {
        //         //         // Close the connection after each response
        //         //         defmt::debug!("Auto-closing connection as per configuration");
        //         //         break;
        //         //     }
        //         // }

        //         // Close the connection after handling
        //         Self::close_connection(&mut socket).await;
        //     } else {
        //         defmt::warn!("No available sockets in the pool, retrying...");
        //         Timer::after(Duration::from_millis(10)).await;
        //     };
        // }
    }

    #[cfg(feature = "ws")]
    async fn web_socket_handshake<'a>(
        web_socket_key: &'a str,
        tcp_socket: &mut TcpSocket<'_>,
    ) -> Result<(), ()> {
        defmt::info!("WebSocket upgrade request detected");
        // TODO: Reduce buffer size to fit to the handshake response only.
        let mut response_buffer = [0; 1024];
        let res = try_handle_websocket_handshake(
            HttpResponseBufferRef::bind(&mut response_buffer, false),
            web_socket_key,
        );

        match res {
            Ok(response) => {
                defmt::info!("WebSocket handshake successful");
                // Here you would typically hand off the WebSocket to a WebSocket handler
                // For this example, we'll just close the connection
                Self::send_response(tcp_socket, &response_buffer[..response.len()]).await
            }
            Err(e) => {
                defmt::error!("WebSocket handshake error: {:?}", e);
                // Send a 500 error response
                Self::send_server_internal_error(tcp_socket).await
            }
        }
    }

    async fn send_response<'socket>(
        socket: &mut TcpSocket<'socket>,
        response_bytes: &[u8],
    ) -> Result<(), ()> {
        if response_bytes.len() < 256 {
            defmt::debug!(
                "Raw response: {:?}",
                core::str::from_utf8(&response_bytes[..response_bytes.len()])
                    .unwrap_or("<invalid utf8>")
            );
        } else {
            defmt::debug!("Response length: {} bytes", response_bytes.len());
        }

        socket.write_all(response_bytes).await.map_err(|e| {
            defmt::warn!("Failed to write response: {:?}", e);
        })
    }

    async fn send_server_internal_error<'a>(socket: &mut TcpSocket<'a>) -> Result<(), ()> {
        let error_response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 21\r\n\r\nInternal Server Error";
        Self::send_response(socket, error_response).await
    }

    /// Close the connection gracefully
    async fn close_connection<'a>(socket: &mut TcpSocket<'a>) {
        let remote_endpoint = socket.remote_endpoint();

        // Close the write side of the connection
        socket.close();
        // Ensure all pending data is sent
        socket.flush().await.ok();
        // Close the socket
        socket.abort();
        // Ensure the RST is sent
        socket.flush().await.ok();

        defmt::info!("Connection closed {:?}", remote_endpoint);
    }

    async fn abort_connection<'a>(socket: &mut TcpSocket<'a>) {
        let remote_endpoint = socket.remote_endpoint();

        // Close the socket
        socket.abort();
        // Ensure the RST is sent
        socket.flush().await.ok();

        defmt::info!("Connection aborted {:?}", remote_endpoint);
    }

    async fn handle_connection<'buf, H>(
        &mut self,
        request: &HttpRequest<'_>,
        mut response_buffer: HttpResponseBufferRef<'buf>,
        handler: &mut H,
    ) -> Result<HttpResponse, Error>
    where
        H: HttpHandler,
    {
        // Handle the request
        match with_timeout(
            Duration::from_secs(self.timeouts.handler_timeout),
            handler.handle_request(&request, response_buffer.reborrow()),
        )
        .await
        {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(e)) => {
                defmt::warn!("Handler error: {:?}", e);
                HttpResponseBuilder::new(response_buffer.reborrow())
                    .with_status(StatusCode::InternalServerError)?
                    .with_header("Content-Type", "text/plain")?
                    .with_body_from_str("Internal Server Error")
            }
            Err(_) => HttpResponseBuilder::new(response_buffer.reborrow())
                .with_status(StatusCode::InternalServerError)?
                .with_header("Content-Type", "text/plain")?
                .with_body_from_str("Request Timeout"),
        }
    }
}

/// Type alias for `HttpServer` with default buffer sizes (4KB each)
pub type DefaultHttpServer = HttpServer;

/// Type alias for `HttpServer` with small buffer sizes for memory-constrained environments (1KB each)
pub type SmallHttpServer = HttpServer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_server_creation() {
        let server: DefaultHttpServer = HttpServer::new(8080);
        assert_eq!(server.port, 8080);
        assert_eq!(server.timeouts.accept_timeout, 10);
        assert_eq!(server.timeouts.read_timeout, 30);
        assert_eq!(server.timeouts.handler_timeout, 60);

        let server: SmallHttpServer = HttpServer::new(3000);
        assert_eq!(server.port, 3000);
    }

    #[test]
    fn test_server_timeouts() {
        // Test default timeouts
        let timeouts = ServerTimeouts::default();
        assert_eq!(timeouts.accept_timeout, 10);
        assert_eq!(timeouts.read_timeout, 30);
        assert_eq!(timeouts.handler_timeout, 60);

        // Test custom timeouts
        let custom_timeouts = ServerTimeouts::new(5, 15, 45);
        assert_eq!(custom_timeouts.accept_timeout, 5);
        assert_eq!(custom_timeouts.read_timeout, 15);
        assert_eq!(custom_timeouts.handler_timeout, 45);

        // Test server with custom timeouts
        let server = HttpServer::new(8080).with_timeouts(custom_timeouts);
        assert_eq!(server.port, 8080);
        assert_eq!(server.timeouts.accept_timeout, 5);
        assert_eq!(server.timeouts.read_timeout, 15);
        assert_eq!(server.timeouts.handler_timeout, 45);
    }
}
