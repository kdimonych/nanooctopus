#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(all(feature = "tokio_impl", feature = "embassy_impl"))]
compile_error!("features `tokio_impl` and `embassy_impl` are mutually exclusive");

#[cfg(not(any(feature = "tokio_impl", feature = "embassy_impl")))]
compile_error!("either feature `tokio_impl` or `embassy_impl` must be enabled");

#[cfg(all(feature = "tokio_impl", feature = "defmt"))]
compile_error!("feature `defmt` is only supported with `embassy_impl`");

#[cfg(all(feature = "tokio_impl", feature = "proto-ipv6"))]
compile_error!("feature `proto-ipv6` is only supported with `embassy_impl`");

#[cfg(all(feature = "embassy_impl", feature = "log"))]
compile_error!("feature `log` is only supported with `tokio_impl`");

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

/// HTTP request handlers and traits.
pub mod handler;
/// HTTP request types and parsing.
pub mod request;
/// HTTP response builder utilities.
pub mod response;
/// HTTP server implementation.
pub mod server;

/// Common utilities and types for socket management.
pub mod worker_memory;

mod allocator;

pub use handler::*;
pub use protocols::error::Error;
pub use protocols::header::{HttpHeader, headers, mime_types};
pub use protocols::method::HttpMethod;
pub use protocols::status_code::StatusCode;

pub use abstarct_socket::socket::{AbstractSocketBuilder, SocketEndpoint};

#[cfg(feature = "embassy_impl")]
pub use abstarct_socket::embassy_impl::socket::EmbassyTcpSocketBuilder;
#[cfg(feature = "tokio_impl")]
pub use abstarct_socket::tokio_impl::socket::TokioTcpSocketBuilder;

pub use allocator::HttpAllocator;
pub use request::HttpRequest;
pub use response::HttpResponseBuilder;
pub use server::{HttpServer, ServerTimeouts};
