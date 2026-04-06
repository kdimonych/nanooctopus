use crate::{
    HttpResponseBuilder,
    request::HttpRequest,
    response_builder::{HttpResponse, HttpResponseBufferRef},
};

use protocols::error::Error;
use protocols::status_code::StatusCode;

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
    async fn handle_request(
        &mut self,
        request: &HttpRequest<'_>,
        response_buffer: HttpResponseBufferRef<'_>,
    ) -> Result<HttpResponse, Error>;

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
        web_socket: WebSocket<'_, '_>,
    ) -> Result<(), ()>;
}

/// A simple handler that serves basic endpoints for testing
#[derive(Debug)]
pub struct SimpleHandler;

impl HttpHandler for SimpleHandler {
    async fn handle_request(
        &mut self,
        request: &HttpRequest<'_>,
        mut response_buffer: HttpResponseBufferRef<'_>,
    ) -> Result<HttpResponse, Error> {
        match request.path {
            "/" => HttpResponseBuilder::new(response_buffer.reborrow())
                .with_status(StatusCode::Ok)?
                .with_header("Content-Type", "text/html")?
                .with_body_from_str("<h1>Hello from nanofish HTTP server!</h1>"),
            "/health" => HttpResponseBuilder::new(response_buffer.reborrow())
                .with_status(StatusCode::Ok)?
                .with_header("Content-Type", "application/json")?
                .with_body_from_str("{\"status\":\"ok\"}"),
            _ => HttpResponseBuilder::new(response_buffer.reborrow())
                .with_status(StatusCode::Ok)?
                .with_header("Content-Type", "text/plain")?
                .with_body_from_str("404 Not Found"),
        }
    }

    #[cfg(feature = "ws")]
    async fn handle_websocket_connection(
        &mut self,
        _request: &HttpRequest<'_>,
        _web_socket: WebSocket<'_, '_>,
    ) -> Result<(), ()> {
        Err(()) // Close the connection immediately
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
