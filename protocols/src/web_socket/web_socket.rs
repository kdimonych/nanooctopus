use crate::web_socket::header::*;
use crate::web_socket::header_reader::*;
use crate::web_socket::header_writer::*;
use abstarct_socket::head_arena::HeadArena;
use abstarct_socket::read_with::ReadWith;
use abstarct_socket::write_with::WriteWith;
use core::fmt::Debug;
use defmt_or_log as log;
use embedded_io_async::{Error, ErrorType, Read, ReadExactError, ReadReady, Write};

#[derive(Debug, PartialEq, Clone, Copy)]
enum PipeState {
    Open,
    Closed,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum WebSocketState {
    Open,
    Closing,
    ClosedByRemoteSide,
    Closed,
}

#[derive(Debug)]
pub enum WebSocketError<E: embedded_io_async::Error> {
    InvalidHeader,
    BufferOverflow,
    Closed,
    SocketError(E),
}

impl<E: embedded_io_async::Error> embedded_io_async::Error for WebSocketError<E> {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            WebSocketError::InvalidHeader => embedded_io_async::ErrorKind::InvalidData,
            WebSocketError::BufferOverflow => embedded_io_async::ErrorKind::OutOfMemory,
            WebSocketError::Closed => embedded_io_async::ErrorKind::BrokenPipe,
            WebSocketError::SocketError(e) => e.kind(),
        }
    }
}

impl<E: embedded_io_async::Error> From<WebSocketProtoError> for WebSocketError<E> {
    fn from(_err: WebSocketProtoError) -> Self {
        WebSocketError::InvalidHeader
    }
}

impl<E: embedded_io_async::Error> From<E> for WebSocketError<E> {
    fn from(err: E) -> Self {
        WebSocketError::SocketError(err)
    }
}

pub struct WebSocket<S> {
    socket: S,
    receiving_state: PipeState,
    sending_state: PipeState,
    recv_header_buffer: [u8; MAX_WS_FRAME_HEADER_SIZE],
    send_header_buffer: [u8; MAX_WS_FRAME_HEADER_SIZE],
    active_payload_reader: Option<WSPayloadReader>,
}

impl<S> WebSocket<S>
where
    S: ErrorType,
{
    pub const fn new(socket: S) -> Self {
        Self {
            socket,
            receiving_state: PipeState::Open,
            sending_state: PipeState::Open,
            recv_header_buffer: [0; MAX_WS_FRAME_HEADER_SIZE],
            send_header_buffer: [0; MAX_WS_FRAME_HEADER_SIZE],
            active_payload_reader: None,
        }
    }

    /// Performs close handshake and releases the underlying socket
    pub async fn release(mut self) -> (S, Result<(), WebSocketError<S::Error>>)
    where
        S: Write + Read + ReadReady,
        WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
    {
        let res = self.close().await;
        (self.socket, res)
    }

    fn close_on_critical_error<E>(&mut self, e: E) -> E
    where
        E: Debug,
    {
        log::error!(
            "WebSocket: Close due to unrecoverable error occurred: {:?}",
            log::Debug2Format(&e)
        );
        self.receiving_state = PipeState::Closed;
        self.sending_state = PipeState::Closed;
        e
    }

    pub async fn close(&mut self) -> Result<(), WebSocketError<S::Error>>
    where
        S: Write + Read + ReadReady,
        WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
    {
        if self.receiving_state == PipeState::Open && self.sending_state == PipeState::Open {
            // Flush any remaining data in the read stream. This allow to make sure we read any pending close frame.
            self.flush_read_stream().await?;

            // We are the first to initiate close
            self.send_close_frame().await?;
            self.sending_state = PipeState::Closed;

            self.wait_for_close_frame().await?;
            self.receiving_state = PipeState::Closed;
            return Ok(());
        } else if self.receiving_state == PipeState::Open {
            // Somehow the close procedure is not finished yet and we are waitng for the close frame
            self.wait_for_close_frame().await?;
            self.receiving_state = PipeState::Closed;
            return Ok(());
        } else if self.sending_state == PipeState::Open {
            // We need to send the close frame as we have received the close frame already
            self.send_close_frame().await?;
            self.sending_state = PipeState::Closed;
        }

        Ok(())
    }

    pub fn state(&self) -> WebSocketState {
        match (self.sending_state, self.receiving_state) {
            (PipeState::Open, PipeState::Open) => WebSocketState::Open,
            (PipeState::Closed, PipeState::Open) => WebSocketState::Closing,
            (PipeState::Open, PipeState::Closed) => WebSocketState::ClosedByRemoteSide,
            (PipeState::Closed, PipeState::Closed) => WebSocketState::Closed,
        }
    }

    async fn read_header(&mut self) -> Result<WSFrameHeader, WebSocketError<S::Error>>
    where
        S: Read,
        WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
    {
        let mut read_pos: usize = 0;
        let mut header_size: usize = MIN_WS_FRAME_HEADER_SIZE;

        loop {
            self.socket
                .read_exact(&mut self.recv_header_buffer[read_pos..header_size])
                .await
                .map_err(|e| self.close_on_critical_error(e))?;

            match read_frame_header(&self.recv_header_buffer[..header_size]) {
                Ok((header, _)) => {
                    return Ok(header);
                }
                Err(WebSocketProtoError::NotEnoughData(expected_size)) => {
                    // Next iteration will read more data
                    read_pos = header_size;
                    header_size = expected_size;
                    assert!(read_pos < header_size);
                    continue;
                }
                Err(_) => {
                    return {
                        self.close_on_critical_error(());
                        Err(WebSocketError::InvalidHeader)
                    };
                }
            };
        }
    }

    async fn send_close_frame(&mut self) -> Result<(), WebSocketError<S::Error>>
    where
        S: Write,
    {
        let header_size = write_frame_header(0, &mut self.send_header_buffer, WSOpcode::Close, 1);

        self.socket
            .write_all(&self.send_header_buffer[..header_size])
            .await
            .map_err(|e| self.close_on_critical_error(e))?;

        Ok(())
    }

    async fn flush_read_stream(&mut self) -> Result<(), WebSocketError<S::Error>>
    where
        S: Read + ReadReady,
        WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
    {
        // Read out previously active payload reader if any
        if let Some(mut payload_reader) = self.active_payload_reader.take() {
            log::warn!(
                "WebSocket: Flushing incomplete payload reader with {} bytes remaining",
                payload_reader.payload_len()
            );
            // Reuse the existing recv_header_buffer to read data into
            let mut buf = self.recv_header_buffer;

            while !payload_reader.is_complete() {
                let read_len: usize = payload_reader.payload_len();
                let actual_read_len = core::cmp::min(read_len, buf.len());
                self.socket
                    .read_exact(&mut buf[..actual_read_len])
                    .await
                    .map_err(|e| self.close_on_critical_error(e))?;
                payload_reader.decode_payload_in_place(&mut buf[..actual_read_len]);
            }
        }

        while self
            .socket
            .read_ready()
            .map_err(|e| self.close_on_critical_error(e))?
        {
            log::trace!("WebSocket: Flushing additional data from read stream");

            // There is more data to read, continue flushing
            let header: WSFrameHeader = self.read_header().await?;
            let mut payload_reader = WSPayloadReader::from_header(header);

            if payload_reader.opcode() == WSOpcode::Close {
                log::trace!("WebSocket: Close frame received during flush of read stream");
                self.receiving_state = PipeState::Closed;
                break;
            }

            let mut buf = self.recv_header_buffer;
            while !payload_reader.is_complete() {
                let read_len: usize = payload_reader.payload_len();
                let actual_read_len = core::cmp::min(read_len, buf.len());
                self.socket
                    .read_exact(&mut buf[..actual_read_len])
                    .await
                    .map_err(|e| self.close_on_critical_error(e))?;
                payload_reader.decode_payload_in_place(&mut buf[..actual_read_len]);
            }
        }
        Ok(())
    }

    async fn wait_for_close_frame(&mut self) -> Result<(), WebSocketError<S::Error>>
    where
        S: Read,
        WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
    {
        loop {
            let header: WSFrameHeader = self.read_header().await?;
            let mut payload_reader = WSPayloadReader::from_header(header);

            let mut buf = [0u8; 128];
            while !payload_reader.is_complete() {
                let read_len: usize = payload_reader.payload_len();
                let actual_read_len = core::cmp::min(read_len, buf.len());
                self.socket
                    .read_exact(&mut buf[..actual_read_len])
                    .await
                    .map_err(|e| self.close_on_critical_error(e))?;
                payload_reader.decode_payload_in_place(&mut buf[..actual_read_len]);
            }

            if payload_reader.opcode() == WSOpcode::Close {
                log::trace!("WebSocket: Close frame received");
                return Ok(());
            }
        }
    }
}

impl<S> ErrorType for WebSocket<S>
where
    S: ErrorType,
{
    type Error = WebSocketError<S::Error>;
}

impl<S> Read for WebSocket<S>
where
    S: Read + ErrorType,
    WebSocketError<S::Error>: From<ReadExactError<S::Error>>,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, WebSocketError<S::Error>> {
        let mut payload_reader = if let Some(payload_reader) = self.active_payload_reader.take() {
            payload_reader
        } else {
            if self.receiving_state == PipeState::Closed {
                return Err(WebSocketError::Closed);
            }
            let header: WSFrameHeader = self.read_header().await?;
            WSPayloadReader::from_header(header)
        };

        let read_len: usize = payload_reader.payload_len();
        let actual_read_len = core::cmp::min(read_len, buf.len());
        self.socket
            .read_exact(&mut buf[..actual_read_len])
            .await
            .map_err(|e| self.close_on_critical_error(e))?;
        payload_reader.decode_payload_in_place(&mut buf[..actual_read_len]);

        if payload_reader.opcode() == WSOpcode::Close {
            // Mark receiving pipe as closed
            log::info!("WebSocket: Received close frame during read");
            self.receiving_state = PipeState::Closed;
        }

        if !payload_reader.is_complete() {
            self.active_payload_reader = Some(payload_reader);
        }
        Ok(actual_read_len)
    }
}

impl<S> Write for WebSocket<S>
where
    S: Write + ErrorType,
{
    /// Writes data to the WebSocket stream.
    /// This method sends the data as a binary WebSocket frame.
    /// Returns the number of bytes written.
    ///
    /// ### Error:
    /// - `WebSocketError::Closed`: If the WebSocket sending pipe is closed.
    /// - `WebSocketError::SocketError`: If there is an error while writing to the underlying socket.
    ///
    async fn write(&mut self, buf: &[u8]) -> Result<usize, WebSocketError<S::Error>> {
        if self.sending_state == PipeState::Closed {
            return Err(WebSocketError::Closed);
        }

        let header_size =
            write_frame_header(buf.len(), &mut self.send_header_buffer, WSOpcode::Binary, 1);

        self.socket
            .write_all(&self.send_header_buffer[..header_size])
            .await
            .map_err(|e| self.close_on_critical_error(e))?;

        self.socket
            .write_all(buf)
            .await
            .map_err(|e| self.close_on_critical_error(e))?;

        Ok(buf.len())
    }

    #[inline]
    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.socket
            .flush()
            .await
            .map_err(|e| self.close_on_critical_error(e))?;
        Ok(())
    }

    #[inline]
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.write(buf)
            .await
            .map_err(|e| self.close_on_critical_error(e))?;
        Ok(())
    }
}
