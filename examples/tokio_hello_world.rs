use nanooctopus::{
    Error, HttpAllocator, HttpHandler, HttpRequest, HttpResponseBuilder, HttpServer, HttpSocketWrite, ServerTimeouts,
    SocketEndpoint, StatusCode, TokioTcpSocketBuilder, response::HttpResponse, worker_memory::HttpWorkerMemory,
};

struct HelloWorldHandler;

impl HttpHandler for HelloWorldHandler {
    async fn handle_request(
        &mut self,
        _allocator: &mut HttpAllocator<'_>,
        request: &HttpRequest<'_>,
        http_socket: &mut impl HttpSocketWrite,
        context_id: usize,
    ) -> Result<HttpResponse, Error> {
        log::info!("Received request: {:?} in context {}", request, context_id);

        let response = HttpResponseBuilder::new(http_socket);
        response
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

    tokio::task::spawn_local(async move {
        let socket_builder =
            TokioTcpSocketBuilder::new(SocketEndpoint::new([127, 0, 0, 1].try_into().unwrap(), 8080)).await;

        let server = HttpServer::new(&socket_builder, ServerTimeouts::default());

        server
            .serve(HttpWorkerMemory::<1024>::new(), HelloWorldHandler, 1)
            .await
    })
    .await
    .unwrap();
}
