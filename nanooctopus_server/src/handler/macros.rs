/// This macro generates a handler that maps specific request paths to corresponding handler implementations.
/// It allows for easy routing of HTTP requests based on the request path, enabling developers to define custom behavior for different endpoints in a
/// concise manner.
///
/// ### Usage:
/// ```rust, ignore
/// use nanooctopus_server::socket::*;
/// use nanooctopus_server::*;
///
/// let handler = map_handler!(
///     ("/", root: MainHandler = MainHandler {}),
///     ("/hello", hello: HelloHandler = HelloHandler {})
/// );
/// ```
///
/// ### Note:
/// - The macro generates a struct that implements the `Handler` trait, which can be used with the HTTP server to handle incoming requests.
/// - Each tuple in the macro has the format: (<path>, <handler_name>: <handler_type> = <handler_instance>)
/// - Each handler instance must implement the `Handler` trait as well.
#[macro_export]
macro_rules! map_handler {
    ($( ($key:literal, $name:ident : $ty:ty = $h:expr) ),+) => {{
        struct MapHandlerImpl{
            $( $name : $ty ),+
        }

        impl Handler for MapHandlerImpl{
            type Error<E>
                = HandlerError<E>
            where
                E: Debug;

            async fn handle<S, const CN: usize>(
                &self,
                task_id: impl Display + Copy,
                conn: &mut Connection<'_, S, CN>,
            ) -> Result<(), Self::Error<S::Error>>
            where
                S: SocketRead + SocketWrite + SocketSplit,
            {
                let h = conn.headers()?;
                let mut mem_buf = [const { core::mem::MaybeUninit::uninit() }; nanooctopus_server::DEFAULT_HEADLER_BUFFER];
                let mut arena = PrefixArena::from_uninit(&mut mem_buf);

                match h.path {
                    $( $key => {
                        if self.$name.supported_methods().contains(&h.method) {
                            nanooctopus_server::log::debug!("Handling request for path: {}", $key);
                            self.$name.handle((), task_id, conn, arena.reborrow()).await?;
                        } else {
                            nanooctopus_server::log::debug!("Method not allowed for path: {}", $key);
                            conn.initiate_response(405, Some("Method Not Allowed"), &[]).await?;
                            conn.write_all(b"Method Not Allowed").await?;
                            conn.flush().await?;
                            conn.complete().await?;
                        }
                    } ),+,
                    _ => {
                        nanooctopus_server::log::debug!("Request path not found: {}", h.path);
                        conn.initiate_response(404, Some("Not Found"), &[]).await?;
                        conn.write_all(b"Not Found").await?;
                        conn.flush().await?;
                        conn.complete().await?;
                    }
                }
                Ok(())
            }
        }

        MapHandlerImpl{
            $( $name: $h ),+
        }
    }};

    ($buf_size:expr, $ctx:expr, $( ($key:literal, $name:ident : $ty:ty = $h:expr) ),+) => {{
        struct MapHandlerImpl<Context>{
            ctx: Context,
            $( $name : $ty ),+
        }

        impl<Context: Copy> Handler for MapHandlerImpl<Context> {
            type Error<E>
                = HandlerError<E>
            where
                E: Debug;

            async fn handle<S, const CN: usize>(
                &self,
                task_id: impl Display + Copy,
                conn: &mut Connection<'_, S, CN>,
            ) -> Result<(), Self::Error<S::Error>>
            where
                S: SocketRead + SocketWrite + SocketSplit,
            {
                let h = conn.headers()?;
                let mut mem_buf = [const { core::mem::MaybeUninit::uninit() }; $buf_size];
                let mut arena = PrefixArena::from_uninit(&mut mem_buf);

                match h.path {
                    $( $key => {
                        if self.$name.supported_methods().contains(&h.method) {
                            nanooctopus_server::log::debug!("Handling request for path: {}", $key);
                            self.$name.handle((), task_id, conn, arena.reborrow()).await?;
                        } else {
                            nanooctopus_server::log::debug!("Method not allowed for path: {}", $key);
                            conn.initiate_response(405, Some("Method Not Allowed"), &[]).await?;
                            conn.write_all(b"Method Not Allowed").await?;
                            conn.flush().await?;
                            conn.complete().await?;
                        }
                    } ),+,
                    _ => {
                        nanooctopus_server::log::debug!("Request path not found: {}", h.path);
                        conn.initiate_response(404, Some("Not Found"), &[]).await?;
                        conn.write_all(b"Not Found").await?;
                        conn.flush().await?;
                        conn.complete().await?;
                    }
                }
                Ok(())
            }
        }

        MapHandlerImpl{
            ctx: $ctx,
            $( $name: $h ),+
        }
    }};
}
