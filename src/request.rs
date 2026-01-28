use heapless::Vec;
use protocols::error::Error;
use protocols::header::HttpHeader;
use protocols::method::HttpMethod;

use abstarct_socket::head_arena::HeadArena;
use abstarct_socket::read_stream::ReadStream;
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

/// Find the position of the double CRLF sequence that separates headers from body
fn find_double_crlf(data: &[u8]) -> Option<usize> {
    const DOUBLE_CRLF: &[u8] = b"\r\n\r\n";
    (0..data.len().saturating_sub(3)).find(|&i| &data[i..i + 4] == DOUBLE_CRLF)
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
        buf: &'buf mut [u8],
    ) -> Result<HttpRequest<'buf>, Error>
    where
        Reader: ReadStream,
        Error: From<Reader::Error>,
    {
        let parser = HttpHeaderParser::new(stream);
        let mut buffer = HeadArena::new(buf);

        let (first_line, mut parser) = parser.parse_first_line(&mut buffer).await?;
        let mut request = HttpRequest::new(first_line.method, first_line.path, first_line.version);

        for _ in 0..MAX_HEADERS {
            let header = parser.parse_next_header(&mut buffer).await?;

            if let Some(header) = header {
                request
                    .headers
                    .push(header)
                    .map_err(|_| Error::InvalidData("Too many headers"))?;
            } else {
                break;
            }
        }

        // Finalize the parser (e.g., read body if needed)
        let body_size = parser.finalize(&mut buffer).await?;
        if buffer.len() < body_size {
            // Not enough buffer to receive body
            return Err(Error::ReadBufferOverflow);
        }

        let actually_read = stream
            .read_exact(&mut buffer.as_mut_slice()[..body_size])
            .await?;
        request.body = buffer.take_front(actually_read);

        #[cfg(feature = "ws")]
        {
            request.web_socket_key = try_get_web_socket_key(&request.headers);
        }

        Ok(request)
    }

    /// Parse an HTTP request from headers string and body bytes
    ///
    /// ## Errors
    ///
    /// Returns an error if:
    /// - The request line is missing or malformed
    /// - The HTTP method is invalid or unsupported  
    /// - Required parts (method, path, version) are missing
    /// - Too many headers are provided (exceeds `MAX_HEADERS`)
    pub fn parse_from(headers_str: &'a str, body: &'a [u8]) -> Result<Self, Error> {
        let mut lines = headers_str.lines();

        // Parse request line
        let request_line = lines
            .next()
            .ok_or(Error::InvalidData("Missing request line"))?;
        let mut parts = request_line.split_whitespace();

        let method_str = parts.next().ok_or(Error::InvalidData("Missing method"))?;
        let path = parts.next().ok_or(Error::InvalidData("Missing path"))?;
        let version = parts.next().ok_or(Error::InvalidData("Missing version"))?;

        let method = HttpMethod::try_from(method_str)
            .map_err(|_| Error::InvalidData("Unknown HTTP method"))?;

        // Parse headers
        let mut headers = Vec::new();

        for line in lines {
            if line.is_empty() {
                break;
            }

            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim();
                let value = line[colon_pos + 1..].trim();

                let header = HttpHeader::new(name, value);
                headers
                    .push(header)
                    .map_err(|_| Error::InvalidData("Too many headers"))?;
            }
        }

        #[cfg(feature = "ws")]
        let web_socket_key = try_get_web_socket_key(&headers);

        Ok(HttpRequest {
            method,
            path,
            version,
            headers,
            body,
            #[cfg(feature = "ws")]
            web_socket_key,
        })
    }
}

#[cfg(feature = "ws")]
fn try_get_web_socket_key<'buf, const N: usize>(
    headers: &'_ Vec<HttpHeader<'buf>, N>,
) -> Option<&'buf str> {
    // Find the Sec-WebSocket-Key header if present.
    // This check is done first because it appears rarely in requests, so we can avoid unnecessary iterations.
    let res = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("Sec-WebSocket-Key"))
        .map(|header| header.value);

    if !headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("Upgrade")
            && header.value.eq_ignore_ascii_case("websocket")
    }) {
        return None;
    };

    if !headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("Connection")
            && header.value.eq_ignore_ascii_case("upgrade")
    }) {
        return None;
    }

    res
}

impl<'a> TryFrom<&'a [u8]> for HttpRequest<'a> {
    type Error = Error;

    fn try_from(buffer: &'a [u8]) -> Result<Self, Self::Error> {
        // Find the end of headers (double CRLF)
        let end_of_headers =
            find_double_crlf(buffer).ok_or(Error::InvalidData("Incomplete request headers"))?;

        // Parse the headers string
        let headers_str = core::str::from_utf8(&buffer[..end_of_headers])
            .map_err(|_| Error::InvalidData("Invalid UTF-8 in request"))?;

        // Body starts after the double CRLF
        let body = &buffer[end_of_headers + 4..];

        Self::parse_from(headers_str, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpMethod;
    use abstarct_socket::mocks::read_stream::DummyReadStream;

    fn create_mock_stream(data: &mut [u8]) -> DummyReadStream {
        DummyReadStream::new(data)
    }

    #[test]
    fn test_parse_request_get() {
        let request_str =
            "GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";
        let body = b"";

        let request: HttpRequest<'_> = HttpRequest::parse_from(request_str, body).unwrap();

        assert_eq!(request.method, HttpMethod::GET);
        assert_eq!(request.path, "/index.html");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.body, b"");
    }

    #[tokio::test]
    async fn test_try_parse_from_stream() {
        let mut request =
            b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n".to_vec();

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

    #[test]
    fn test_parse_request_post_with_body() {
        let request_str = "POST /api/data HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n";
        let body = b"{\"key\":\"value\"}";

        let request = HttpRequest::parse_from(request_str, body).unwrap();

        assert_eq!(request.method, HttpMethod::POST);
        assert_eq!(request.path, "/api/data");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.body, b"{\"key\":\"value\"}");

        // Check specific headers
        let content_type_header = request
            .headers
            .iter()
            .find(|h| h.name == "Content-Type")
            .unwrap();
        assert_eq!(content_type_header.value, "application/json");
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
        assert_eq!(request.headers.len(), 2);
        for header in request.headers {
            match header.name {
                "Content-Type" => assert_eq!(header.value, "application/json"),
                "Content-Length" => assert_eq!(header.value, "15"),
                _ => panic!("Unexpected header"),
            }
        }
        assert_eq!(request.body, b"{\"key\":\"value\"}");
    }

    #[test]
    fn test_parse_request_invalid_method() {
        let request_str = "INVALID /path HTTP/1.1\r\n\r\n";
        let body = b"";

        let result = HttpRequest::parse_from(request_str, body);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_request_missing_parts() {
        // Missing path
        let request_str = "GET HTTP/1.1\r\n\r\n";
        let body = b"";
        let result = HttpRequest::parse_from(request_str, body);
        assert!(result.is_err());

        // Missing version
        let request_str = "GET /path\r\n\r\n";
        let result = HttpRequest::parse_from(request_str, body);
        assert!(result.is_err());

        // Empty request
        let request_str = "";
        let result = HttpRequest::parse_from(request_str, body);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_request_all_http_methods() {
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
            let request = HttpRequest::parse_from(&request_str, b"").unwrap();
            assert_eq!(request.method, *expected_method);
        }
    }

    #[test]
    fn test_find_double_crlf() {
        // Normal case
        let data = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\nBody";
        assert_eq!(find_double_crlf(data), Some(33));

        // At the beginning
        let data = b"\r\n\r\nBody";
        assert_eq!(find_double_crlf(data), Some(0));

        // At the end
        let data = b"Headers\r\n\r\n";
        assert_eq!(find_double_crlf(data), Some(7));

        // Not found
        let data = b"GET / HTTP/1.1\r\nHost: example.com\r\n";
        assert_eq!(find_double_crlf(data), None);

        // Too short
        let data = b"\r\n\r";
        assert_eq!(find_double_crlf(data), None);

        // Empty
        let data = b"";
        assert_eq!(find_double_crlf(data), None);
    }

    #[test]
    fn test_try_from_complete_request() {
        let buffer = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";

        let request = HttpRequest::try_from(buffer.as_slice()).unwrap();

        assert_eq!(request.method, HttpMethod::GET);
        assert_eq!(request.path, "/index.html");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.body, b"");
    }

    #[test]
    fn test_try_from_request_with_body() {
        let buffer =
            b"POST /api/data HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"key\":\"value\"}";

        let request = HttpRequest::try_from(buffer.as_slice()).unwrap();

        assert_eq!(request.method, HttpMethod::POST);
        assert_eq!(request.path, "/api/data");
        assert_eq!(request.version, "HTTP/1.1");
        assert_eq!(request.headers.len(), 1);
        assert_eq!(request.body, b"{\"key\":\"value\"}");
    }

    #[test]
    fn test_try_from_incomplete_headers() {
        let buffer = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\n";

        let result = HttpRequest::try_from(buffer.as_slice());
        assert!(result.is_err());
    }

    #[test]
    fn test_try_from_invalid_utf8() {
        // Create buffer with invalid UTF-8 in headers
        let mut buffer: Vec<u8, 128> = Vec::new();
        let _ = buffer.extend_from_slice(b"GET /index.html HTTP/1.1\r\nHost: ");
        let _ = buffer.push(0xFF); // Invalid UTF-8
        let _ = buffer.extend_from_slice(b"\r\n\r\n");

        let result = HttpRequest::try_from(buffer.as_slice());
        assert!(result.is_err());
    }
}
