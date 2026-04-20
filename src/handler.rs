use crate::request::HttpRequest;
use prefix_arena::PrefixArena;

use protocols::error::Error;

pub use embedded_io_async::Write as HttpWriteSocket;

/// The WebSOcket implementation
#[cfg(feature = "ws")]
pub type WebSocket<'a, 's> = protocols::web_socket::WebSocket<'a, embassy_net::tcp::TcpSocket<'s>>;

#[cfg(feature = "ws")]
pub use protocols::web_socket::{WebSocketError, WebSocketState};

#[cfg(feature = "ws")]
pub use protocols::web_socket::{
    WebSocketIoError, WebSocketRead, WebSocketReadReady, WebSocketWrite, WebSocketWriteReady,
};

/// Trait for handling HTTP requests
#[allow(async_fn_in_trait)]
pub trait HttpHandler {
    /// Handle an incoming HTTP request and return a response
    async fn handle_request<HttpSocket: HttpWriteSocket>(
        &mut self,
        allocator: &mut PrefixArena<'_>,
        request: &HttpRequest<'_>,
        http_socket: &mut HttpSocket,
        context_id: usize,
    ) -> Result<(), Error>;

    #[cfg(feature = "ws")]
    /// Handle a WebSocket connection
    ///
    /// If the handler returns result Ok() the WebSocket connection will be automatically closed, but the TCP socket
    /// will remain open and server will process further HTTP requests on it.
    ///
    /// If the handler returns Err() the TCP socket will be closed and the server will wait for a new connection.
    async fn handle_websocket_connection(
        &mut self,
        request: &HttpRequest<'_>,
        web_socket: &mut WebSocket<'_, '_>,
        context_id: usize,
    ) -> Result<(), ()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    //TODO: add tests for HttpHandler implementations
}
