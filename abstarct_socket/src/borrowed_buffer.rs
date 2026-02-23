#[derive(Debug)]
pub struct BorrowedBuffer<'a> {
    inner: &'a mut [u8],
    len: usize,
}

impl<'a> BorrowedBuffer<'a> {
    /// Create a new BorrowedBuffer wrapping the given mutable slice.
    pub const fn new(buffer: &'a mut [u8]) -> Self {
        Self { inner: buffer, len: 0 }
    }

    const fn from_parts(buffer: &'a mut [u8], len: usize) -> Self {
        Self { inner: buffer, len }
    }

    /// Push a byte into the buffer, returning an error if there is not enough capacity.
    ///
    /// Returns Ok(()) on success, Err(()) if there is not enough capacity.
    pub fn push(&mut self, byte: u8) -> Result<(), ()> {
        if self.len >= self.inner.len() {
            return Err(());
        }
        self.inner[self.len] = byte;
        self.len += 1;
        Ok(())
    }

    /// Extend the buffer from a slice, returning an error if there is not enough capacity.
    ///
    /// Returns Ok(()) on success, Err(()) if there is not enough capacity.
    pub fn extend_from_slice(&mut self, slice: &[u8]) -> Result<(), ()> {
        if self.len + slice.len() > self.inner.len() {
            return Err(());
        }
        self.inner[self.len..self.len + slice.len()].copy_from_slice(slice);
        self.len += slice.len();
        Ok(())
    }

    /// Append the data from a slice, returning the number of bytes actually appended.
    pub fn append_from_slice(&mut self, slice: &[u8]) -> usize {
        let to_fill = core::cmp::min(self.remaining_capacity(), slice.len());
        self.inner[self.len..self.len + to_fill].copy_from_slice(&slice[..to_fill]);
        self.len += to_fill;
        return to_fill;
    }

    /// Split the buffer into used and remaining parts, returning two BorrowedBuffers
    /// for each part.
    pub fn take_splited_mut(self) -> (BorrowedBuffer<'a>, BorrowedBuffer<'a>) {
        let (used, remaining) = self.inner.split_at_mut(self.len);
        (
            BorrowedBuffer::from_parts(used, used.len()),
            BorrowedBuffer::from_parts(remaining, 0),
        )
    }

    /// Get the used portion as a slice
    pub fn as_slice(&'a self) -> &'a [u8] {
        &self.inner[..self.len]
    }

    /// Get the used portion as a mutable slice
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.inner[..self.len]
    }

    /// Get the remaining portion as a slice
    pub fn as_remaining_slice(&self) -> &[u8] {
        &self.inner[self.len..]
    }

    /// Get the remaining portion as a mutable slice
    pub fn as_mut_remaining_slice(&mut self) -> &mut [u8] {
        &mut self.inner[self.len..]
    }

    /// Get the length of the whole anderling buffer
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Get the total capacity of the buffer
    pub const fn capacity(&self) -> usize {
        self.inner.len()
    }

    pub const fn remaining_capacity(&self) -> usize {
        self.inner.len() - self.len
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Take the used portion as a mutable slice, consuming self
    pub fn take_used(self) -> &'a mut [u8] {
        &mut self.inner[..self.len]
    }

    // Split off the used portion and return the rest
    pub fn split_off_remaining(self) -> &'a mut [u8] {
        &mut self.inner[self.len..]
    }

    /// Consume bytes as used, returning an error if it exceeds capacity
    pub fn consume(&mut self, used: usize) -> Result<(), ()> {
        if self.len + used > self.inner.len() {
            return Err(());
        }
        self.len += used;
        Ok(())
    }

    /// Unsafely consume bytes as used without checking capacity
    pub unsafe fn consume_unchecked(&mut self, used: usize) {
        debug_assert!(used + self.len <= self.inner.len());
        self.len += used;
    }

    // Get the full buffer back
    pub fn into_inner(self) -> (&'a [u8], &'a mut [u8]) {
        let (used, remaining) = self.inner.split_at_mut(self.len);
        (used, remaining)
    }
}

impl<'a> From<&'a mut [u8]> for BorrowedBuffer<'a> {
    fn from(buffer: &'a mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'a, const N: usize> From<&'a mut [u8; N]> for BorrowedBuffer<'a> {
    fn from(buffer: &'a mut [u8; N]) -> Self {
        Self::new(buffer)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_basic_push() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 5);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);

        borrowed_buffer.push(42).unwrap();
        assert_eq!(borrowed_buffer.len(), 1);
        assert_eq!(borrowed_buffer.as_slice(), &[42]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 4);
    }

    #[test]
    fn test_push_until_full() {
        let mut buffer = [0u8; 3];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.push(1).unwrap();
        borrowed_buffer.push(2).unwrap();
        borrowed_buffer.push(3).unwrap();

        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
        assert!(borrowed_buffer.push(4).is_err());
    }

    #[test]
    fn test_clear() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();

        let mut_slice = borrowed_buffer.as_mut_slice();
        mut_slice[1] = 99;

        assert_eq!(borrowed_buffer.as_slice(), &[1, 99, 3]);
    }

    #[test]
    fn test_empty_buffer() {
        let mut buffer = [0u8; 0];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 0);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
        assert!(borrowed_buffer.push(1).is_err());
        assert!(borrowed_buffer.extend_from_slice(&[1]).is_err());
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_as_remaining_slice() {
        let mut buffer = [0u8; 8];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();

        let remaining = borrowed_buffer.as_remaining_slice();
        assert_eq!(remaining.len(), 5);

        let mut_remaining = borrowed_buffer.as_mut_remaining_slice();
        assert_eq!(mut_remaining.len(), 5);
        mut_remaining[0] = 42;

        assert_eq!(borrowed_buffer.as_remaining_slice()[0], 42);
    }

    #[test]
    fn test_from_parts_internal() {
        let mut buffer = [1, 2, 3, 4, 5];
        let borrowed_buffer = BorrowedBuffer::from_parts(&mut buffer, 3);

        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.capacity(), 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_take_splited_mut_empty() {
        let mut buffer = [0u8; 5];
        let borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        let (used, remaining) = borrowed_buffer.take_splited_mut();

        assert_eq!(used.len(), 0);
        assert_eq!(used.capacity(), 0);
        assert_eq!(remaining.len(), 0);
        assert_eq!(remaining.capacity(), 5);
    }

    #[test]
    fn test_take_splited_mut_full() {
        let mut buffer = [0u8; 3];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        let (used, remaining) = borrowed_buffer.take_splited_mut();

        assert_eq!(used.len(), 3);
        assert_eq!(used.capacity(), 3);
        assert_eq!(used.as_slice(), &[1, 2, 3]);
        assert_eq!(remaining.len(), 0);
        assert_eq!(remaining.capacity(), 0);
    }

    #[test]
    fn test_single_byte_buffer() {
        let mut buffer = [0u8; 1];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert_eq!(borrowed_buffer.capacity(), 1);
        assert_eq!(borrowed_buffer.remaining_capacity(), 1);

        borrowed_buffer.push(42).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[42]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        assert!(borrowed_buffer.push(1).is_err());
        assert!(borrowed_buffer.extend_from_slice(&[1]).is_err());
    }

    #[test]
    fn test_clear_and_reuse() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        borrowed_buffer.extend_from_slice(&[4, 5, 6, 7, 8]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[4, 5, 6, 7, 8]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
    }

    #[test]
    fn test_extend_at_exact_capacity() {
        let mut buffer = [0u8; 3];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert!(borrowed_buffer.extend_from_slice(&[1, 2, 3]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        assert!(borrowed_buffer.extend_from_slice(&[]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
    }

    #[test]
    fn test_take_used() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[10, 20, 30]).unwrap();
        let used = borrowed_buffer.take_used();

        assert_eq!(used, &[10, 20, 30]);
        used[0] = 255;
        assert_eq!(used, &[255, 20, 30]);
    }

    #[test]
    fn test_split_off_remaining() {
        let mut buffer = [0u8; 8];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        let remaining = borrowed_buffer.split_off_remaining();

        assert_eq!(remaining.len(), 5);
        remaining[0] = 42;
        assert_eq!(remaining[0], 42);
    }

    #[test]
    fn test_into_inner() {
        let mut buffer = [0u8; 6];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3, 4]).unwrap();
        let (used, remaining) = borrowed_buffer.into_inner();

        assert_eq!(used, &[1, 2, 3, 4]);
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn test_extend_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[]).unwrap();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_capacity_overflow() {
        let mut buffer = [0u8; 2];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert!(borrowed_buffer.extend_from_slice(&[1, 2, 3]).is_err());
        assert_eq!(borrowed_buffer.len(), 0);

        borrowed_buffer.extend_from_slice(&[1, 2]).unwrap();
        assert!(borrowed_buffer.push(3).is_err());
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2]);
    }

    #[test]
    fn test_multiple_extension() {
        let mut buffer = [0u8; 10];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);

        borrowed_buffer.extend_from_slice(&[4, 5]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);

        borrowed_buffer.extend_from_slice(&[6, 7, 8, 9, 10]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        assert!(borrowed_buffer.extend_from_slice(&[11]).is_err());
    }

    #[test]
    fn test_take_remaining_mut() {
        let mut buffer = [0u8; 10];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        let (used, mut remaining) = borrowed_buffer.take_splited_mut();
        let s123 = used.as_slice();
        assert_eq!(used.as_slice(), &[1, 2, 3]);

        remaining.extend_from_slice(&[4, 5]).unwrap();
        let (used, mut remaining) = remaining.take_splited_mut();
        let s45 = used.as_slice();
        assert_eq!(used.as_slice(), &[4, 5]);

        remaining.extend_from_slice(&[6, 7, 8, 9, 10]).unwrap();
        let (used, mut remaining) = remaining.take_splited_mut();
        let s678910 = used.as_slice();
        assert_eq!(used.as_slice(), &[6, 7, 8, 9, 10]);

        assert!(remaining.extend_from_slice(&[11]).is_err());
        assert_eq!(s123, &[1, 2, 3]);
        assert_eq!(s45, &[4, 5]);
        assert_eq!(s678910, &[6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_take_remaining_mut_in_loop() {
        let mut buffer = [0u8; 10];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        let mut borrowed = Vec::new();

        for i in 0..3 {
            borrowed_buffer.extend_from_slice(&[i]).unwrap();
            let (used, remaining) = borrowed_buffer.take_splited_mut();
            borrowed_buffer = remaining;

            borrowed.push(used.take_used());
        }

        assert_eq!(borrowed[0], &[0]);
        assert_eq!(borrowed[1], &[1]);
        assert_eq!(borrowed[2], &[2]);
    }

    #[test]
    fn test_append_from_slice() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        // Append with data larger than remaining capacity
        let filled = borrowed_buffer.append_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        // Try to append when buffer is full
        let filled = borrowed_buffer.append_from_slice(&[8, 9]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_append_from_slice_partial() {
        let mut buffer = [0u8; 10];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        // Add some data first
        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.remaining_capacity(), 7);

        // Append with smaller slice
        let filled = borrowed_buffer.append_from_slice(&[4, 5]);
        assert_eq!(filled, 2);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);

        // Append remaining with exact size
        let filled = borrowed_buffer.append_from_slice(&[6, 7, 8, 9, 10]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
    }

    #[test]
    fn test_consume() {
        let mut buffer = [1, 2, 3, 4, 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        assert_eq!(borrowed_buffer.len(), 0);

        // Consume some bytes
        borrowed_buffer.consume(3).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 2);

        // Consume remaining bytes
        borrowed_buffer.consume(2).unwrap();
        assert_eq!(borrowed_buffer.len(), 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        // Try to consume beyond capacity
        assert!(borrowed_buffer.consume(1).is_err());
        assert_eq!(borrowed_buffer.len(), 5); // Should remain unchanged
    }

    #[test]
    fn test_consume_zero() {
        let mut buffer = [1, 2, 3];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        borrowed_buffer.consume(0).unwrap();
        assert_eq!(borrowed_buffer.len(), 0);
    }

    #[test]
    fn test_consume_unchecked() {
        let mut buffer = [1, 2, 3, 4, 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        unsafe {
            borrowed_buffer.consume_unchecked(3);
        }
        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);

        unsafe {
            borrowed_buffer.consume_unchecked(2);
        }
        assert_eq!(borrowed_buffer.len(), 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_from_trait() {
        let mut buffer = [1, 2, 3, 4, 5];
        let borrowed_buffer: BorrowedBuffer = (&mut buffer).into();

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 5);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);
    }

    #[test]
    fn test_from_trait_usage() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::from(&mut buffer);

        borrowed_buffer.extend_from_slice(&[10, 20, 30]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[10, 20, 30]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 2);
    }

    #[test]
    fn test_fill_remaining_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        let filled = borrowed_buffer.append_from_slice(&[]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.len(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic]
    fn test_debug_assertions_consume_unchecked() {
        // This test would panic in debug builds if consume_unchecked goes beyond capacity
        let mut buffer = [1, 2, 3];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);

        unsafe {
            borrowed_buffer.consume_unchecked(3); // Should be fine
        }
        assert_eq!(borrowed_buffer.len(), 3);

        // This would panic in debug due to debug_assert!
        unsafe {
            borrowed_buffer.consume_unchecked(1); // Would exceed capacity
        }
    }

    #[test]
    fn test_as_slice_lifetime() {
        let mut buffer = [1, 2, 3, 4, 5];
        let mut borrowed_buffer = BorrowedBuffer::new(&mut buffer);
        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();

        let slice1 = borrowed_buffer.as_slice();
        let slice2 = borrowed_buffer.as_slice();

        assert_eq!(slice1, slice2);
        assert_eq!(slice1, &[1, 2, 3]);
    }
}
