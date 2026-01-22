#![cfg_attr(not(test), no_std)]

/// HTTP read stream trait and error types.
pub mod read_stream;

/// HTTP read stream extention trait and error types.
pub mod read_stream_ext;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Test mocks for read streams and related utilities.
pub mod mocks;
