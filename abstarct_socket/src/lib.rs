#![cfg_attr(not(test), no_std)]

/// Read stream trait and error types.
pub mod read_with;

/// Read stream extention trait and error types.
pub mod read_with_ext;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Implementations for various socket types.
pub mod embassy_impls;

/// Buffer types for borrowed data.
pub mod borrowed_buffer;

/// A bump allocator based on the principle of play all-in, then gather the cream.
pub mod head_arena;

/// Test mocks for read/write streams and related utilities.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
