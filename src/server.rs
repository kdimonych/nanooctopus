use core::mem::MaybeUninit;

#[cfg(feature = "ws")]
use crate::{
    HttpResponseBuilder,
    handler::HttpHandler,
    request::HttpRequest,
    socket_pool::{RoundRobinSocketPoolBuilder, SocketBuffers, SocketPool},
};

use defmt_or_log as log;
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::{Duration, with_timeout};
use embedded_io_async::Write as EmbeddedWrite;
use prefix_arena::PrefixArena;
use protocols::error::Error;
use protocols::status_code::StatusCode;

#[cfg(feature = "ws")]
use protocols::web_socket::WebSocket;
#[cfg(feature = "ws")]
use sha1::{Digest, Sha1};

#[cfg(feature = "ws")]
const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// HTTP server timeout configuration
#[derive(Debug, Clone, Copy)]
pub struct ServerTimeouts {
    /// Socket accept timeout in seconds
    pub accept_timeout: u64,
    /// Socket read timeout in seconds  
    pub read_timeout: u64,
    /// Keep-alive timeout in seconds (currently not used, placeholder for future functionality)
    pub keep_alive_timeout: u64,
    /// Request handler timeout in seconds
    pub handler_timeout: u64,
}

impl Default for ServerTimeouts {
    fn default() -> Self {
        Self {
            accept_timeout: 10,
            read_timeout: 30,
            keep_alive_timeout: 5,
            handler_timeout: 60,
        }
    }
}

impl ServerTimeouts {
    /// Create new server timeouts with custom values
    #[must_use]
    pub fn new(accept_timeout: u64, read_timeout: u64, keep_alive_timeout: u64, handler_timeout: u64) -> Self {
        Self {
            accept_timeout,
            read_timeout,
            keep_alive_timeout,
            handler_timeout,
        }
    }
}

/// Simple HTTP server implementation
///
/// **Note**: This server only supports HTTP connections, not HTTPS/TLS.
/// For secure connections, consider using a reverse proxy or load balancer
/// that handles TLS termination.
pub struct HttpServer<'stack, const SOCKETS: usize> {
    socket_pool: SocketPool<'stack, SOCKETS>,
    timeouts: ServerTimeouts,
    auto_close_connection: bool,
}

impl<'stack, const SOCKETS: usize> HttpServer<'stack, SOCKETS> {
    /// Create a new HTTP server with default timeouts
    #[must_use]
    pub fn new<'buffer, const SOCKET_RX_SIZE: usize, const SOCKET_TX_SIZE: usize>(
        socket_buffers: &'buffer mut [SocketBuffers<SOCKET_RX_SIZE, SOCKET_TX_SIZE>; SOCKETS],
        stack: Stack<'stack>,
        port: u16,
        timeouts: ServerTimeouts,
    ) -> Self
    where
        'buffer: 'stack,
    {
        //The tcp socket life cycle
        let socket_pool = RoundRobinSocketPoolBuilder::new(port)
            .with_socket_io_timeout(Duration::from_secs(timeouts.accept_timeout))
            .with_keep_alive_timeout(Duration::from_secs(timeouts.keep_alive_timeout))
            .build(socket_buffers, stack);

        Self {
            socket_pool,
            timeouts,
            auto_close_connection: false,
        }
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
    pub async fn serve<H>(&self, worker_memory_buf: &mut [MaybeUninit<u8>], handler: &mut H, context_id: usize) -> !
    where
        H: HttpHandler,
    {
        log::info!(
            "WebServer[{}]: HTTP server started on port {}",
            context_id,
            self.socket_pool.port()
        );

        log::debug!("WebServer[{}]: HTTP server started listening", context_id);
        log::info!(
            "WebServer[{}]: Auto-close connection is {}",
            context_id,
            self.auto_close_connection
        );

        loop {
            // Create arena allocator for this connection's request and response processing
            let mut head_arena_alloc = PrefixArena::from_uninit(worker_memory_buf);

            let mut socket = self.socket_pool.acquire_next_request().await;

            log::info!(
                "WebServer[{}]: New connection/request {:?}, {:?}",
                context_id,
                socket.remote_endpoint(),
                self.auto_close_connection
            );

            let request = match with_timeout(
                Duration::from_secs(self.timeouts.read_timeout),
                HttpRequest::try_parse_from_stream(&mut socket.split().0, &mut head_arena_alloc),
            )
            .await
            {
                Ok(Ok(request)) => request,
                Ok(Err(e)) => {
                    log::warn!(
                        "WebServer[{}]: Read error: {:?}, {:?}",
                        context_id,
                        e,
                        socket.remote_endpoint()
                    );
                    self.close_connection(&mut socket, context_id).await;
                    continue;
                }
                Err(_) => {
                    log::warn!(
                        "WebServer[{}]: Socket read timeout, {:?}",
                        context_id,
                        socket.remote_endpoint()
                    );
                    self.close_connection(&mut socket, context_id).await;
                    continue;
                }
            };

            #[cfg(feature = "ws")]
            if let Some(web_socket_key) = request.web_socket_key {
                log::info!(
                    "WebServer[{}]: Process the websocket connection from, {:?}",
                    context_id,
                    socket.remote_endpoint()
                );
                if self
                    .web_socket_handshake(&mut head_arena_alloc, web_socket_key, &mut socket, context_id)
                    .await
                    .is_err()
                {
                    // Handshake failed, close the connection
                    self.close_connection(&mut socket, context_id).await;
                    continue;
                }

                let socket_ref: &mut TcpSocket<'_> = &mut socket;
                let mut web_socket = WebSocket::new(socket_ref);
                if let Err(e) = handler
                    .handle_websocket_connection(&request, &mut web_socket, context_id)
                    .await
                {
                    // Handle error during WebSocket connection
                    log::error!(
                        "WebServer[{}]: Error handling WebSocket connection: {:?}",
                        context_id,
                        e
                    );
                }

                // Ensure the WebSocket connection is closed gracefully
                if let Err(e) = web_socket.close().await {
                    log::error!("WebServer[{}]: Error closing WebSocket connection: {}", context_id, e);
                }
                // After handling the WebSocket connection, we will close the TCP connection and wait for a new one
                self.close_connection(&mut socket, context_id).await;
                continue;
            } else {
                log::info!(
                    "WebServer[{}]: Process the request of, {:?}",
                    context_id,
                    socket.remote_endpoint()
                );

                match self
                    .handle_connection(&mut head_arena_alloc, &request, &mut socket, handler, context_id)
                    .await
                {
                    Ok(()) => {
                        log::info!(
                            "WebServer[{}]: Request handled successfully, {:?}",
                            context_id,
                            socket.remote_endpoint()
                        );
                    }
                    Err(e) => {
                        log::error!("WebServer[{}]: Error handling request: {:?}", context_id, e);
                        // Send a 500 error response
                        if self.send_server_internal_error(&mut socket, context_id).await.is_err() {
                            // Failed to send error response, close the connection
                            log::error!(
                                "WebServer[{}]: Failed to send internal server error response",
                                context_id
                            );
                        }
                        self.close_connection(&mut socket, context_id).await;
                        continue;
                    }
                }
            }

            log::debug!(
                "WebServer[{}]: It is about to process following request... {:?}",
                context_id,
                socket.remote_endpoint()
            );
        }
    }

    #[cfg(feature = "ws")]
    async fn web_socket_handshake<'a>(
        &self,
        allocator: &mut PrefixArena<'_>,
        web_socket_key: &'a str,
        tcp_socket: &mut TcpSocket<'_>,
        context_id: usize,
    ) -> Result<(), ()> {
        log::info!("WebServer[{}]: WebSocket upgrade request detected", context_id);
        let res = try_handle_websocket_handshake(allocator, tcp_socket, web_socket_key).await;

        match res {
            Ok(()) => {
                log::info!("WebServer[{}]: WebSocket handshake successful", context_id);
                Ok(())
            }
            Err(e) => {
                log::error!("WebServer[{}]: WebSocket handshake error: {:?}", context_id, e);
                // Send a 500 error response
                self.send_server_internal_error(tcp_socket, context_id).await
            }
        }
    }

    async fn send_response<'socket>(
        &self,
        socket: &mut TcpSocket<'socket>,
        response_bytes: &[u8],
        context_id: usize,
    ) -> Result<(), ()> {
        #[cfg(any(feature = "defmt", feature = "log"))]
        if response_bytes.len() < 256 {
            log::trace!(
                "WebServer[{}]: Raw response: {:?}",
                context_id,
                core::str::from_utf8(&response_bytes[..response_bytes.len()]).unwrap_or("<invalid utf8>")
            );
        } else {
            log::trace!(
                "WebServer[{}]: Response length: {} bytes",
                context_id,
                response_bytes.len()
            );
        }

        socket.write_all(response_bytes).await.map_err(|e| {
            log::warn!("WebServer[{}]: Failed to write response: {:?}", context_id, e);
        })
    }

    async fn send_server_internal_error<'a>(&self, socket: &mut TcpSocket<'a>, context_id: usize) -> Result<(), ()> {
        let error_response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 21\r\n\r\nInternal Server Error";
        self.send_response(socket, error_response, context_id).await
    }

    /// Close the connection gracefully
    async fn close_connection<'a>(&self, socket: &mut TcpSocket<'a>, context_id: usize) {
        let remote_endpoint = socket.remote_endpoint();

        // Close the write side of the connection
        socket.close();
        // Ensure all pending data is sent
        socket.flush().await.ok();
        // Close the socket
        socket.abort();
        // Ensure the RST is sent
        socket.flush().await.ok();

        log::info!("WebServer[{}]: Connection closed {:?}", context_id, remote_endpoint);
    }

    async fn handle_connection<H>(
        &self,
        allocator: &mut PrefixArena<'_>,
        request: &HttpRequest<'_>,
        http_socket: &mut TcpSocket<'_>,
        handler: &mut H,
        context_id: usize,
    ) -> Result<(), Error>
    where
        H: HttpHandler,
    {
        // Handle the request
        match with_timeout(
            Duration::from_secs(self.timeouts.handler_timeout),
            handler.handle_request(allocator, &request, http_socket, context_id),
        )
        .await
        {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(e)) => {
                log::warn!("WebServer[{}]: Handler error: {:?}", context_id, e);

                HttpResponseBuilder::new(http_socket)
                    .with_status(StatusCode::InternalServerError)
                    .await?
                    .with_header("Content-Type", "text/plain")
                    .await?
                    .with_body_from_str("Internal Server Error")
                    .await
            }
            Err(_) => {
                HttpResponseBuilder::new(http_socket)
                    .with_status(StatusCode::InternalServerError)
                    .await?
                    .with_header("Content-Type", "text/plain")
                    .await?
                    .with_body_from_str("Request Timeout")
                    .await
            }
        }
    }
}

/// Handles the WebSocket handshake process.
#[cfg(feature = "ws")]
async fn try_handle_websocket_handshake<'a>(
    allocator: &mut PrefixArena<'_>,
    http_socket: &mut TcpSocket<'_>,
    web_socket_key: &'a str,
) -> Result<(), Error> {
    // Compute the Sec-WebSocket-Accept value
    let key_bytes = web_socket_key.as_bytes();
    let mut hasher = Sha1::new();
    hasher.update(key_bytes);
    hasher.update(WS_GUID);
    let hash = hasher.finalize();

    let mut tmp_buf = allocator.view();
    let buf = unsafe { tmp_buf.as_slice_mut_unchecked() };
    let encoded_hash =
        binascii::b64encode(&hash, buf).map_err(|_| Error::InvalidData("Failed to encode Sec-WebSocket-Accept"))?;

    let builder = HttpResponseBuilder::new(http_socket);
    builder
        .with_status(crate::StatusCode::SwitchingProtocols)
        .await?
        .with_header("Upgrade", "websocket")
        .await?
        .with_header("Connection", "Upgrade")
        .await?
        .with_header_from_slice("Sec-WebSocket-Accept", encoded_hash)
        .await?
        .with_no_body()
        .await
}

/// Type alias for `HttpServer` with default buffer sizes (4KB each)
pub type DefaultHttpServer<'a> = HttpServer<'a, 1>;

#[cfg(test)]
mod tests {
    //TODO: add tests for HttpServer, including:
    // - Test that the server can accept and handle a simple HTTP request correctly
    // - Test that the server can handle multiple requests sequentially
    // - Test that the server can handle WebSocket upgrade requests correctly (if ws feature is enabled)
    // - Test that the server properly handles timeouts and errors, returning appropriate HTTP responses
    // - Test that the server can handle large requests and responses without crashing or leaking memory
}
