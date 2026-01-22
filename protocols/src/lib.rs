#![cfg_attr(not(test), no_std)]

/// HTTP header types and helpers.
pub mod header;

/// HTTP method enum and helpers.
pub mod method;

/// Stream-based HTTP request parser.
pub mod http_header_parser;

/// WebSocket protocol support.
pub mod web_socket_proto;

/// Predefined HTTP status codes as per RFC 2616.
pub mod status_code;

/// Error types for HTTP operations.
pub mod error;
