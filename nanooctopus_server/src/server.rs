#![allow(async_fn_in_trait)]

use crate::socket::*;

pub use edge_http::Method;
pub use edge_http::io::Error as IoError;
pub use edge_http::io::server::Handler;
use edge_http::io::server::Server as EdgeServer;

pub use edge_http::DEFAULT_MAX_HEADERS_COUNT;
pub use edge_http::io::server::{DEFAULT_BUF_SIZE, DEFAULT_HANDLER_TASKS_COUNT};

/// Configuration options for the HTTP server.
/// This struct can be extended in the future to include more settings.
pub struct Config {
    /// Optional keepalive timeout in milliseconds.
    /// If set, the server will automatically close idle connections that have been inactive for longer than this duration.
    pub keepalive_timeout_ms: Option<u32>,
}

/// A simple HTTP server implementation that can handle incoming TCP connections and process HTTP requests using a user-defined handler.
pub type DefaultServer = Server<{ DEFAULT_HANDLER_TASKS_COUNT }, { DEFAULT_BUF_SIZE }, { DEFAULT_MAX_HEADERS_COUNT }>;

/// TODO: Add more configuration options to the server, such as max concurrent connections, request timeout, etc.
#[derive(Default)]
pub struct Server<
    const TASK: usize = DEFAULT_HANDLER_TASKS_COUNT,
    const BUFFER_SIZE: usize = DEFAULT_BUF_SIZE,
    const HEADERS: usize = DEFAULT_MAX_HEADERS_COUNT,
> {
    inner: EdgeServer<{ TASK }, { BUFFER_SIZE }, { HEADERS }>,
}

impl<const TASK: usize, const BUFFER_SIZE: usize, const HEADERS: usize> Server<{ TASK }, { BUFFER_SIZE }, { HEADERS }> {
    /// Create a new HTTP server instance with the specified configuration.
    pub const fn new() -> Self {
        Self {
            inner: EdgeServer::new(),
        }
    }

    /// Run the HTTP server with the given configuration and TCP acceptor. This method will block indefinitely, handling incoming connections
    /// and processing HTTP requests using the provided handler. The server will automatically manage a pool of tasks to handle multiple
    /// concurrent connections efficiently.
    ///
    /// ### Parameters:
    /// - `config`: The configuration for the server, including keepalive timeout and other settings.
    /// - `acceptor`: The TCP acceptor that listens for incoming connections and provides the sockets to the server.
    ///
    /// ### Returns:
    /// - `Ok(())` if the server runs successfully without errors.
    /// - `Err(Error<A::Error>)` if there is an error during server operation, such as issues with accepting connections or handling requests.
    pub async fn run<A, H, const Q: usize>(
        &mut self,
        config: Config,
        acceptor: A,
        handler: H,
    ) -> Result<(), IoError<A::Error>>
    where
        A: SocketAccept,
        H: Handler,
    {
        self.inner
            .run_with_socket_queue::<_, _, Q>(config.keepalive_timeout_ms, acceptor, handler)
            .await?;
        Ok(())
    }
}
