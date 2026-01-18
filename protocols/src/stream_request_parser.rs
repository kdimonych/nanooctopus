use crate::method::HttpMethod;

pub trait HttpReadStreamError {}

impl HttpReadStreamError for () {}

pub trait ReadStream {
    type ReadError: HttpReadStreamError;

    fn read_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::ReadError>> + Send
    where
        F: FnMut(&[u8]) -> (usize, R) + Send;
}

#[derive(Debug)]
pub enum HttpParseError<ReadError: HttpReadStreamError> {
    /// Error occurred while reading from the stream
    ReadError(ReadError),
    /// Malformed HTTP request
    MalformedRequest,
    /// Unsupported HTTP method
    UnsupportedMethod,
}

impl<ReadError: HttpReadStreamError> From<ReadError> for HttpParseError<ReadError> {
    fn from(err: ReadError) -> Self {
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
pub struct ReadHeaders;
/// State markers for the different parts of the HTTP request being read
pub struct ReadBody;

/// Stream-based HTTP request parser
pub struct StreamRequest<ReadPart> {
    _state: core::marker::PhantomData<ReadPart>,
}

impl StreamRequest<ReadMethod> {
    /// Create a new StreamRequest parser in the given state
    #[must_use]
    pub fn new() -> Self {
        Self {
            _state: core::marker::PhantomData,
        }
    }
}

impl StreamRequest<ReadMethod> {
    /// Parse HTTP method from the stream
    ///
    /// The buffer is used to store the method string temporarily. It should be large enough to hold the method.
    ///
    /// # Errors
    /// - Returns `HttpParseError::ReadError` if reading from the stream fails
    /// - Returns `HttpParseError::MalformedRequest` if the method is not valid UTF-8
    /// - Returns `HttpParseError::UnsupportedMethod` if the method is not recognized
    pub async fn parse_method<Reader>(
        &mut self,
        reader: &mut Reader,
        buffer: &mut [u8],
    ) -> Result<(HttpMethod, StreamRequest<ReadPath>), HttpParseError<Reader::ReadError>>
    where
        Reader: ReadStream,
    {
        let mut reade_size = 0;

        // Read method into buffer until space or buffer full
        while reader
            .read_with(|data| {
                let buffer_space = buffer.len() - reade_size;
                if let Some(space_pos) = data.iter().position(|&b| b == b' ') {
                    let to_read = core::cmp::min(buffer_space, space_pos);
                    buffer[reade_size..reade_size + to_read].copy_from_slice(&data[..to_read]);
                    reade_size += to_read;
                    (to_read, false)
                } else {
                    // Read as much as possible into the buffer
                    let to_read = core::cmp::min(buffer_space, data.len());
                    buffer[reade_size..reade_size + to_read].copy_from_slice(&data[..to_read]);
                    reade_size += to_read;
                    (to_read, to_read != 0)
                }
            })
            .await?
        {}

        let method_str = core::str::from_utf8(&buffer[..reade_size])
            .map_err(|_| HttpParseError::MalformedRequest)?;

        let method =
            HttpMethod::try_from(method_str).map_err(|_| HttpParseError::UnsupportedMethod)?;

        Ok((
            method,
            StreamRequest::<ReadPath> {
                _state: core::marker::PhantomData,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct DummyReadStream<'a> {
        buffer: &'a [u8],
        position: usize,
    }

    impl<'a> DummyReadStream<'a> {
        fn new(buffer: &'a [u8]) -> Self {
            Self {
                buffer,
                position: 0,
            }
        }
    }

    #[derive(Debug)]
    struct EOF;

    impl HttpReadStreamError for EOF {}

    impl<'a> ReadStream for DummyReadStream<'a> {
        type ReadError = EOF;

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&[u8]) -> (usize, R) + Send,
        {
            let size = self.buffer.len() - self.position;
            if self.position >= size {
                return Err(EOF);
            }

            let data = &self.buffer[self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    struct DummyMultipartReadStream<'a> {
        multipart_buffer: &'a [&'a [u8]],
        part: usize,
        position: usize,
    }

    impl<'a> DummyMultipartReadStream<'a> {
        fn new(multipart_buffer: &'a [&'a [u8]]) -> Self {
            Self {
                multipart_buffer,
                part: 0,
                position: 0,
            }
        }
    }

    impl<'a> ReadStream for DummyMultipartReadStream<'a> {
        type ReadError = ();

        async fn read_with<F, R>(&mut self, mut f: F) -> Result<R, Self::ReadError>
        where
            F: FnMut(&[u8]) -> (usize, R) + Send,
        {
            if self.part >= self.multipart_buffer.len() {
                return Err(());
            }

            if self.position >= self.multipart_buffer[self.part].len() {
                self.part += 1;
                self.position = 0;
                if self.part >= self.multipart_buffer.len() {
                    return Err(());
                }
            }

            let data = &self.multipart_buffer[self.part][self.position..];
            let (read_bytes, res) = f(data);
            self.position += read_bytes;
            Ok(res)
        }
    }

    #[test]
    fn test_all_method_at_once() {
        let request_data = b"GET ";
        let mut stream = DummyReadStream::new(request_data);
        let mut parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];
        let parse_future = parser.parse_method(&mut stream, &mut method_buffer);

        let (method, _next_parser) = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(parse_future)
            .expect("Failed to parse method");
        assert_eq!(method, HttpMethod::GET);
    }

    #[tokio::test]
    async fn test_method_parse_part_by_part_no_filizing_space() {
        let request_data = b"UPDATE";
        let mut stream = DummyReadStream::new(request_data);
        let mut parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];
        let error = parser.parse_method(&mut stream, &mut method_buffer).await.map(|(method,_)| method).expect_err("Failed to parse method");

        // Should fail because "UPDATE" without space is not a complete method line
        assert!(matches!(error, HttpParseError::ReadError(EOF)));
    }

    #[tokio::test]
    async fn test_method_parse_part_by_part_with_chunked_method() {
        let request_data: [&[u8]; 2] = [&b"CONNE"[..], &b"CT "[..]];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let mut parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];

        let (method, _next_parser) = parser
            .parse_method(&mut stream, &mut method_buffer)
            .await
            .expect("Failed to parse method");
        assert_eq!(method, HttpMethod::CONNECT);
    }

    #[tokio::test]
    async fn test_method_parse_with_invalid_method() {
        // This test actually tests chunked method parsing across parts
        let request_data: [&[u8]; 2] = [
            &b"GE"[..],
            &b"T /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n"[..],
        ];
        let mut stream = DummyMultipartReadStream::new(&request_data);
        let mut parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];
        let (method, _next_parser) = parser
            .parse_method(&mut stream, &mut method_buffer)
            .await
            .expect("Failed to parse method");

        assert_eq!(method, HttpMethod::GET);
    }

    #[tokio::test]
    async fn test_method_parse_with_truly_invalid_method() {
        let request_data = b"INVALID /index.html HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mut stream = DummyReadStream::new(request_data);
        let mut parser = StreamRequest::new();

        let mut method_buffer = [0u8; 16];
        let result = parser.parse_method(&mut stream, &mut method_buffer).await;

        // Should fail with UnsupportedMethod
        assert!(matches!(result, Err(HttpParseError::UnsupportedMethod)));
    }
}
