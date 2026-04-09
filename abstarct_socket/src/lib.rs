#![cfg_attr(not(test), no_std)]

/// Read stream trait and error types.
pub mod read_with;

/// Write stream trait and error types.
pub mod write_with;

/// Read stream extension trait and helper error types.
pub mod stream_search;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Implementations for various socket types.
pub mod embassy_impls;
/// Test mocks for read/write streams and related utilities.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
