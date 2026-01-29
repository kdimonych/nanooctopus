/// Trait representing a write stream interface
pub trait WriteWith: embedded_io_async::ErrorType {
    /// Write to the stream using the provided function
    ///
    /// The function `f` is called with a slice of available data from the stream.
    /// It should return a tuple containing the number of bytes written and a result value.
    ///
    /// ## Returns
    /// - Returns Ok(R) where R is the result returned by the function `f`.
    ///
    /// ## Errors
    /// - Returns `Self::Error` if an error occurs while writing to the stream.
    ///
    fn write_with<F, R>(
        &mut self,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Self::Error>>
    where
        F: FnOnce(&mut [u8]) -> (usize, R);
}
