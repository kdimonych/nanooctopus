#![doc = include_str!("./README.md")]
#![allow(async_fn_in_trait)]
/// Socket pool implementation for managing multiple socket connections.
mod socket_traits;

/// Trait defining the behavior of a socket listener, including methods for accepting incoming connections and retrieving the local endpoint.
mod socket_listener;

/// Re-export of the socket module for easier access to socket traits and types.
pub use socket_traits::*;

/// Re-export of the socket listener module for easier access to socket listener traits and types.
pub use socket_listener::*;
