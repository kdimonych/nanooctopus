#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(all(feature = "defmt", any(feature = "log", feature = "std")))]
compile_error!("the features `defmt` is not compatible with `log` or `std`");

#[cfg(all(test, feature = "defmt"))]
mod defmt_test_logger {
    #[defmt::global_logger]
    struct TestLogger;

    unsafe impl defmt::Logger for TestLogger {
        fn acquire() {}

        unsafe fn release() {}

        unsafe fn flush() {}

        unsafe fn write(bytes: &[u8]) {
            let _ = bytes;
        }
    }

    defmt::timestamp!("{=u8}", 0);
}

/// This module contains the implementation of the abstract socket traits and utilities,
/// providing a common interface for socket operations.
pub mod socket;

/// HTTP server implementation and related types.
mod server;

/// Handler trait and related types for processing HTTP requests.
mod handler;

/// WebSocket handling implementation and related types.
pub mod web_socket;

pub use handler::*;
pub use server::*;
