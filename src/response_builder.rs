use crate::slice_view::SliceView;
use protocols::error::Error;
use protocols::status_code::StatusCode;

/// Marker types for the builder stages
pub struct NotCreated;
/// Marker type for building status stage
pub struct BuildStatus;
/// Marker type for building body stage
pub struct BuildHeader;

/// HTTP Response Builder for constructing HTTP responses in a staged manner.
pub struct HttpResponseBuilder<'a, Stage = NotCreated> {
    base: BuilderBase<'a>,
    _marker: core::marker::PhantomData<Stage>,
}

struct BuilderBase<'a> {
    buffer: SliceView<'a>,
    auto_close_connection: bool,
}

/// A buffer wrapper for HTTP response building.
/// This struct holds a mutable reference to a byte slice used as a temporary buffer for constructing HTTP responses.
/// This buffer is intended to use with `HttpResponseBuilder` only.
pub struct HttpResponseBufferRef<'a> {
    inner: &'a mut [u8],
    auto_close_connection: bool,
}

impl HttpResponseBufferRef<'_> {
    /// Returns HttpResponseBuffer with reborrowed inner buffer.
    pub fn reborrow(&mut self) -> HttpResponseBufferRef<'_> {
        HttpResponseBufferRef {
            inner: self.inner,
            auto_close_connection: self.auto_close_connection,
        }
    }

    /// Binds a mutable byte slice to the HttpResponseBuffer.
    pub fn bind<'a>(buffer: &'a mut [u8], auto_close_connection: bool) -> HttpResponseBufferRef<'a> {
        HttpResponseBufferRef {
            inner: buffer,
            auto_close_connection,
        }
    }
}

/// The HTTP response type.
/// This is a dummy struct representing the finalized HTTP response.
pub struct HttpResponse {
    response_len: usize,
}

impl HttpResponse {
    /// Returns the length of the HTTP response.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.response_len
    }

    /// Returns true if the HTTP response is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.response_len == 0
    }
}

impl<'a> HttpResponseBuilder<'a, NotCreated> {
    /// Creates a new HttpResponseBuilder with the provided buffer.
    pub fn new(buffer: HttpResponseBufferRef<'a>) -> HttpResponseBuilder<'a, BuildStatus> {
        HttpResponseBuilder {
            base: BuilderBase {
                buffer: SliceView::new(buffer.inner),
                auto_close_connection: buffer.auto_close_connection,
            },
            _marker: core::marker::PhantomData,
        }
    }
}

impl<'a> HttpResponseBuilder<'a, BuildStatus> {
    /// Adds a header to the HTTP response.
    pub fn with_status(mut self, status_code: StatusCode) -> Result<HttpResponseBuilder<'a, BuildHeader>, Error> {
        // Write "HTTP/1.1 "
        self.base.extend_from_str("HTTP/1.1 ")?;

        // Write status code as decimal
        self.base.extend_from_decimal(status_code.as_u16() as usize)?;

        // Write " <reason>\r\n"
        self.base.extend_from_str(" ")?;
        self.base.extend_from_str(status_code.text())?;
        self.base.new_line()?;

        // If auto-close connection is set, add the header
        if self.base.auto_close_connection {
            self.base.extend_from_str("Connection: close\r\n")?;
        }

        Ok(HttpResponseBuilder {
            base: self.base,
            _marker: core::marker::PhantomData,
        })
    }

    /// Creates a preflight response with CORS-like PNA headers.
    pub fn preflight_response(self) -> Result<HttpResponse, Error> {
        self.with_status(StatusCode::NoContent)?
            .with_cors_like_pna_headers()?
            .with_no_body()
    }
}

impl<'a> HttpResponseBuilder<'a, BuildHeader> {
    /// Adds a header to the HTTP response.
    pub fn add_header(&mut self, name: &str, value: &str) -> Result<(), Error> {
        // Write header name
        self.write_header_name(name)?;
        // Write header value
        self.write_header_value(value)?;
        // Finalize header line
        self.new_line()?;

        Ok(())
    }

    /// Adds a header with value filled by a custom filler function.
    pub fn add_header_value_from_filler<Filler>(&mut self, name: &str, filler: Filler) -> Result<(), Error>
    where
        Filler: FnOnce(&mut [u8]) -> Result<usize, Error>,
    {
        // Write header name
        self.write_header_name(name)?;
        // Write header value
        self.base.buffer.fill_with(filler)?;
        // Finalize header line
        self.new_line()?;

        Ok(())
    }

    /// Adds a header with value filled by a custom filler function and returns self for chaining.
    pub fn with_header_value_from_filler<Filler>(mut self, name: &str, filler: Filler) -> Result<Self, Error>
    where
        Filler: FnOnce(&mut [u8]) -> Result<usize, Error>,
    {
        self.add_header_value_from_filler(name, filler)?;

        Ok(self)
    }

    /// Adds a header to the HTTP response and returns self for chaining.
    pub fn with_header(mut self, name: &str, value: &str) -> Result<Self, Error> {
        self.add_header(name, value)?;
        Ok(self)
    }

    /// Finalizes response.
    #[inline(always)]
    pub fn with_no_body(mut self) -> Result<HttpResponse, Error> {
        self.add_header("Content-Length", "0")?;
        self.new_line()?;
        Ok(self.finalize())
    }

    /// Prepares the builder to add a body to the HTTP response.
    pub fn with_body_filler<Filler>(mut self, filler: Filler) -> Result<HttpResponse, Error>
    where
        Filler: FnOnce(&mut [u8]) -> Result<usize, Error>,
    {
        const CONTENT_LENGTH_PLACEHOLDER_SIZE: usize = 10;
        // Prepare space for Content-Length header
        self.write_header_name("Content-Length")?;

        // Prepare placeholder for content length
        let placeholder_pos = self.base.buffer.len();

        let value_buf = self
            .base
            .buffer
            .try_allocate(CONTENT_LENGTH_PLACEHOLDER_SIZE)
            .map_err(|_| Error::InvalidStatusCode)?;
        value_buf.fill(b'0');
        //value_buf.fill_with(f);
        self.base.new_line()?;

        self.base.new_line()?;

        let content_length = self.base.buffer.fill_with(filler)?;
        // Update Content-Length placeholder
        self.base.buffer.modify_inner(|inner_buf| {
            let placeholder_slice = &mut inner_buf[placeholder_pos..placeholder_pos + CONTENT_LENGTH_PLACEHOLDER_SIZE];
            write_decimal_to_placeholder(placeholder_slice, content_length).unwrap();
        });

        Ok(self.finalize())
    }

    /// Prepares the builder to add a binary body to the HTTP response.
    pub fn with_body_from_slice(self, s: &[u8]) -> Result<HttpResponse, Error> {
        self.with_body_filler(|body_buf| {
            if s.len() > body_buf.len() {
                return Err(Error::InvalidStatusCode);
            }
            let len = s.len().min(body_buf.len());
            body_buf[..len].copy_from_slice(&s[..len]);
            Ok(len)
        })
    }

    /// Prepares the builder to add a text body to the HTTP response.
    pub fn with_body_from_str(self, s: &str) -> Result<HttpResponse, Error> {
        self.with_body_from_slice(s.as_bytes())
    }

    /// Prepares the builder to add a plain text body to the HTTP response.
    pub fn with_plain_text_body(self, s: &str) -> Result<HttpResponse, Error> {
        self.with_header("Content-Type", "text/plain; charset=utf-8")?
            .with_body_from_str(s)
    }

    /// Adds CORS-like PNA headers to the HTTP response.
    /// These headers allow cross-origin requests and private network access.
    pub fn with_cors_like_pna_headers(self) -> Result<Self, Error> {
        self.with_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")?
            .with_header("Access-Control-Allow-Origin", "*")?
            .with_header("Access-Control-Allow-Private-Network", "true")?
            .with_header("Access-Control-Allow-Headers", "Content-Type")
    }

    /// Creates the response out of compressed HTML page
    /// # Note:
    /// The page data must be in HTML format compressed with gzip algorithm.
    /// No check is performed to verify the format.
    ///
    pub fn with_compressed_page(self, page_data: &[u8]) -> Result<HttpResponse, Error> {
        self.with_header("Content-Encoding", "gzip")?
            .with_header("Content-Type", "text/html; charset=utf-8")?
            .with_body_from_slice(page_data)
    }

    /// Creates the response out of HTML page
    /// # Note:
    /// The page data must be in HTML format.
    /// No check is performed to verify the format.
    ///
    pub fn with_page(self, page_data: &[u8]) -> Result<HttpResponse, Error> {
        self.with_header("Content-Type", "text/html; charset=utf-8")?
            .with_body_from_slice(page_data)
    }

    fn write_header_name(&mut self, name: &str) -> Result<(), Error> {
        self.base.extend_from_str(name)?;
        self.base.extend_from_str(": ")
    }

    #[inline(always)]
    fn write_header_value(&mut self, value: &str) -> Result<(), Error> {
        self.base.extend_from_str(value)
    }

    #[inline(always)]
    fn new_line(&mut self) -> Result<(), Error> {
        self.base.new_line()
    }

    fn finalize(self) -> HttpResponse {
        HttpResponse {
            response_len: self.base.buffer.len(),
        }
    }
}

impl BuilderBase<'_> {
    pub fn new_line(&mut self) -> Result<(), Error> {
        self.buffer
            .extend_from_slice(b"\r\n")
            .map_err(|_| Error::InvalidStatusCode)
    }

    pub fn extend_from_slice(&mut self, slice: &[u8]) -> Result<(), Error> {
        self.buffer
            .extend_from_slice(slice)
            .map_err(|_| Error::InvalidStatusCode)
    }

    #[inline(always)]
    pub fn extend_from_str(&mut self, s: &str) -> Result<(), Error> {
        self.extend_from_slice(s.as_bytes())
    }

    /// Write a decimal number to the buffer
    pub fn extend_from_decimal(&mut self, mut num: usize) -> Result<(), Error> {
        if num == 0 {
            self.buffer.push(b'0').map_err(|_| Error::InvalidStatusCode)?;
            return Ok(());
        }

        let mut digits = [0u8; 10];
        let mut i = 0;

        while num > 0 {
            #[allow(clippy::cast_possible_truncation)]
            {
                digits[i] = (num % 10) as u8 + b'0';
            }
            num /= 10;
            i += 1;
        }

        // Write digits in reverse order
        for j in (0..i).rev() {
            self.buffer.push(digits[j]).map_err(|_| Error::InvalidStatusCode)?;
        }

        Ok(())
    }
}

fn write_decimal_to_placeholder(bytes: &mut [u8], mut num: usize) -> Result<(), Error> {
    if bytes.is_empty() {
        return Err(Error::InvalidStatusCode);
    }
    bytes.fill(b'0');

    if num == 0 {
        return Ok(());
    }

    let mut i = bytes.len();

    while num > 0 {
        if i == 0 {
            // Not enough space to write the number
            return Err(Error::InvalidStatusCode);
        }

        #[allow(clippy::cast_possible_truncation)]
        {
            bytes[i - 1] = (num % 10) as u8 + b'0';
        }
        num /= 10;
        i -= 1;
    }

    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use heapless::Vec;

//     #[test]
//     fn test_http_response_builder_create_with_status_only() {
//         // let mut buffer = [0u8; 128];
//         // let builder = HttpResponseBuilder::new(&mut buffer);
//         // let response = builder.with_status(StatusCode::Ok).unwrap().build();
//         // let expected = b"HTTP/1.1 200 OK\r\n";
//         // assert_eq!(&response.buffer.as_slice()[..expected.len()], expected);
//     }

//     // TODO: Add more tests for headers and body building
// }
