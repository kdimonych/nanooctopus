#![cfg_attr(not(test), no_std)]

/// HTTP header types and helpers.
pub mod header;

/// HTTP method enum and helpers.
pub mod method;

/// Stream-based HTTP request parser.
pub mod stream_request_parser;

/// WebSocket protocol support.
pub mod web_socket_proto;
