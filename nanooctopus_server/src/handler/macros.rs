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


        impl Handler for MapHandlerImpl {
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
                match h.path {
                    $( $key => {self.$name.handle(task_id, conn).await?;} ),+,
                    _ => {
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
}
