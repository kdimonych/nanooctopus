use crate::error::Error;
use crate::header::{HttpHeader, headers::*};
use crate::method::HttpMethod;
use abstarct_socket::detachable_buffer::DetachableBuffer;
use abstarct_socket::read_stream::ReadStream;
use abstarct_socket::read_stream_ext::{ReadError, ReadStreamExt};

const LINE_DELIMITTER: &[u8; 2] = b"\r\n";
const LINE_DELIMITTER_SIZE: usize = LINE_DELIMITTER.len();
const KEY_VALUE_DELIMITTER: char = ':';

/// Errors that can occur during HTTP header parsing
#[derive(Debug)]
pub enum HttpParseError<SocketReadErrorT> {
    /// Error occurred while reading from the stream
    ReadError(ReadError<SocketReadErrorT>),
    /// Malformed HTTP request
    MalformedRequest,
    /// HTTP method not recognized
    NoMethod,
    /// HTTP path not found
    NoPath,
    /// HTTP version not found
    NoVersion,
    /// Unsupported HTTP method
    UnsupportedMethod,
    /// Parsing cannot continue due to no Content-Length header.
    /// This error means that the stream is in an invalid state so it must be closed.
    NoContentLength,
}

impl<SocketReadErrorT> From<ReadError<SocketReadErrorT>> for HttpParseError<SocketReadErrorT> {
    fn from(err: ReadError<SocketReadErrorT>) -> Self {
        HttpParseError::ReadError(err)
    }
}

impl<SocketReadErrorT> From<HttpParseError<SocketReadErrorT>> for Error
where
    Error: From<ReadError<SocketReadErrorT>>,
{
    fn from(err: HttpParseError<SocketReadErrorT>) -> Self {
        match err {
            HttpParseError::ReadError(e) => Error::from(e),
            HttpParseError::MalformedRequest => Error::HeaderError("Malformed request"),
            HttpParseError::NoMethod => Error::InvalidData("No method"),
            HttpParseError::NoPath => Error::InvalidData("No path"),
            HttpParseError::NoVersion => Error::InvalidData("No version"),
            HttpParseError::UnsupportedMethod => Error::UnsupportedScheme("Unsupported method"),
            HttpParseError::NoContentLength => Error::InvalidData("No Content-Length header found"),
        }
    }
}

/// Stream-based HTTP request parser state machine
pub struct ReadFirstLine;
/// State markers for the different parts of the HTTP request being read
pub struct ReadHeaders {
    all_parsed: bool,
    content_length: Option<usize>,
}

impl ReadHeaders {
    /// Create a new ReadHeaders state
    pub const fn new() -> Self {
        Self {
            all_parsed: false,
            content_length: None,
        }
    }
}

#[derive(Debug)]
pub struct HttpFirstLine<'buf> {
    pub method: HttpMethod,
    pub path: &'buf str,
    pub version: &'buf str,
}

/// Stream-based HTTP request parser
pub struct HttpHeaderParser<'reader, Reader, ReadMethod>
where
    Reader: ?Sized,
{
    reader: &'reader mut Reader,
    state: ReadMethod,
}

impl<'reader, Reader: ?Sized> HttpHeaderParser<'reader, Reader, ReadFirstLine> {
    /// Create a new StreamRequest parser in the given state
    #[must_use]
    pub fn new(reader: &'reader mut Reader) -> Self {
        Self {
            reader,
            state: ReadFirstLine,
        }
    }
}

impl<'reader, Reader: ?Sized> HttpHeaderParser<'reader, Reader, ReadFirstLine> {
    /// Parse HTTP method from the stream
    ///
    /// The buffer is used to store the method string temporarily. It should be large enough to hold the method plus the following space.
    ///
    /// ## Returns
    /// A tuple that contains: (method, path_parser)
    /// - method - A method enum representing the HTTP method.
    /// - path_parser - The parser to parse the path out of the stream.
    ///
    /// ## Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_first_line<'buf>(
        self,
        buffer: &mut DetachableBuffer<'buf>,
    ) -> Result<
        (
            HttpFirstLine<'buf>,
            HttpHeaderParser<'reader, Reader, ReadHeaders>,
        ),
        HttpParseError<Reader::Error>,
    >
    where
        Reader: ReadStream,
    {
        let read_size = self
            .reader
            .read_till_stop_sequence(LINE_DELIMITTER, buffer.as_mut_slice())
            .await?;
        let line = buffer.detach(read_size);

        let line_str: &str = core::str::from_utf8(&line[..read_size - LINE_DELIMITTER_SIZE]) // Exclude the delimiter
            .map_err(|_| HttpParseError::MalformedRequest)?;

        //let mut parts = line_str.split_ascii_whitespace();
        let mut parts = line_str.split(|c: char| c == ' ');

        let method_str = parts.next().ok_or(HttpParseError::NoMethod)?.trim();
        if method_str.is_empty() {
            return Err(HttpParseError::NoMethod);
        }

        let path = parts.next().ok_or(HttpParseError::NoPath)?.trim();
        if path.is_empty() {
            return Err(HttpParseError::NoPath);
        }

        let version = parts.next().ok_or(HttpParseError::NoVersion)?.trim();
        if version.is_empty() {
            return Err(HttpParseError::NoVersion);
        }

        let method =
            HttpMethod::try_from(method_str).map_err(|_| HttpParseError::UnsupportedMethod)?;

        Ok((
            HttpFirstLine {
                method,
                path,
                version,
            },
            HttpHeaderParser {
                reader: self.reader,
                state: ReadHeaders::new(),
            },
        ))
    }
}

impl<'reader, Reader: ?Sized> HttpHeaderParser<'reader, Reader, ReadHeaders> {
    /// Parse HTTP path from the stream
    ///
    /// The buffer is used to store the path string temporarily. It should be large enough to hold the path plus the following space.
    ///
    /// ## Returns
    ///
    /// A tuple that contains: (`Option<HttpHeader>`, `buffer_tail`)
    /// - HttpHeader - The parsed HTTP header if available. If None, it indicates the end of headers.
    /// - buffer_tail - The remaining buffer after parsing the header.
    ///
    /// ## Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_next_header<'buf>(
        &mut self,
        buffer: &mut DetachableBuffer<'buf>,
    ) -> Result<Option<HttpHeader<'buf>>, HttpParseError<Reader::Error>>
    where
        Reader: ReadStream,
    {
        if self.state.all_parsed {
            // All headers have been parsed during current session
            return Ok(None);
        }

        let read_size = self
            .reader
            .read_till_stop_sequence(LINE_DELIMITTER, buffer.as_mut_slice())
            .await?;

        let header = buffer.detach(read_size);

        if read_size == LINE_DELIMITTER_SIZE {
            // Empty line indicates end of headers
            self.state.all_parsed = true;
            return Ok(None);
        }

        let header_str: &str = core::str::from_utf8(&header[..read_size - LINE_DELIMITTER_SIZE])
            .map_err(|_| HttpParseError::MalformedRequest)?;

        let (key_str, value_str) = header_str
            .split_once(KEY_VALUE_DELIMITTER)
            .ok_or(HttpParseError::MalformedRequest)?;

        if key_str.eq_ignore_ascii_case(CONTENT_LENGTH) {
            if self.state.content_length.is_some() {
                // Duplicate Content-Length header
                return Err(HttpParseError::MalformedRequest);
            }

            let content_length = value_str
                .trim()
                .parse::<usize>()
                .map_err(|_| HttpParseError::MalformedRequest)?;
            self.state.content_length = Some(content_length);
        }

        Ok(Some(HttpHeader::new(key_str.trim(), value_str.trim())))
    }

    /// Check if all headers have been parsed
    fn no_pending_headers(&self) -> bool {
        self.state.all_parsed
    }

    /// Get the content length if specified in headers
    fn content_length(&self) -> Option<usize> {
        self.state.content_length
    }

    /// Check if all headers have been parsed and stream is ready for body reading
    fn body_ready(&self) -> bool {
        self.no_pending_headers() && self.has_content_length()
    }

    /// Check if Content-Length header is present
    fn has_content_length(&self) -> bool {
        self.state.content_length.is_some()
    }

    /// This method finalizes the header parsing process and release the stream so it can be used for reading the body directly.
    /// It ensures that all headers are read and the Content-Length header is present.
    ///
    /// ## Returns
    /// The content length specified in the headers.
    ///
    /// ## Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails and releases the stream.
    /// - Returns `HttpParseError::NoContentLength` if the headers were not fully parsed or no Content-Length header was found during previous. This indicates
    ///   that the http datagram is not fully read yet, so we return unrecoverable error to indicate
    ///   that the stream is in invalid state.
    ///   It is responsibility of the caller to close the stream in this case.
    ///
    pub async fn finalize(
        mut self,
        buffer: &mut DetachableBuffer<'_>,
    ) -> Result<usize, HttpParseError<Reader::Error>>
    where
        Reader: ReadStream,
    {
        // Read out all remaining headers
        while self.parse_next_header(buffer).await?.is_some() {}

        let Some(content_length) = self.content_length() else {
            // There is no Content-Length header, so we cannot proceed to read the body.
            // We return unrecoverable error to indicate that the stream is in invalid state.
            // It is responsibility of the caller to close the stream in this case.
            return Err(HttpParseError::NoContentLength);
        };

        Ok(content_length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header;
    use abstarct_socket::mocks::multipart_read_stream::*;

    fn make_multipart_stream(chunk_size: usize, request: Vec<u8>) -> DummyMultipartReadStream {
        let parts_vec = request.chunks(chunk_size).map(|p| p.to_vec()).collect();
        DummyMultipartReadStream::new(&parts_vec)
    }

    #[tokio::test]
    async fn test_first_line() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\n";
        const EXPECTED_METHOD: HttpMethod = HttpMethod::GET;
        const EXPECTED_PATH: &str = "/index.html";
        const EXPECTED_VERSION: &str = "HTTP/1.1";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()];
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let (first_line, _) = parser
            .parse_first_line(&mut buffer)
            .await
            .expect("Failed to parse method");

        assert_eq!(first_line.method, EXPECTED_METHOD);
        assert_eq!(first_line.path, EXPECTED_PATH);
        assert_eq!(first_line.version, EXPECTED_VERSION);

        assert_eq!(buffer.len(), raw_buffer.len() - FIRST_LINE.len());
    }

    #[tokio::test]
    async fn test_first_line_insufficient_buffer_size() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\n";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len() - 1]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let e = parser
            .parse_first_line(&mut buffer)
            .await
            .map(|_| ())
            .expect_err("Expected failure due to insufficient buffer size");

        assert!(matches!(
            e,
            HttpParseError::ReadError(ReadError::TargetBufferOverflow)
        ));

        assert_eq!(buffer.len(), FIRST_LINE.len() - 1);
    }

    #[tokio::test]
    async fn test_first_line_invalid_method() {
        const FIRST_LINE: &str = "INVALID /index.html HTTP/1.1\r\n";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let e = parser
            .parse_first_line(&mut buffer)
            .await
            .map(|_| ())
            .expect_err("Expected failure due to unsupported method");
        assert!(matches!(e, HttpParseError::UnsupportedMethod));

        assert_eq!(buffer.len(), 0);
    }

    #[tokio::test]
    async fn test_first_line_no_method() {
        const FIRST_LINE: &str = "\r\n";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let e = parser
            .parse_first_line(&mut buffer)
            .await
            .map(|_| ())
            .expect_err("Expected failure due to missing method");

        assert!(matches!(e, HttpParseError::NoMethod));

        assert_eq!(buffer.len(), 0);
    }

    #[tokio::test]
    async fn test_first_line_no_path() {
        const FIRST_LINE: &str = "GET  HTTP/1.1\r\n";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let e = parser
            .parse_first_line(&mut buffer)
            .await
            .map(|_| ())
            .expect_err("Expected failure due to missing path");

        assert!(matches!(e, HttpParseError::NoPath));

        assert_eq!(buffer.len(), 0);
    }

    #[tokio::test]
    async fn test_first_line_no_version() {
        const FIRST_LINE: &str = "GET /index.html \r\n";

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let parser = HttpHeaderParser::new(&mut stream);

        let e = parser
            .parse_first_line(&mut buffer)
            .await
            .map(|_| ())
            .expect_err("Expected failure due to missing version");

        assert!(matches!(e, HttpParseError::NoVersion));

        assert_eq!(buffer.len(), 0);
    }

    async fn get_header_parser<'reader, 'buf, Stream>(
        stream: &'reader mut Stream,
        buffer: &mut DetachableBuffer<'buf>,
    ) -> HttpHeaderParser<'reader, Stream, ReadHeaders>
    where
        Stream: ReadStream,
        Stream::Error: core::fmt::Debug,
    {
        let parser = HttpHeaderParser::new(stream);

        let (_, header_parser) = parser
            .parse_first_line(buffer)
            .await
            .expect("Failed to parse method");

        header_parser
    }

    #[tokio::test]
    async fn test_parse_header() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";
        const EXPECTED_PARSED_PART: &str = "GET /index.html HTTP/1.1\r\nHost: example.com\r\n";
        const EXPECTED_HEADER_NAME: &str = header::headers::HOST;
        const EXPECTED_HEADER_VALUE: &str = "example.com";

        assert_ne!(FIRST_LINE, EXPECTED_PARSED_PART);

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let mut parser = get_header_parser(&mut stream, &mut buffer).await;

        let header = parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header")
            .expect("Expected at least one header line");

        assert_eq!(header.name, EXPECTED_HEADER_NAME);
        assert_eq!(header.value, EXPECTED_HEADER_VALUE);

        assert!(!parser.no_pending_headers());
        assert!(!parser.body_ready());
        assert!(!parser.has_content_length());
        assert!(parser.content_length() == None);

        assert_eq!(buffer.len(), raw_buffer.len() - EXPECTED_PARSED_PART.len());
    }

    #[tokio::test]
    async fn test_parse_header_last_is_none() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";
        const EXPECTED_PARSED_PART: &str = "GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";

        assert_eq!(FIRST_LINE, EXPECTED_PARSED_PART);

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let mut parser = get_header_parser(&mut stream, &mut buffer).await;

        parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header")
            .expect("Expected at least one header line");

        let opt = parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header");
        assert!(opt.is_none());

        assert!(parser.no_pending_headers());
        assert!(!parser.body_ready());
        assert!(!parser.has_content_length());
        assert!(parser.content_length() == None);

        assert_eq!(buffer.len(), raw_buffer.len() - EXPECTED_PARSED_PART.len());
    }

    #[tokio::test]
    async fn test_parse_header_parse_content_length() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\nContent-Length: 123\r\n\r\n";
        const EXPECTED_PARSED_PART: &str = "GET /index.html HTTP/1.1\r\nContent-Length: 123\r\n";
        const EXPECTED_HEADER_NAME: &str = header::headers::CONTENT_LENGTH;
        const EXPECTED_HEADER_VALUE: &str = "123";

        assert_ne!(FIRST_LINE, EXPECTED_PARSED_PART);

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let mut parser = get_header_parser(&mut stream, &mut buffer).await;

        let header = parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header")
            .expect("Expected at least one header line");

        assert_eq!(header.name, EXPECTED_HEADER_NAME);
        assert_eq!(header.value, EXPECTED_HEADER_VALUE);

        assert!(!parser.no_pending_headers());
        assert!(!parser.body_ready());
        assert!(parser.has_content_length());
        assert!(parser.content_length() == Some(123));

        assert_eq!(buffer.len(), raw_buffer.len() - EXPECTED_PARSED_PART.len());
    }

    #[tokio::test]
    async fn test_full_header_with_content_length() {
        const FIRST_LINE: &str = "GET /index.html HTTP/1.1\r\nContent-Length: 123\r\n\r\n";
        const EXPECTED_PARSED_PART: &str =
            "GET /index.html HTTP/1.1\r\nContent-Length: 123\r\n\r\n";
        assert_eq!(FIRST_LINE, EXPECTED_PARSED_PART);

        let mut stream = make_multipart_stream(2, FIRST_LINE.as_bytes().to_vec());

        let mut raw_buffer = [0u8; FIRST_LINE.len()]; // Intentionally smaller buffer
        let mut buffer = DetachableBuffer::new(&mut raw_buffer);

        let mut parser = get_header_parser(&mut stream, &mut buffer).await;

        parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header")
            .expect("Expected at least one header line");
        parser
            .parse_next_header(&mut buffer)
            .await
            .expect("Expected header");

        assert!(parser.no_pending_headers());
        assert!(parser.body_ready());
        assert!(parser.has_content_length());
        assert!(parser.content_length() == Some(123));

        assert_eq!(buffer.len(), raw_buffer.len() - EXPECTED_PARSED_PART.len());
    }
}
