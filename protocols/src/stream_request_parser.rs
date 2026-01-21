use crate::header::HttpHeader;
use crate::method::HttpMethod;
use crate::read_stream::{IntoHttpError, ReadStream, ReadStreamError, ReadStreamExt};

#[derive(Debug)]
pub enum HttpParseError<ReadError: IntoHttpError> {
    /// Error occurred while reading from the stream
    ReadError(ReadStreamError<ReadError>),
    /// Malformed HTTP request
    MalformedRequest,
    /// Unsupported HTTP method
    UnsupportedMethod,
}

impl<ReadError: IntoHttpError> From<ReadStreamError<ReadError>> for HttpParseError<ReadError> {
    fn from(err: ReadStreamError<ReadError>) -> Self {
        HttpParseError::ReadError(err)
    }
}

/// Stream-based HTTP request parser state machine
pub struct ReadMethod;
/// State markers for the different parts of the HTTP request being read
pub struct ReadPath;
/// State markers for the different parts of the HTTP request being read
pub struct ReadVersion;
/// State markers for the different parts of the HTTP request being read
pub struct ReadHeaders {
    all_parsed: bool,
}
/// State markers for the different parts of the HTTP request being read
pub struct ReadBody;

/// Stream-based HTTP request parser
pub struct StreamRequest<ReadPart> {
    state: ReadPart,
}

impl StreamRequest<ReadMethod> {
    /// Create a new StreamRequest parser in the given state
    #[must_use]
    pub fn new() -> Self {
        Self { state: ReadMethod }
    }
}

impl StreamRequest<ReadMethod> {
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
    pub async fn parse_method<'buf, Reader>(
        self,
        reader: &mut Reader,
        buffer: &'buf mut [u8],
    ) -> Result<
        ((HttpMethod, &'buf mut [u8]), StreamRequest<ReadPath>),
        HttpParseError<Reader::ReadError>,
    >
    where
        Reader: ReadStream + Send,
    {
        const DELIMITTER: u8 = b' ';
        const DELIMITTER_SIZE: usize = core::mem::size_of::<u8>();
        let read_size = reader.read_till_delimitter_byte(DELIMITTER, buffer).await?;
        let (method, tail) = buffer.split_at_mut(read_size);

        let method_str = core::str::from_utf8(&method[..read_size - DELIMITTER_SIZE]) // Exclude the delimiter
            .map_err(|_| HttpParseError::MalformedRequest)?;

        let method =
            HttpMethod::try_from(method_str).map_err(|_| HttpParseError::UnsupportedMethod)?;

        Ok(((method, tail), StreamRequest { state: ReadPath }))
    }
}

impl StreamRequest<ReadPath> {
    /// Parse HTTP path from the stream
    ///
    /// The buffer is used to store the path string temporarily. It should be large enough to hold the path plus the following space.
    ///
    /// ## Returns
    /// A tuple that contains: ((<size_of_path, path_str>), version_parser)
    /// - size_of_path - A size of buffer block occupied with the path string slice.
    /// - path_str - The path string slice itself.
    /// - version_parser - The parser to parse the version out of the stream.
    ///
    /// ## Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_path<'buf, Reader>(
        self,
        reader: &mut Reader,
        buffer: &'buf mut [u8],
    ) -> Result<
        ((&'buf str, &'buf mut [u8]), StreamRequest<ReadVersion>),
        HttpParseError<Reader::ReadError>,
    >
    where
        Reader: ReadStream + Send,
    {
        const DELIMITTER: u8 = b' ';
        const DELIMITTER_SIZE: usize = core::mem::size_of::<u8>();
        let read_size = reader.read_till_delimitter_byte(DELIMITTER, buffer).await?;

        let (path_buf, buffer_tail) = buffer.split_at_mut(read_size);

        let path_str = core::str::from_utf8(&path_buf[..read_size - DELIMITTER_SIZE])
            .map_err(|_| HttpParseError::MalformedRequest)?;

        Ok((
            (path_str, buffer_tail),
            StreamRequest { state: ReadVersion },
        ))
    }
}

impl StreamRequest<ReadVersion> {
    /// Parse HTTP path from the stream
    ///
    /// The buffer is used to store the path string temporarily. It should be large enough to hold the path plus the following space.
    ///
    /// ## Returns
    /// A tuple that contains: ((<size_of_version, version_str>), headers_parser)
    /// - size_of_version - A size of buffer block occupied with the version string slice.
    /// - version_str - The version string slice itself.
    /// - headers_parser - The parser to parse the headers out of the stream.
    ///
    /// ## Errorss
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_version<'buf, Reader>(
        self,
        reader: &mut Reader,
        buffer: &'buf mut [u8],
    ) -> Result<
        ((&'buf str, &'buf mut [u8]), StreamRequest<ReadHeaders>),
        HttpParseError<Reader::ReadError>,
    >
    where
        Reader: ReadStream + Send,
    {
        const DELIMITTER: &[u8; 2] = b"\r\n";
        const DELIMITTER_SIZE: usize = DELIMITTER.len();
        let read_size = reader
            .read_till_delimitter_sequence(DELIMITTER, buffer)
            .await?;

        let (version, tail) = buffer.split_at_mut(read_size);

        let version_str = core::str::from_utf8(&version[..read_size - DELIMITTER_SIZE])
            .map_err(|_| HttpParseError::MalformedRequest)?;

        Ok((
            (version_str, tail),
            StreamRequest {
                state: ReadHeaders { all_parsed: false },
            },
        ))
    }
}

impl StreamRequest<ReadHeaders> {
    /// Parse HTTP path from the stream
    ///
    /// The buffer is used to store the path string temporarily. It should be large enough to hold the path plus the following space.
    ///
    /// # Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_next_header<'buf, Reader>(
        &mut self,
        reader: &mut Reader,
        buffer: &'buf mut [u8],
    ) -> Result<(Option<HttpHeader<'buf>>, &'buf [u8]), HttpParseError<Reader::ReadError>>
    where
        Reader: ReadStream + Send,
    {
        const KEY_VALUE_DELIMITTER: char = ':';
        const LINE_DELIMITTER: &[u8; 2] = b"\r\n";
        const LINE_DELIMITTER_SIZE: usize = LINE_DELIMITTER.len();

        if self.state.all_parsed {
            // All headers have been parsed during current session
            return Ok((None, buffer));
        }

        let read_size = reader
            .read_till_delimitter_sequence(LINE_DELIMITTER, buffer)
            .await?;

        let (header, buffer_tail) = buffer.split_at_mut(read_size);

        if read_size == LINE_DELIMITTER_SIZE {
            // Empty line indicates end of headers
            self.state.all_parsed = true;
            return Ok((None, buffer_tail));
        }

        let header_str: &str = core::str::from_utf8(&header[..read_size - LINE_DELIMITTER_SIZE])
            .map_err(|_| HttpParseError::MalformedRequest)?;

        let (key_str, value_str) = header_str
            .split_once(KEY_VALUE_DELIMITTER)
            .ok_or(HttpParseError::MalformedRequest)?;

        Ok((
            Some(HttpHeader::new(key_str.trim(), value_str.trim())),
            buffer_tail,
        ))
    }

    // pub async fn go_body<Reader>(
    //     &mut self,
    //     reader: &mut Reader,
    // ) -> Result<
    //     ( &'buf mut [u8], StreamRequest<ReadHeaders>),
    //     HttpParseError<Reader::ReadError>,
    // > {
    //     while self.state.all_parsed == false {
    //         let mut temp_buffer = [0u8; 64];
    //         let (_header_opt, _tail) = self
    //             .parse_next_header(reader, &mut temp_buffer)
    //             .await?;
    //     }
    //     Ok(StreamRequest { state: ReadBody })
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{header, read_stream::tests::*};

    #[test]
    fn test_all_method_at_once() {
        let mut request_data = b"GET ".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 16];
        let buffer_len = buffer.len();
        let parse_future = parser.parse_method(&mut stream, &mut buffer);

        let ((method, tail), _) = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(parse_future)
            .expect("Failed to parse method");

        assert_eq!(tail.len(), buffer_len - request_data.len());
        assert_eq!(method, HttpMethod::GET);
    }

    #[tokio::test]
    async fn test_method_parse_part_by_part_no_filizing_space() {
        let mut request_data = b"UPDATE".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];
        let error = parser
            .parse_method(&mut stream, &mut method_buffer)
            .await
            .map(|(method, _)| method)
            .expect_err("Failed to parse method");

        // Should fail because "UPDATE" without space is not a complete method line
        assert!(matches!(
            error,
            HttpParseError::ReadError(ReadStreamError::ReadError(EOF))
        ));
    }

    #[tokio::test]
    async fn test_method_parse_part_by_part_with_chunked_method() {
        let request_data: Vec<Vec<u8>> = vec![b"CONNE".to_vec(), b"CT ".to_vec()];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 16];
        let buffer_len = buffer.len();

        let ((method, tail), _) = parser
            .parse_method(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse method");

        assert_eq!(tail.len(), buffer_len - b"CONNECT ".len());
        assert_eq!(method, HttpMethod::CONNECT);
    }

    #[tokio::test]
    async fn test_method_parse_with_truly_invalid_method() {
        let mut request_data =
            b"INVALID /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec();
        let mut stream = DummyReadStream::new(&mut request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 16];
        let result = parser.parse_method(&mut stream, &mut buffer).await;

        // Should fail with UnsupportedMethod
        assert!(matches!(result, Err(HttpParseError::UnsupportedMethod)));
    }

    #[tokio::test]
    async fn test_path_parse() {
        const EXPECTED_PATH: &str = "/index.html";

        // This test actually tests chunked method parsing across parts
        let request_data = vec![
            b"GE".to_vec(),
            b"T /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec(),
        ];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 16];
        let buf_len = buffer.len();
        let (_, path_parser) = parser
            .parse_method(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse method");

        let ((path, tail), _) = path_parser
            .parse_path(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse path");

        assert_eq!(tail.len(), buf_len - EXPECTED_PATH.len() - 1); // -1 for space
        assert_eq!(path, EXPECTED_PATH);
    }

    #[tokio::test]
    async fn test_version_parse() {
        const EXPECTED_VERSION: &str = "HTTP/1.1";

        // This test actually tests chunked method parsing across parts
        let request_data = vec![
            b"GE".to_vec(),
            b"T /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec(),
        ];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 16];
        let buffer_len = buffer.len();
        let (_, path_parser) = parser
            .parse_method(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse method");

        let (_, version_parser) = path_parser
            .parse_path(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse path");

        let ((version, tail), _) = version_parser
            .parse_version(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse version");

        assert_eq!(tail.len(), buffer_len - EXPECTED_VERSION.len() - 2); // -2 for \r\n
        assert_eq!(version, EXPECTED_VERSION);
    }

    #[tokio::test]
    async fn test_headers_parse() {
        // This test actually tests chunked method parsing across parts
        let request_data = vec![
            b"GE".to_vec(),
            b"T /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n".to_vec(),
        ];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let parser = StreamRequest::new();

        let mut buffer = [0u8; 32];
        let buffer_len = buffer.len();
        let (_, path_parser) = parser
            .parse_method(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse method");

        let (_, version_parser) = path_parser
            .parse_path(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse path");

        let (_, mut header_parser) = version_parser
            .parse_version(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse version");

        let (header_opt, tail) = header_parser
            .parse_next_header(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse header");

        assert_eq!(tail.len(), buffer_len - b"Host: example.com\r\n".len());

        let header = header_opt.expect("Expected a header");
        assert_eq!(header.name, header::headers::HOST);
        assert_eq!(header.value, "example.com");

        let (header_opt, tail) = header_parser
            .parse_next_header(&mut stream, &mut buffer)
            .await
            .expect("Failed to parse header");

        assert_eq!(tail.len(), buffer_len - b"\r\n".len());
        assert!(header_opt.is_none(), "Expected end of headers");
    }
}
