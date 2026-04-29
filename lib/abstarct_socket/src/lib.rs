#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// Read stream extension trait and helper error types.
pub mod stream_search;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Socket pool implementation for managing multiple socket connections.
pub mod socket;

/// Socket builder trait and related utilities for constructing socket instances.
pub use socket::AbstractSocketListener;

/// Implementations for various socket types.
#[cfg(feature = "embassy_impl")]
pub mod embassy_impl;

/// Test mocks for read/write streams and related utilities.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
/// Tokio-specific adapters and wrappers for the socket traits.
#[cfg(feature = "tokio_impl")]
pub mod tokio_impl;
