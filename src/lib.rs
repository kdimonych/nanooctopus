#![cfg_attr(not(test), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// HTTP client implementation and request logic.
pub mod client;
/// HTTP request handlers and traits.
pub mod handler;
/// HTTP client configuration options.
pub mod options;
/// HTTP request types and parsing.
pub mod request;
/// HTTP response types and body handling.
pub mod response;
/// HTTP response builder utilities.
pub mod response_builder;
/// HTTP server implementation.
pub mod server;
/// Slice view utilities for HTTP responses.
pub mod slice_view;

// #[cfg(feature = "ws")]
// pub mod ws;

mod socket_pool;

pub use client::{DefaultHttpClient, HttpClient, SmallHttpClient};
pub use handler::{HttpHandler, SimpleHandler};
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

pub use request::HttpRequest;
pub use response_builder::{HttpResponse, HttpResponseBufferRef, HttpResponseBuilder};
pub use server::{DefaultHttpServer, HttpServer, HttpServerBuffers, ServerTimeouts, SmallHttpServer};
pub use socket_pool::{SocketBuffers, SocketPool};
