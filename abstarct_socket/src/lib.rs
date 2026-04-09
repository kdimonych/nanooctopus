#![cfg_attr(not(test), no_std)]

/// Read stream trait and error types.
pub mod read_with;

/// Write stream trait and error types.
pub mod write_with;

/// Read stream extention trait and error types.
pub mod read_with_ext;

/// Utility to find value sequences in a stream.
pub mod find_sequence;

/// Implementations for various socket types.
pub mod embassy_impls;

/// Prefix-detaching arena primitives.
pub mod arena;

/// Buffer types for staging written data in arena-backed storage.
pub mod staging_buffer;

/// Backward-compatible exports for the previous head_arena module path.
pub mod head_arena;

pub use arena::{ArenaView, PrefixArena};
pub use staging_buffer::StagingBuffer;

/// Test mocks for read/write streams and related utilities.
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
