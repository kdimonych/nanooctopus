# abstarct_socket

`abstarct_socket` is a thin TCP socket abstraction layer for `nanofish`.

The crate exists to let the web server work against one socket-oriented contract while keeping the underlying transport as transparent as possible. The abstraction is intentionally small:

- common TCP metadata via `SocketInfo`
- connection lifecycle via `SocketConnect`, `SocketAccept`, and `SocketClose`
- async I/O via `embedded_io_async::Read` and `embedded_io_async::Write`
- optional direct-buffer hooks via `ReadWith` and `WriteWith`

The main target is parity between Embassy TCP sockets and the web server implementation. Tokio support is provided as an adapter so the same higher-level code can be tested and exercised on a host machine.

## Design goals

- Keep the abstraction close to the native TCP socket model.
- Reuse `embedded-io-async` traits instead of inventing another I/O API.
- Preserve platform-specific behavior when possible instead of hiding it behind a large wrapper.
- Support no-std Embassy targets and host-side Tokio execution from the same crate.

## Main pieces

- `Socket`: the common TCP contract used by the server.
- `SocketExtended`: `Socket` plus `read_with` and `write_with` for custom buffer handling.
- `AbstractSocketBuilder`: a tiny builder trait used to construct implementation-specific sockets.
- `StreamSearch`: helpers for reading until a byte or sequence without duplicating parsing logic in the server.

## Feature flags

- `embassy_impl`: adapters for `embassy_net::tcp::TcpSocket`, `TcpReader`, and `TcpWriter`
- `tokio_impl`: host-side wrappers around Tokio TCP types
- `mocks`: mock streams and sockets used in tests and examples
- `std`: enables standard-library support; tests also enable it automatically

## Example: parse until the HTTP header boundary

This example uses the testing mock, but the same `StreamSearch` code works with any type that implements `ReadWith`.

```rust
use abstarct_socket::mocks::mock_read_stream::MockReadStream;
use abstarct_socket::stream_search::StreamSearch;
use prefix_arena::PrefixArena;

# tokio_test::block_on(async {
let mut input = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\nbody".to_vec();
let mut stream = MockReadStream::new(&mut input);

let mut arena_storage = [0u8; 128];
let mut arena = PrefixArena::new(&mut arena_storage);

let header = stream
	.seek_until_sequence(b"\r\n\r\n", &mut arena)
	.await
	.unwrap();

assert_eq!(header, b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n");
# });
```

## Example: build a Tokio-backed socket

`TokioTcpSocketBuilder` gives the server a socket-shaped object that still behaves like Tokio underneath.

```no_run
use abstarct_socket::socket::{AbstractSocketBuilder, SocketConnect, SocketInfo};
use abstarct_socket::tokio_impl::socket::{IpVersion, TokioTcpSocketBuilder};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let builder = TokioTcpSocketBuilder::new(IpVersion::V4);
	let mut socket = builder.build();

	socket.connect(([127, 0, 0, 1], 8080)).await.unwrap();

	assert!(socket.local_endpoint().is_some());
	assert!(socket.remote_endpoint().is_some());
}
```

## Example: build an Embassy socket

Embassy keeps ownership of the network stack and the I/O buffers. The builder just packages those dependencies into a socket that matches the crate traits.

```ignore
use abstarct_socket::embassy_impl::socket::EmbassyTcpSocketBuilder;
use abstarct_socket::socket::AbstractSocketBuilder;

let mut rx_buffer = [0u8; 1024];
let mut tx_buffer = [0u8; 1024];

let builder = EmbassyTcpSocketBuilder::new(stack, &mut rx_buffer, &mut tx_buffer);
let socket = builder.build();
```

## When to use `read_with` and `write_with`

Use plain `Read` and `Write` when copied buffers are fine.

Use `read_with` and `write_with` when the parser or serializer wants to work directly with the adapter's currently available chunk. That keeps the abstraction thin and lets adapters preserve implementation-specific buffering behavior.
