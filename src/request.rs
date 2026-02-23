use heapless::Vec;
use protocols::error::Error;
use protocols::header::HttpHeader;
use protocols::method::HttpMethod;

use abstarct_socket::head_arena::HeadArena;
use abstarct_socket::read_with::ReadWith;
use embedded_io_async::Read;
use protocols::http_header_parser::HttpHeaderParser;

/// Maximum number of headers allowed in a request
pub const MAX_HEADERS: usize = 16;

/// HTTP request parsed from client
#[derive(Debug)]
pub struct HttpRequest<'a> {
    /// HTTP method
    pub method: HttpMethod,
    /// Request path
    pub path: &'a str,
    /// HTTP version (e.g., "HTTP/1.1")
    pub version: &'a str,
    /// Request headers
    pub headers: Vec<HttpHeader<'a>, MAX_HEADERS>,
    /// Request body (if present)
    pub body: &'a [u8],

    /// WebSocket key if this is a WebSocket upgrade request
    #[cfg(feature = "ws")]
    pub web_socket_key: Option<&'a str>,
}

impl<'a> HttpRequest<'a> {
    const fn new(method: HttpMethod, path: &'a str, version: &'a str) -> Self {
        Self {
            method,
            path,
            version,
            headers: Vec::new(),
            body: &[],
            #[cfg(feature = "ws")]
            web_socket_key: None,
        }
    }

    /// Try to parse an HTTP request from a TCP stream asynchronously
    /// ## Errors
    /// Returns an error if:
    /// - Reading from the stream fails
    /// - The request is malformed  
    ///
    pub async fn try_parse_from_stream<'buf, Reader>(
        stream: &'_ mut Reader,
        read_buf: &'buf mut [u8],
    ) -> Result<HttpRequest<'buf>, Error>
    where
        Reader: ReadWith + Read,
        Error: From<Reader::Error>,
    {
        let parser = HttpHeaderParser::new(stream);
        let mut buffer = HeadArena::new(read_buf);

        let (first_line, mut parser) = parser.parse_first_line(&mut buffer).await?;
        let mut request = HttpRequest::new(first_line.method, first_line.path, first_line.version);

        #[cfg(feature = "ws")]
        let mut web_socket_search = WebSocketKeySearch::new();
        let mut content_length_search = ContentLengthSearch::new();

        while let Some(header) = parser.parse_next_header(&mut buffer).await? {
            #[cfg(feature = "ws")]
            let is_filtered_out = { content_length_search.process(&header)? || web_socket_search.process(&header) };
            #[cfg(not(feature = "ws"))]
            let is_filtered_out = content_length_search.process(&header)?;

            if is_filtered_out {
                continue;
            }

            request
                .headers
                .push(header)
                .map_err(|_| Error::InvalidData("Too many headers"))?;
        }

        #[cfg(feature = "ws")]
        {
            request.web_socket_key = web_socket_search.web_socket_key();
        }

        // Finalize the parser (e.g., read body if needed)
        parser.finalize(&mut buffer).await?;

        let body_size = content_length_search.content_length().unwrap_or(0);

        if buffer.len() < body_size {
            // Not enough buffer to receive body
            return Err(Error::ReadBufferOverflow);
        }

        stream.read_exact(&mut buffer.as_mut_slice()[..body_size]).await?;
        request.body = buffer.take_front(body_size);

        Ok(request)
    }
}

struct ContentLengthSearch {
    length: Option<usize>,
}

impl ContentLengthSearch {
    /// Create a new ContentLengthSearch
    pub const fn new() -> Self {
        Self { length: None }
    }

    /// Process a header to check for Content-Length
    ///
    /// ## Returns
    /// - Returns Ok(true) if the header was Content-Length and processed
    /// - Returns Ok(false) if the header was not Content-Length
    ///
    /// ## Errors
    /// - Returns `Error::InvalidData` if multiple Content-Length headers are found or if
    /// the value is invalid.
    pub fn process(&mut self, header: &HttpHeader<'_>) -> Result<bool, Error> {
        let mut res: bool = false;
        if self.length.is_none() {
            if header.name.eq_ignore_ascii_case("Content-Length") {
                let val = header
                    .value
                    .parse::<usize>()
                    .map_err(|_| Error::InvalidData("Invalid Content-Length header value"))?;
                self.length = Some(val);
                res = true;
            }
        } else {
            return Err(Error::InvalidData("Multiple Content-Length headers found"));
        }
        Ok(res)
    }

    pub fn content_length(&self) -> Option<usize> {
        self.length
    }
}

#[cfg(feature = "ws")]
struct WebSocketKeySearch<'buf> {
    is_done: bool,
    is_upgrade: bool,
    is_connection_upgrade: bool,
    key: Option<&'buf str>,
}
#[cfg(feature = "ws")]
impl<'buf> WebSocketKeySearch<'buf> {
    /// Create a new WebSocketKeySearch
    pub const fn new() -> Self {
        Self {
            is_done: false,
            is_upgrade: false,
            is_connection_upgrade: false,
            key: None,
        }
    }

    /// Process a header to check for WebSocket upgrade headers
    /// ## Returns
    /// - Returns Some(key) if the WebSocket key is found and all upgrade headers are present
    /// - Returns None otherwise
    pub fn process(&mut self, header: &HttpHeader<'buf>) -> bool {
        let mut res: bool = false;
        if self.is_done {
            return res;
        }

        if self.key.is_none() && header.name.eq_ignore_ascii_case("Sec-WebSocket-Key") {
            self.key = Some(header.value);
            res = true;
        } else if !self.is_upgrade
            && header.name.eq_ignore_ascii_case("Upgrade")
            && header.value.eq_ignore_ascii_case("websocket")
        {
            self.is_upgrade = true;
            res = true;
        } else if !self.is_connection_upgrade
            && header.name.eq_ignore_ascii_case("Connection")
            && header.value.eq_ignore_ascii_case("upgrade")
        {
            self.is_connection_upgrade = true;
            res = true;
        }

        self.is_done = self.is_upgrade && self.is_connection_upgrade && self.key.is_some();

        res
    }

    pub fn web_socket_key(&self) -> Option<&'buf str> {
        if self.is_done { self.key } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpMethod;
    use abstarct_socket::mocks::mock_read_stream::MockReadStream;

    fn create_mock_stream<'buf>(data: &'buf mut [u8]) -> MockReadStream<'buf> {
        MockReadStream::new(data)
    }

    #[tokio::test]
    async fn test_try_parse_from_stream() {
        let mut request = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let request = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect("Expected successful parse");

        assert_eq!(request.method, HttpMethod::GET);
        assert_eq!(request.path, "/index.html");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 2);
        for header in request.headers {
            match header.name {
                "Host" => assert_eq!(header.value, "example.com"),
                "User-Agent" => assert_eq!(header.value, "test"),
                _ => panic!("Unexpected header"),
            }
        }
        assert_eq!(request.body, b"");
    }

    #[tokio::test]
    async fn test_try_parse_from_stream_post_with_body() {
        let mut request =
            b"POST /api/data HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"key\":\"value\"}".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let request = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect("Expected successful parse");

        assert_eq!(request.method, HttpMethod::POST);
        assert_eq!(request.path, "/api/data");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 1);

        #[cfg(feature = "ws")]
        {
            assert!(request.web_socket_key.is_none());
        }

        for header in request.headers {
            match header.name {
                "Content-Type" => assert_eq!(header.value, "application/json"),
                _ => panic!("Unexpected header"),
            }
        }
        assert_eq!(request.body, b"{\"key\":\"value\"}");
    }

    #[tokio::test]
    async fn test_parse_request_invalid_method() {
        let mut request = b"INVALID /path HTTP/1.1\r\n\r\n".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let e = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect_err("Expected error due to invalid method");

        assert!(matches!(e, Error::HeaderError(_)));
    }

    #[tokio::test]
    async fn test_parse_request_missing_version() {
        let mut request = b"GET /path\r\n\r\n".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let e = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect_err("Expected error due to invalid method");

        assert!(matches!(e, Error::HeaderError(_)));
    }

    #[tokio::test]
    async fn test_parse_request_missing_path() {
        let mut request = b"GET  \r\n\r\n".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let e = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect_err("Expected error due to invalid method");

        assert!(matches!(e, Error::HeaderError(_)));
    }

    #[tokio::test]
    async fn test_parse_request_missing_method() {
        let mut request = b"  \r\n\r\n".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let e = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect_err("Expected error due to invalid method");

        assert!(matches!(e, Error::HeaderError(_)));
    }

    #[tokio::test]
    async fn test_parse_request_empty_request() {
        let mut request = b"".to_vec();

        let mut stream = create_mock_stream(request.as_mut_slice());
        let mut buffer = [0u8; 256];

        let e = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
            .await
            .expect_err("Expected error due to invalid method");

        assert!(matches!(e, Error::SocketError(_)));
    }

    #[tokio::test]
    async fn test_parse_request_all_http_methods() {
        let methods = [
            ("GET", HttpMethod::GET),
            ("POST", HttpMethod::POST),
            ("PUT", HttpMethod::PUT),
            ("DELETE", HttpMethod::DELETE),
            ("PATCH", HttpMethod::PATCH),
            ("HEAD", HttpMethod::HEAD),
            ("OPTIONS", HttpMethod::OPTIONS),
            ("TRACE", HttpMethod::TRACE),
            ("CONNECT", HttpMethod::CONNECT),
        ];

        for (method_str, expected_method) in &methods {
            let request_str = format!("{method_str} /path HTTP/1.1\r\n\r\n");
            let mut request_bytes = std::vec::Vec::from(request_str.as_bytes());

            let mut stream = create_mock_stream(request_bytes.as_mut_slice());
            let mut buffer = [0u8; 256];

            let request = HttpRequest::try_parse_from_stream(&mut stream, &mut buffer)
                .await
                .expect("Expected successful parse");

            assert_eq!(request.method, *expected_method);
        }
    }
}
