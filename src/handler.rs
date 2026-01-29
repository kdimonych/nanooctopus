#[cfg(feature = "ws")]
use crate::WebSocket;
use crate::{
    HttpResponseBuilder,
    request::HttpRequest,
    response_builder::{HttpResponse, HttpResponseBufferRef},
};

use protocols::error::Error;
use protocols::status_code::StatusCode;

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
        request: &HttpRequest<'_>,
        web_socket: WebSocket<'_, '_>,
    ) -> Result<(), ()> {
        Err(()) // Close the connection immediately
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::{HttpMethod, HttpRequest, StatusCode};
//     use heapless::Vec;

//     // TODO: Fix these tests
//     #[test]
//     fn test_simple_handler() {
//         // Test root path
//         let mut handler = SimpleHandler;
//         let request = HttpRequest {
//             method: HttpMethod::GET,
//             path: "/",
//             version: "HTTP/1.1",
//             headers: Vec::new(),
//             body: b"",
//         };

//         let buffer = &mut [0u8; 128];
//         let response_builder: HttpResponseBuilder<'_, BuildStatus> =
//             HttpResponseBuilder::new(buffer);
//         let response =
//             futures_lite::future::block_on(handler.handle_request(&request, response_builder))
//                 .unwrap();
//         let resp_str = heapless::String::<256>::from_utf8_lossy(response);
//         assert_eq!(resp_str, StatusCode::Ok);
//         assert_eq!(
//             response.body.as_str(),
//             Some("<h1>Hello from nanofish HTTP server!</h1>")
//         );

//         // Test health endpoint
//         let mut handler = SimpleHandler;
//         let request = HttpRequest {
//             method: HttpMethod::GET,
//             path: "/health",
//             version: "HTTP/1.1",
//             headers: Vec::new(),
//             body: b"",
//         };

//         let response = futures_lite::future::block_on(handler.handle_request(&request)).unwrap();
//         assert_eq!(response.status_code, StatusCode::Ok);
//         assert_eq!(response.body.as_str(), Some("{\"status\":\"ok\"}"));

//         // Test 404 path
//         let mut handler = SimpleHandler;
//         let request = HttpRequest {
//             method: HttpMethod::GET,
//             path: "/nonexistent",
//             version: "HTTP/1.1",
//             headers: Vec::new(),
//             body: b"",
//         };

//         let response = futures_lite::future::block_on(handler.handle_request(&request)).unwrap();
//         assert_eq!(response.status_code, StatusCode::NotFound);
//         assert_eq!(response.body.as_str(), Some("404 Not Found"));
//     }
// }
