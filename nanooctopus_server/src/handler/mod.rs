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

/// Content type header used in HTTP responses.
pub const H_CONTENT_TYPE: &str = "Content-Type";
/// Content length header for HTTP responses.
pub const H_CONTENT_LENGTH: &str = "Content-Length";
/// Content encoding header for HTTP responses.
pub const H_CONTENT_ENCODING: &str = "Content-Encoding";
/// Cross-Origin Resource Policy header for HTTP responses.
pub const H_CACHE_CONTROL: &str = "Cache-Control";
/// Cross-Origin Resource Policy header for HTTP responses.
pub const H_CROSS_ORIGIN_RESOURCE_POLICY: &str = "Cross-Origin-Resource-Policy";

/// JSON content type used in HTTP responses.
pub const CONTENT_TYPE_JSON: &str = "application/json";
/// Plain text content type used in HTTP responses.
pub const CONTENT_TYPE_TEXT_PLAIN: &str = "text/plain";
/// HTML content type used in HTTP responses.
pub const CONTENT_TYPE_HTML: &str = "text/html";
/// HTML content type with UTF-8 charset used in HTTP responses.
pub const CONTENT_TYPE_HTML_UTF8: &str = "text/html; charset=utf-8";
/// image/x-icon content type used in HTTP responses.
pub const CONTENT_TYPE_IMAGE_X_ICON: &str = "image/x-icon";

/// Gzip content encoding used in HTTP responses.
pub const CONTENT_ENCODING_GZIP: &str = "gzip";

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
                (H_CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN),
                (H_CONTENT_LENGTH, content_length_str.as_str()),
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
        write!(&mut content_length_str, "{}", self.favicon_data.len()).map_err(|_| IoError::InvalidState)?;

        conn.initiate_response(
            200,
            Some("OK"),
            &[
                (H_CONTENT_TYPE, CONTENT_TYPE_IMAGE_X_ICON),
                (H_CONTENT_LENGTH, content_length_str.as_str()),
                (H_CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (H_CROSS_ORIGIN_RESOURCE_POLICY, "same-origin"),
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

/// A handler that serves a plain text response with the given content.
#[allow(unused)]
pub struct PlainTextHandler<'a> {
    content: &'a str,
}

impl<'a> PlainTextHandler<'a> {
    /// Create a new `PlainTextHandler` with the given content.
    pub const fn new(content: &'a str) -> Self {
        Self { content }
    }
}

impl<'a> Handler for PlainTextHandler<'a> {
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
        write!(&mut content_length_str, "{}", self.content.len()).map_err(|_| IoError::InvalidState)?;

        conn.initiate_response(
            200,
            Some("OK"),
            &[
                (H_CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN),
                (H_CONTENT_LENGTH, content_length_str.as_str()),
            ],
        )
        .await?;
        conn.write_all(self.content.as_bytes()).await?;
        conn.flush().await?;
        conn.complete().await?;

        Ok(())
    }
}

enum HtmlContentEncoding<'a> {
    Text(&'a str),
    Gzip(&'a [u8]),
}

/// A handler that serves a plain text response with the given content.
#[allow(unused)]
pub struct HtmlHandler<'a> {
    content_type: &'static str,
    content: HtmlContentEncoding<'a>,
}

impl<'a> HtmlHandler<'a> {
    /// Create a new `HtmlHandler` with the given content.
    pub const fn new(content: &'a str) -> Self {
        Self {
            content_type: CONTENT_TYPE_HTML,
            content: HtmlContentEncoding::Text(content),
        }
    }

    /// Enable gzip compression for the response. This will set the `Content-Encoding` header to `gzip`.
    pub const fn compressed_gzip(content: &'a [u8]) -> Self {
        Self {
            content_type: CONTENT_TYPE_HTML,
            content: HtmlContentEncoding::Gzip(content),
        }
    }

    /// Create a new `HtmlHandler` with the given content and UTF-8 charset.
    pub const fn with_utf8_charset(self) -> Self {
        Self {
            content_type: CONTENT_TYPE_HTML_UTF8,
            content: self.content,
        }
    }
}

impl<'a> Handler for HtmlHandler<'a> {
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
        match self.content {
            HtmlContentEncoding::Text(content) => {
                let mut content_length_str = heapless::String::<16>::new();
                write!(&mut content_length_str, "{}", content.len()).map_err(|_| IoError::InvalidState)?;

                conn.initiate_response(
                    200,
                    Some("OK"),
                    &[
                        (H_CONTENT_TYPE, self.content_type),
                        (H_CONTENT_LENGTH, content_length_str.as_str()),
                    ],
                )
                .await?;

                conn.write_all(content.as_bytes()).await?;
            }
            HtmlContentEncoding::Gzip(content) => {
                let mut content_length_str = heapless::String::<16>::new();
                write!(&mut content_length_str, "{}", content.len()).map_err(|_| IoError::InvalidState)?;

                conn.initiate_response(
                    200,
                    Some("OK"),
                    &[
                        (H_CONTENT_TYPE, self.content_type),
                        (H_CONTENT_LENGTH, content_length_str.as_str()),
                        (H_CONTENT_ENCODING, CONTENT_ENCODING_GZIP),
                    ],
                )
                .await?;

                conn.write_all(content).await?;
            }
        }
        // The response has already been handled in the match above.
        conn.flush().await?;
        conn.complete().await?;

        Ok(())
    }
}
