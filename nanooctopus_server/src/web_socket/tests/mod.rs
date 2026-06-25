#[allow(dead_code)]
mod helpers;
mod socket_mock;

use super::*;
use mockall::Sequence;

use edge_ws::FrameHeader;
use helpers::*;
use socket_mock::{MockError, MockSocket};

#[test]
fn test_web_socket_compiles_with_references_to_tcp_socket() {
    let mut socket = MockSocket::new();
    let _ = WebSocket::new(&mut socket);
}

#[tokio::test]
async fn test_create_web_socket() {
    let socket = MockSocket::new();
    let web_socket = WebSocket::new(socket);

    assert_eq!(
        web_socket.is_message_pending(),
        false,
        "There should not be pending messages after creating a new WebSocket"
    );

    assert!(
        web_socket.get_read_message_type().is_none(),
        "The read message type should be None after creating a new WebSocket"
    );

    assert_eq!(
        web_socket.get_send_frame_type(),
        MessageType::Binary,
        "The write message type should be Binary after creating a new WebSocket"
    );
}

async fn test_process_empty_data_frame(expected_frame_type: MessageType) {
    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();
    socket
        .expect_read()
        .once()
        .in_sequence(&mut seq)
        .returning(FrameHeaderBuilder::new(expected_frame_type.to_frame_type(false)).build());

    let frame_type = WebSocket::new(socket)
        .wait_message()
        .await
        .expect("No error should occur");

    assert_eq!(frame_type, expected_frame_type);
}

#[tokio::test]
async fn test_process_empty_text_frame() {
    test_process_empty_data_frame(MessageType::Text).await;
}

#[tokio::test]
async fn test_process_empty_binary_frame() {
    test_process_empty_data_frame(MessageType::Binary).await;
}

async fn test_process_data_frame(expected_frame_type: MessageType) {
    const PAYLOAD: &[u8] = b"Hello, WebSocket!";

    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();
    socket.expect_read().once().in_sequence(&mut seq).returning(
        FrameHeaderBuilder::new(expected_frame_type.to_frame_type(false))
            .with_payload_len(PAYLOAD.len() as u64)
            .build(),
    );

    socket
        .expect_read()
        .in_sequence(&mut seq)
        .times(2)
        .returning(MaskedBinaryFrame::new(PAYLOAD).build());

    let mut web_socket = WebSocket::new(socket);

    let frame_type = web_socket.wait_message().await.expect("No error should occur");
    assert_eq!(frame_type, expected_frame_type);

    let tmp_buf = &mut [0u8; 10];

    const EXPECTED_FIRST_READ: &[u8] = b"Hello, Web";
    assert_eq!(
        web_socket.read(tmp_buf).await.expect("No error should occur"),
        EXPECTED_FIRST_READ.len()
    );
    assert!(tmp_buf.starts_with(EXPECTED_FIRST_READ));

    const EXPECTED_SECOND_READ: &[u8] = b"Socket!";
    assert_eq!(
        web_socket.read(tmp_buf).await.expect("No error should occur"),
        EXPECTED_SECOND_READ.len()
    );
    assert!(tmp_buf.starts_with(EXPECTED_SECOND_READ));
}

#[tokio::test]
async fn test_process_text_frame() {
    test_process_data_frame(MessageType::Text).await;
}

#[tokio::test]
async fn test_process_binary_frame() {
    test_process_data_frame(MessageType::Binary).await;
}

async fn test_read_ready(returns: bool) {
    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();
    socket
        .expect_read_ready()
        .once()
        .in_sequence(&mut seq)
        .returning(move || Ok(returns));

    let mut web_socket = WebSocket::new(socket);

    assert_eq!(web_socket.read_ready().expect("No error should occur"), returns);
}

#[tokio::test]
async fn test_read_ready_returns_true() {
    test_read_ready(true).await;
}

#[tokio::test]
async fn test_read_ready_returns_false() {
    test_read_ready(false).await;
}

async fn test_write_ready(returns: bool) {
    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();
    socket
        .expect_write_ready()
        .once()
        .in_sequence(&mut seq)
        .returning(move || Ok(returns));

    let mut web_socket = WebSocket::new(socket);

    assert_eq!(web_socket.write_ready().expect("No error should occur"), returns);
}

#[tokio::test]
async fn test_write_ready_returns_true() {
    test_write_ready(true).await;
}

#[tokio::test]
async fn test_write_ready_returns_false() {
    test_write_ready(false).await;
}

#[tokio::test]
async fn test_ping_request() {
    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();
    socket
        .expect_read()
        .once()
        .in_sequence(&mut seq)
        .returning(FrameHeaderBuilder::new(FrameType::Ping).build());

    socket
        .expect_write()
        .once()
        .in_sequence(&mut seq)
        .with(FrameHeaderMatcher::new(FrameType::Pong).build())
        .returning(|buf| Ok(buf.len()));

    //Finalizing the wait_frame call with data frame
    socket
        .expect_read()
        .once()
        .in_sequence(&mut seq)
        .returning(FrameHeaderBuilder::new(FrameType::Binary(false)).build());

    let frame_type = WebSocket::new(socket)
        .wait_message()
        .await
        .expect("No error should occur");

    assert_eq!(frame_type, MessageType::Binary);
}

#[tokio::test]
async fn test_receive_fragmented_message_with_no_payload() {
    let mut seq = Sequence::new();

    let mut socket = MockSocket::new();

    expect_receive_fragmented_message_sequence(&mut socket, &mut seq, &[b""], MessageType::Text, None);

    let mut web_socket = WebSocket::new(socket);

    let frame_type = web_socket.wait_message().await.expect("No error should occur");
    assert_eq!(frame_type, MessageType::Text);

    let mut buf = [0u8; 10];
    assert_eq!(0, web_socket.read(&mut buf).await.expect("No error should occur"));
}

#[tokio::test]
async fn test_receive_fragmented_payload() {
    const FRAME_1_PAYLOAD: &[u8] = b"Hello, ";
    const FRAME_2_PAYLOAD: &[u8] = b"Web";
    const FRAME_3_PAYLOAD: &[u8] = b"Socket!";

    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();

    expect_receive_fragmented_message_sequence(
        &mut socket,
        &mut seq,
        &[FRAME_1_PAYLOAD, FRAME_2_PAYLOAD, FRAME_3_PAYLOAD],
        MessageType::Text,
        None,
    );

    let mut web_socket = WebSocket::new(socket);

    expect_fragmented_message(
        &mut web_socket,
        &[FRAME_1_PAYLOAD, FRAME_2_PAYLOAD, FRAME_3_PAYLOAD],
        MessageType::Text,
    )
    .await;
}

#[tokio::test]
async fn test_receive_fragmented_masked_payload() {
    const MASK_KEY: Option<u32> = Some(0x12345678);
    const FRAME_1_PAYLOAD: &[u8] = b"Hello, ";
    const FRAME_2_PAYLOAD: &[u8] = b"Web";
    const FRAME_3_PAYLOAD: &[u8] = b"Socket!";

    print!("Testing receive fragmented masked payload with mask key: ");
    match MASK_KEY {
        Some(k) => println!("{k}, (0x{k:08X})"),
        None => println!("None"),
    }

    println!("Frame 1 payload: {:?}", FRAME_1_PAYLOAD);
    println!("Frame 2 payload: {:?}", FRAME_2_PAYLOAD);
    println!("Frame 3 payload: {:?}", FRAME_3_PAYLOAD);

    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();

    expect_receive_fragmented_message_sequence(
        &mut socket,
        &mut seq,
        &[FRAME_1_PAYLOAD, FRAME_2_PAYLOAD, FRAME_3_PAYLOAD],
        MessageType::Text,
        MASK_KEY,
    );

    let mut web_socket = WebSocket::new(socket);

    expect_fragmented_message(
        &mut web_socket,
        &[FRAME_1_PAYLOAD, FRAME_2_PAYLOAD, FRAME_3_PAYLOAD],
        MessageType::Text,
    )
    .await;
}

#[tokio::test]
async fn test_receive_several_messages() {
    const MSG_1_FRAME_1: &[u8] = b"Hello, ";
    const MSG_1_FRAME_2: &[u8] = b"WebSocket!";
    const MSG_2_FRAME_1: &[u8] = b"Hello again";

    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();

    // Send first frame of the first message
    expect_receive_fragmented_message_sequence(
        &mut socket,
        &mut seq,
        &[MSG_1_FRAME_1, MSG_1_FRAME_2],
        MessageType::Text,
        None,
    );

    // Send the first and only frame of the second message
    expect_receive_fragmented_message_sequence(&mut socket, &mut seq, &[MSG_2_FRAME_1], MessageType::Text, None);

    let mut web_socket = WebSocket::new(socket);

    // Expect the first message with two frames
    expect_fragmented_message(&mut web_socket, &[MSG_1_FRAME_1, MSG_1_FRAME_2], MessageType::Text).await;

    // Expect the second message with one frame
    expect_fragmented_message(&mut web_socket, &[MSG_2_FRAME_1], MessageType::Text).await;
}

fn expect_close_by_peer_sequence(socket: &mut MockSocket, seq: &mut Sequence) {
    socket
        .expect_read()
        .once()
        .in_sequence(seq)
        .returning(FrameHeaderBuilder::new(FrameType::Close).build());

    socket
        .expect_write()
        .once()
        .in_sequence(seq)
        .with(FrameHeaderMatcher::new(FrameType::Close).build())
        .returning(|buf| Ok(buf.len()));
}

fn expect_close_by_peer_sequence_with_the_close_reason(socket: &mut MockSocket, seq: &mut Sequence) {
    let close_code_bytes = CloseCode::NormalClosure.to_be_bytes();

    socket.expect_read().once().in_sequence(seq).returning(
        FrameHeaderBuilder::new(FrameType::Close)
            .with_payload_len(close_code_bytes.len() as u64)
            .build(),
    );
    socket
        .expect_read()
        .once()
        .in_sequence(seq)
        .returning(MaskedBinaryFrame::new(&close_code_bytes).build());

    socket
        .expect_write()
        .once()
        .in_sequence(seq)
        .with(
            FrameHeaderMatcher::new(FrameType::Close)
                .with_payload_len(close_code_bytes.len() as u64)
                .build(),
        )
        .returning(|buf| Ok(buf.len()));

    socket
        .expect_write()
        .once()
        .in_sequence(seq)
        .with(BinaryMatcher::new(&close_code_bytes).build())
        .returning(|buf| Ok(buf.len()));
}

async fn expect_any_read_write_return_connection_reset(socket: &mut WebSocket<MockSocket>) {
    // Wait for the message should return a connection reset
    let err = socket.wait_message().await.expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    // Read should also return a connection reset
    let err = socket.read(&mut []).await.expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    let err = socket.read_ready().expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    // Write should also return a connection reset
    let err = socket.write(&[]).await.expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    let err = socket.write_ready().expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));
}

#[tokio::test]
async fn test_close_by_peer_request_while_read() {
    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();
    expect_close_by_peer_sequence(&mut socket, &mut seq);

    let mut web_socket = WebSocket::new(socket);

    // First wait for the message, which should be a close frame from the peer
    let err = web_socket
        .wait_message()
        .await
        .expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    expect_any_read_write_return_connection_reset(&mut web_socket).await;
}

#[tokio::test]
async fn test_close_by_peer_request_while_read_with_close_reason() {
    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();
    expect_close_by_peer_sequence_with_the_close_reason(&mut socket, &mut seq);

    let mut web_socket = WebSocket::new(socket);

    // First wait for the message, which should be a close frame from the peer
    let err = web_socket
        .wait_message()
        .await
        .expect_err("Connection should be closed");
    assert!(matches!(err, WebSocketError::ConnectionReset));

    expect_any_read_write_return_connection_reset(&mut web_socket).await;
}

fn expect_local_close_sequence(socket: &mut MockSocket, seq: &mut Sequence) {
    let expected_close_code_bytes = CloseCode::NormalClosure.to_be_bytes();

    socket
        .expect_write()
        .in_sequence(seq)
        .once()
        .with(
            FrameHeaderMatcher::new(FrameType::Close)
                .with_payload_len(expected_close_code_bytes.len() as u64)
                .build(),
        )
        .returning(|buf| Ok(buf.len()));

    socket
        .expect_write()
        .in_sequence(seq)
        .once()
        .with(BinaryMatcher::new(expected_close_code_bytes).build())
        .returning(|buf| Ok(buf.len()));

    socket.expect_flush().in_sequence(seq).once().returning(|| Ok(()));

    // Receive close frame from peer without close reason, which is a valid response to our close frame and should be handled gracefully.
    socket
        .expect_read()
        .in_sequence(seq)
        .once()
        .returning(FrameHeaderBuilder::new(FrameType::Close).build());

    socket
        .expect_close()
        .in_sequence(seq)
        .once()
        .withf(|what| {
            let res = *what == SocketCloseStream::Both;
            if !res {
                println!("Close should be called with Both streams, but got {:?}", what);
            }
            res
        })
        .returning(|_| Ok(()));
}

#[tokio::test]
async fn test_local_close() {
    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();

    expect_local_close_sequence(&mut socket, &mut seq);

    let mut web_socket = WebSocket::new(socket);
    web_socket
        .close(SocketCloseStream::Both)
        .await
        .expect("No error should occur");

    expect_any_read_write_return_connection_reset(&mut web_socket).await;
}

fn expect_local_close_in_case_underlying_socket_error_sequence(socket: &mut MockSocket, seq: &mut Sequence) {
    let expected_close_code_bytes = CloseCode::NormalClosure.to_be_bytes();

    socket
        .expect_write()
        .in_sequence(seq)
        .once()
        .with(
            FrameHeaderMatcher::new(FrameType::Close)
                .with_payload_len(expected_close_code_bytes.len() as u64)
                .build(),
        )
        .returning(|buf| Ok(buf.len()));

    // Emulate socket error on the write
    socket
        .expect_write()
        .in_sequence(seq)
        .once()
        .with(BinaryMatcher::new(expected_close_code_bytes).build())
        .returning(|_| Err(MockError {}));

    socket.expect_abort().in_sequence(seq).once().returning(|| Ok(()));
}

#[tokio::test]
async fn test_local_close_in_case_underlying_socket_error_call_abort() {
    let mut seq = Sequence::new();
    let mut socket = MockSocket::new();

    expect_local_close_in_case_underlying_socket_error_sequence(&mut socket, &mut seq);

    let mut web_socket = WebSocket::new(socket);
    let err = web_socket
        .close(SocketCloseStream::Both)
        .await
        .expect_err("Error should occur");

    assert!(matches!(err, WebSocketError::Socket(MockError {})));

    expect_any_read_write_return_connection_reset(&mut web_socket).await;
}

#[tokio::test]
async fn test_local_abort() {
    let mut socket = MockSocket::new();

    socket.expect_abort().once().returning(|| Ok(()));

    let mut web_socket = WebSocket::new(socket);
    web_socket.abort().await.expect("No error should occur");

    expect_any_read_write_return_connection_reset(&mut web_socket).await;
}

// Additional helpers for testing

fn expect_receive_fragmented_message_sequence(
    socket: &mut MockSocket,
    seq: &mut Sequence,
    fragments: &[&[u8]],
    message_type: MessageType,
    mask_key: Option<u32>,
) {
    assert!(!fragments.is_empty(), "Fragments should not be empty");

    let get_frame_type = |i: usize| {
        if i == 0 {
            message_type.to_frame_type(fragments.len() > 1)
        } else if i == fragments.len() - 1 {
            FrameType::Continue(true)
        } else {
            FrameType::Continue(false)
        }
    };

    for (i, fragment) in fragments.iter().enumerate() {
        FrameHeaderReadSequence::new(get_frame_type(i))
            .with_payload_len(fragment.len() as u64)
            .with_mask_key(mask_key)
            .add_to_test(socket, seq);

        if !fragment.is_empty() {
            socket
                .expect_read()
                .once()
                .in_sequence(seq)
                .returning(MaskedBinaryFrame::new(*fragment).with_mask_key(mask_key).build());
        }
    }
}

async fn expect_fragmented_message(
    web_socket: &mut WebSocket<MockSocket>,
    fragments: &[&[u8]],
    expected_message_type: MessageType,
) {
    assert!(!fragments.is_empty(), "Fragments should not be empty");

    let frame_type = web_socket.wait_message().await.expect("No error should occur");
    assert_eq!(frame_type, expected_message_type);
    assert_eq!(
        web_socket.is_message_pending(),
        true,
        "Message should be pending after any successful wait_message call"
    );

    let has_payload_expected = fragments.iter().any(|fragment| !fragment.is_empty());
    if has_payload_expected {
        // We expect at least one fragment to have a payload
        for (i, fragment) in fragments.iter().enumerate() {
            if fragment.len() > 0 {
                assert_eq!(
                    web_socket.is_message_pending(),
                    true,
                    "Message should be pending if fragment {} has a payload and not read yet",
                    i
                );
                let mut buf = vec![0u8; fragment.len()];
                let read_len = web_socket.read(&mut buf).await.expect("No error should occur");

                assert_eq!(read_len, fragment.len(), "Read length should match fragment length");
                assert_eq!(&buf[..], *fragment, "Read buffer should match fragment");
            }
        }

        assert_eq!(
            web_socket.is_message_pending(),
            false,
            "Message should not be pending after all fragments are read"
        );
    }
}

struct FrameHeaderReadSequence {
    frame_header: FrameHeader,
}

impl FrameHeaderReadSequence {
    pub const fn new(frame_type: FrameType) -> Self {
        Self {
            frame_header: FrameHeader {
                frame_type,
                payload_len: 0,
                mask_key: None,
            },
        }
    }

    pub const fn with_payload_len(mut self, payload_len: u64) -> Self {
        self.frame_header.payload_len = payload_len;
        self
    }

    pub const fn with_mask_key(mut self, mask_key: Option<u32>) -> Self {
        self.frame_header.mask_key = mask_key;
        self
    }

    pub fn add_to_test(self, socket: &mut MockSocket, seq: &mut Sequence) {
        let read_count = estimate_read_count(&self.frame_header);
        socket.expect_read().times(read_count).in_sequence(seq).returning(
            FrameHeaderBuilder::new(self.frame_header.frame_type)
                .with_payload_len(self.frame_header.payload_len)
                .with_mask_key(self.frame_header.mask_key)
                .build(),
        );
    }
}
