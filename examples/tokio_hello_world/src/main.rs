use nanooctopus::{
    Error, HttpAllocator, HttpHandler, HttpRequest, HttpResponseBuilder, HttpServer, HttpSocketWrite, ServerTimeouts,
    SocketEndpoint, StatusCode, TokioTcpListener, response::HttpResponse, worker_memory::HttpWorkerMemory,
};

/// Request handler that responds to every HTTP request with "Hello, World!".
struct HelloWorldHandler;

impl HttpHandler for HelloWorldHandler {
    async fn handle_request(
        &mut self,
        _allocator: &mut HttpAllocator<'_>, // unused in this simple handler
        request: &HttpRequest<'_>,
        http_socket: &mut impl HttpSocketWrite,
        context_id: usize,
    ) -> Result<HttpResponse, Error> {
        log::info!("Received request: {:?} in context {}", request, context_id);

        // Stream the response directly to the socket: status → headers → body.
        HttpResponseBuilder::new(http_socket)
            .with_status(StatusCode::Ok)
            .await?
            .with_header("Content-Type", "text/plain")
            .await?
            .with_body_from_slice(b"Hello, World!")
            .await
    }
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
}

#[tokio::main(flavor = "local")]
async fn main() {
    init_logging();

    // `spawn_local` keeps everything on the current thread, matching the
    // single-threaded (`flavor = "local"`) Tokio runtime used here.
    tokio::task::spawn_local(async move {
        // Bind the TCP listener to localhost:8080.
        let listener = TokioTcpListener::new(SocketEndpoint::new([127, 0, 0, 1].into(), 8080)).await;

        let server = HttpServer::new(&listener, ServerTimeouts::default());

        // 1024-byte scratch buffer for parsing incoming HTTP headers.
        // A single worker (context_id = 1) handles requests sequentially.
        server
            .serve(HttpWorkerMemory::<1024>::new(), HelloWorldHandler, 1)
            .await
    })
    .await
    .unwrap();
}
