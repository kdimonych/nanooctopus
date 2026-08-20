use crate::socket::{SocketRead, SocketSplit, SocketWrite};
pub use edge_http::io::server::Connection;
pub use edge_http::{Headers, Method};
pub use prefix_arena::PrefixArena;

use core::fmt::{Debug, Display};

/// Represent HTTP endpoint handler
pub trait EndpointHandler {
    /// Error type
    type Error<E>: Debug
    where
        E: Debug;

    /// Returns methods that are supported by this endpoint handler
    fn supported_methods(&self) -> &'static [Method];

    /// Handle an incoming HTTP request
    ///
    /// Parameters:
    /// - `task_id`: An identifier for the task, that can be used by the handler for logging purposes
    /// - `connection`: A connection state machine for the request-response cycle
    async fn handle<T, const N: usize>(
        &self,
        ctx: impl Copy,
        task_id: impl Display + Copy,
        connection: &mut Connection<'_, T, N>,
        allocator: PrefixArena<'_>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: SocketRead + SocketWrite + SocketSplit;
}

impl<H> EndpointHandler for &H
where
    H: EndpointHandler,
{
    type Error<E>
        = H::Error<E>
    where
        E: Debug;

    fn supported_methods(&self) -> &'static [Method] {
        (**self).supported_methods()
    }

    async fn handle<T, const N: usize>(
        &self,
        ctx: impl Copy,
        task_id: impl Display + Copy,
        connection: &mut Connection<'_, T, N>,
        allocator: PrefixArena<'_>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: SocketRead + SocketWrite + SocketSplit,
    {
        (**self).handle(ctx, task_id, connection, allocator).await
    }
}

impl<H> EndpointHandler for &mut H
where
    H: EndpointHandler,
{
    type Error<E>
        = H::Error<E>
    where
        E: Debug;

    fn supported_methods(&self) -> &'static [Method] {
        (**self).supported_methods()
    }

    async fn handle<T, const N: usize>(
        &self,
        ctx: impl Copy,
        task_id: impl Display + Copy,
        connection: &mut Connection<'_, T, N>,
        allocator: PrefixArena<'_>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: SocketRead + SocketWrite + SocketSplit,
    {
        (**self).handle(ctx, task_id, connection, allocator).await
    }
}
