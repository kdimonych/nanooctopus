use core::mem::MaybeUninit;

use crate::{
    HttpResponse, HttpResponseBufferRef, HttpResponseBuilder,
    handler::HttpHandler,
    request::HttpRequest,
    socket_pool::{RoundRobinSocketPoolBuilder, SocketBuffers, SocketPool},
};

use abstarct_socket::head_arena_buffer::HeadArenaBuffer;
use defmt_or_log as log;
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::{Duration, with_timeout};
use embedded_io_async::Write as EmbeddedWrite;
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
    pub async fn serve<H>(&self, worker_memory_buf: &mut [MaybeUninit<u8>], handler: &mut H) -> !
    where
        H: HttpHandler,
    {
        log::info!("WebServer: HTTP server started on port {}", self.socket_pool.port());

        log::debug!("WebServer: HTTP server started listening");
        log::info!("WebServer: Auto-close connection is {}", self.auto_close_connection);

        // SAFETY: We are not dependant of initial state of the buffer,
        // and we will only write to it before reading, so it is safe to treat it as a mutable byte slice.
        let worker_memory_buf = unsafe { core::mem::transmute::<_, &mut [u8]>(worker_memory_buf) };

        loop {
            // Create arena allocator for this connection's request and response processing
            let mut head_arena_alloc = HeadArenaBuffer::from_uninitialized(worker_memory_buf);

            let mut socket = self.socket_pool.acquire_next_request().await;

            log::info!(
                "WebServer: New connection/request {:?}, {:?}",
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
                    log::warn!("WebServer: Read error: {:?}, {:?}", e, socket.remote_endpoint());
                    Self::close_connection(&mut socket).await;
                    continue;
                }
                Err(_) => {
                    log::warn!("WebServer: Socket read timeout, {:?}", socket.remote_endpoint());
                    Self::close_connection(&mut socket).await;
                    continue;
                }
            };

            #[cfg(feature = "ws")]
            if let Some(web_socket_key) = request.web_socket_key {
                log::info!(
                    "WebServer: Process the websocket connection from, {:?}",
                    socket.remote_endpoint()
                );
                if Self::web_socket_handshake(web_socket_key, &mut socket).await.is_err() {
                    // Handshake failed, close the connection
                    Self::close_connection(&mut socket).await;
                    continue;
                }

                if handler
                    .handle_websocket_connection(&request, WebSocket::new(&mut socket))
                    .await
                    .is_err()
                {
                    // Handle error during WebSocket connection
                    log::error!("Error handling WebSocket connection");
                    Self::close_connection(&mut socket).await;
                    continue;
                }
            } else {
                log::info!("WebServer: Process the request of, {:?}", socket.remote_endpoint());

                let response_buf = unsafe { head_arena_alloc.borrow_mut_slice_unchecked() };

                match self
                    .handle_connection(
                        &request,
                        HttpResponseBufferRef::bind(response_buf, self.auto_close_connection),
                        handler,
                    )
                    .await
                {
                    Ok(response) => {
                        if Self::send_response(&mut socket, &response_buf[..response.len()])
                            .await
                            .is_err()
                        {
                            // Failed to send response, close the connection
                            log::debug!("WebServer: Failed to send response, closing connection");
                            Self::close_connection(&mut socket).await;
                            continue;
                        }
                    }
                    Err(e) => {
                        log::error!("WebServer: Error handling request: {:?}", e);
                        // Send a 500 error response
                        if Self::send_server_internal_error(&mut socket).await.is_err() {
                            // Failed to send error response, close the connection
                            log::error!("WebServer: Failed to send internal server error response");
                        }
                        Self::close_connection(&mut socket).await;
                        continue;
                    }
                }
            }

            log::debug!(
                "WebServer: It is about to process following request... {:?}",
                socket.remote_endpoint()
            );
        }
    }

    #[cfg(feature = "ws")]
    async fn web_socket_handshake<'a>(web_socket_key: &'a str, tcp_socket: &mut TcpSocket<'_>) -> Result<(), ()> {
        log::info!("WebServer: WebSocket upgrade request detected");
        // TODO: Reduce buffer size to fit to the handshake response only.
        let mut response_buffer = [0; 1024];
        let res =
            try_handle_websocket_handshake(HttpResponseBufferRef::bind(&mut response_buffer, false), web_socket_key);

        match res {
            Ok(response) => {
                log::info!("WebServer: WebSocket handshake successful");
                // Here you would typically hand off the WebSocket to a WebSocket handler
                // For this example, we'll just close the connection
                Self::send_response(tcp_socket, &response_buffer[..response.len()]).await
            }
            Err(e) => {
                log::error!("WebServer: WebSocket handshake error: {:?}", e);
                // Send a 500 error response
                Self::send_server_internal_error(tcp_socket).await
            }
        }
    }

    async fn send_response<'socket>(socket: &mut TcpSocket<'socket>, response_bytes: &[u8]) -> Result<(), ()> {
        #[cfg(any(feature = "defmt", feature = "log"))]
        if response_bytes.len() < 256 {
            log::trace!(
                "WebServer: Raw response: {:?}",
                core::str::from_utf8(&response_bytes[..response_bytes.len()]).unwrap_or("<invalid utf8>")
            );
        } else {
            log::trace!("WebServer: Response length: {} bytes", response_bytes.len());
        }

        socket.write_all(response_bytes).await.map_err(|e| {
            log::warn!("WebServer: Failed to write response: {:?}", e);
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

        log::info!("WebServer: Connection closed {:?}", remote_endpoint);
    }

    async fn handle_connection<'buf, H>(
        &self,
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
                log::warn!("WebServer: Handler error: {:?}", e);

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

/// Handles the WebSocket handshake process.
#[cfg(feature = "ws")]
fn try_handle_websocket_handshake<'a>(
    mut response_buffer: HttpResponseBufferRef<'a>,
    web_socket_key: &'a str,
) -> Result<HttpResponse, Error> {
    // Compute the Sec-WebSocket-Accept value
    let key_bytes = web_socket_key.as_bytes();
    let mut hasher = Sha1::new();
    hasher.update(key_bytes);
    hasher.update(WS_GUID);
    let hash = hasher.finalize();

    HttpResponseBuilder::new(response_buffer.reborrow())
        .with_status(crate::StatusCode::SwitchingProtocols)?
        .with_header("Upgrade", "websocket")?
        .with_header("Connection", "Upgrade")?
        .with_header_value_from_filler("Sec-WebSocket-Accept", |buf| {
            // Encode the hash in base64 directly into the provided response buffer
            let encoded = binascii::b64encode(&hash, buf)
                .map_err(|_| Error::InvalidData("Failed to encode Sec-WebSocket-Accept"))?;
            Ok(encoded.len())
        })?
        .with_no_body()
}

/// Type alias for `HttpServer` with default buffer sizes (4KB each)
pub type DefaultHttpServer<'a> = HttpServer<'a, 1>;

#[cfg(test)]
mod tests {
    use super::*;
}
