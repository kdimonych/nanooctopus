#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// HTTP request handlers and traits.
pub mod handler;
/// HTTP client configuration options.
pub mod options;
/// HTTP request types and parsing.
pub mod request;
/// HTTP response builder utilities.
pub mod response_builder;
/// HTTP server implementation.
pub mod server;

mod socket_pool;

pub use handler::HttpHandler;
pub use options::HttpClientOptions;
pub use protocols::error::Error;
pub use protocols::header::{HttpHeader, headers, mime_types};
pub use protocols::method::HttpMethod;
pub use protocols::status_code::StatusCode;

#[cfg(feature = "ws")]
pub use handler::{
    WebSocket, WebSocketError, WebSocketIoError, WebSocketRead, WebSocketReadReady, WebSocketState, WebSocketWrite,
    WebSocketWriteReady,
};

pub use abstarct_socket::socket::AbstractSocketBuilder;
pub use request::HttpRequest;
pub use response_builder::HttpResponseBuilder;
pub use server::{HttpServer, ServerTimeouts};
