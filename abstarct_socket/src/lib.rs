#![cfg_attr(not(test), no_std)]

/// HTTP read stream trait and error types.
pub mod read_stream;

/// HTTP read stream extention trait and error types.
pub mod read_stream_ext;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Implementations for various socket types.
pub mod embassy_impls;

/// Buffer types for borrowed data.
pub mod borrowed_buffer;

/// A bump allocator based on the principle of play all-in, then gather the cream.
pub mod detachable_buffer;

/// Test mocks for read streams and related utilities.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
