#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// HTTP header types and helpers.
pub mod header;

/// HTTP method enum and helpers.
pub mod method;

/// Stream-based HTTP request parser.
pub mod http_header_parser;

/// Predefined HTTP status codes as per RFC 2616.
pub mod status_code;

/// Error types for HTTP operations.
pub mod error;

/// A mocks module containing test utilities and mock implementations for HTTP operations, such as mock TCP sockets and stream readers.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;

/// This module contains the implementation of WebSocket traits and utilities, providing support for WebSocket communication in the library.
#[cfg(feature = "ws")]
pub mod web_socket;
