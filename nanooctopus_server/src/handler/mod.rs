#![allow(async_fn_in_trait)]

/// This module contains some macros for conveniently creating handlers for the HTTP server.
pub mod macros;

use crate::socket::*;
use core::fmt::Write;
use core::fmt::{Debug, Display};
pub use edge_http::RequestHeaders;
pub use edge_http::io::Body;
pub use edge_http::io::Error as IoError;
pub use edge_http::io::server::{Connection, Handler};

/// An error type that represents possible errors that can occur while handling HTTP requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[defmt_or_log::maybe_derive_format]
pub enum HdlError {
    /// The handler does not support the requested path.
    NotFound,
}

/// A type alias for the handler error type used in the HTTP server.
pub type HandlerError<E> = edge_http::io::server::HandlerError<E, HdlError>;

/// A handler that serves a default response for the root path.
pub struct DefaultRootResponse;

impl Handler for DefaultRootResponse {
    type Error<E>
        = IoError<E>
    where
        E: Debug;

    async fn handle<S, const CN: usize>(
        &self,
        _task_id: impl Display + Copy,
        conn: &mut Connection<'_, S, CN>,
    ) -> Result<(), Self::Error<S::Error>>
    where
        S: SocketRead + SocketWrite + SocketSplit,
    {
        const HELLO_WORLD: &[u8] = b"Ok!";
        let mut content_length_str = heapless::String::<16>::new();
        let _ = write!(&mut content_length_str, "{}", HELLO_WORLD.len());

        conn.initiate_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "text/plain"),
                ("Content-Length", content_length_str.as_str()),
            ],
        )
        .await?;

        conn.write_all(HELLO_WORLD).await?;
        conn.flush().await?;
        conn.complete().await?;

        Ok(())
    }
}

/// A handler that serves a favicon.ico file in response to requests for the /favicon.ico path.
pub struct FaviconHandler<'a> {
    favicon_data: &'a [u8],
}

impl<'a> FaviconHandler<'a> {
    /// Create a new `FaviconResponse` handler with the given favicon data.
    pub const fn new(favicon_data: &'a [u8]) -> Self {
        Self { favicon_data }
    }
}

impl<'a> Handler for FaviconHandler<'a> {
    type Error<E>
        = IoError<E>
    where
        E: Debug;

    async fn handle<S, const CN: usize>(
        &self,
        _task_id: impl Display + Copy,
        conn: &mut Connection<'_, S, CN>,
    ) -> Result<(), Self::Error<S::Error>>
    where
        S: SocketRead + SocketWrite + SocketSplit,
    {
        let mut content_length_str = heapless::String::<16>::new();
        let _ = write!(&mut content_length_str, "{}", self.favicon_data.len());

        conn.initiate_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "image/x-icon"),
                ("Content-Length", content_length_str.as_str()),
                ("Cache-Control", "public, max-age=31536000, immutable"),
                ("Cross-Origin-Resource-Policy", "same-origin"),
            ],
        )
        .await?;

        conn.write_all(self.favicon_data).await?;
        conn.flush().await?;
        conn.complete().await?;

        Ok(())
    }
}

impl<E> TryFrom<HandlerError<E>> for HdlError {
    type Error = ();

    fn try_from(value: HandlerError<E>) -> Result<Self, Self::Error> {
        match value {
            HandlerError::Handler(e) => Ok(e),
            _ => Err(()),
        }
    }
}

impl<E> From<HdlError> for HandlerError<E> {
    fn from(value: HdlError) -> Self {
        HandlerError::Handler(value)
    }
}
