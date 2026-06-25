#![doc = include_str!("../README.md")]

use edge_nal::TcpBind;
use nanooctopus_server::socket::*;
use nanooctopus_server::*;
use std::fmt::{Debug, Display};

struct RootHandler;
impl Handler for RootHandler {
    type Error<E>
        = IoError<E>
    where
        E: Debug;

    async fn handle<S, const CN: usize>(
        &self,
        _task_id: impl Display + Copy,
        conn: &mut Connection<'_, S, CN>,
    ) -> Result<(), Self::Error<S::Error>>
    where
        S: SocketRead + SocketWrite + SocketSplit,
    {
        conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
            .await?;

        conn.write_all(b"Generic: Hello world!").await?;
        conn.flush().await?;
        conn.complete().await?;

        Ok(())
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
        let mut server = DefaultServer::new();
        let config = Config {
            keepalive_timeout_ms: None,
        };

        let acceptor = edge_nal_std::Stack::new()
            .bind("127.0.0.1:8080".parse().unwrap())
            .await
            .unwrap();

        let h = map_handler!(
            ("/", root: RootHandler = RootHandler {}),
            ("/favicon.ico", fav: FaviconHandler<'static> = FaviconHandler::new(include_bytes!("favicon.ico")))
        );

        server.run::<_, _, 8>(config, acceptor, h).await.unwrap();
    })
    .await
    .unwrap();
}
