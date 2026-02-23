/// Trait representing a read stream interface
pub trait ReadWith: embedded_io_async::ErrorType {
    /// Read from the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes read and a result value.
    ///
    /// ## Returns
    /// - Returns Ok(R) where R is the result returned by the function `f`.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while reading from the stream.
    ///
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R);
}

/// Implement ReadWith for mutable references to types that implement ReadWith
impl<T: ?Sized + ReadWith> ReadWith for &mut T {
    #[inline]
    fn read_with<F, R>(&mut self, f: F) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R),
    {
        T::read_with(self, f)
    }
}
