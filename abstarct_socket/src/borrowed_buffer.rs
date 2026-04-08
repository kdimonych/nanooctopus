use crate::head_arena::{HeadArena, TempBuffer};

pub struct BorrowedBuffer<'arena, 'b> {
    inner: TempBuffer<'arena, 'b>,
    used: usize,
}

impl<'arena, 'b> BorrowedBuffer<'arena, 'b>
where
    'b: 'arena,
{
    /// Create a new BorrowedBuffer wrapping the given mutable slice.
    pub const fn new(allocator: &'arena mut HeadArena<'b>) -> Self {
        Self {
            inner: allocator.temporary(),
            used: 0,
        }
    }

    /// Push a byte into the buffer, returning an error if there is not enough capacity.
    ///
    /// Returns Ok(()) on success, Err(()) if there is not enough capacity.
    pub fn push(&mut self, byte: u8) -> Result<(), ()> {
        if self.used >= self.inner.len() {
            return Err(());
        }
        self.inner.as_slice_mut()[self.used].write(byte);
        self.used += 1;
        Ok(())
    }

    /// Extend the buffer from a slice, returning an error if there is not enough capacity.
    ///
    /// Returns Ok(()) on success, Err(()) if there is not enough capacity.
    pub fn append_from_slice(&mut self, slice: &[u8]) -> Result<(), ()> {
        if self.used + slice.len() > self.inner.len() {
            return Err(());
        }
        self.inner.as_slice_mut()[self.used..self.used + slice.len()]
            .copy_from_slice(unsafe { core::mem::transmute(slice) });
        self.used += slice.len();
        Ok(())
    }

    /// Append the data from a slice, returning the number of bytes actually appended.
    pub fn try_append_from_slice(&mut self, slice: &[u8]) -> usize {
        let to_fill = core::cmp::min(self.remaining_capacity(), slice.len());
        self.inner.as_slice_mut()[self.used..self.used + to_fill]
            .copy_from_slice(unsafe { core::mem::transmute(&slice[..to_fill]) });
        self.used += to_fill;
        return to_fill;
    }

    /// Get the used portion as a slice
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::mem::transmute(&self.inner.as_slice()[..self.used]) }
    }

    /// Get the used portion as a mutable slice
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::mem::transmute(&mut self.inner.as_slice_mut()[..self.used]) }
    }

    /// Get the length of the whole anderling buffer
    pub const fn len(&self) -> usize {
        self.used
    }

    /// Get the total capacity of the buffer
    pub const fn capacity(&self) -> usize {
        self.inner.len()
    }

    pub const fn remaining_capacity(&self) -> usize {
        self.inner.len() - self.used
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.used = 0;
    }

    /// Take the used portion as a mutable slice, consuming self
    pub fn take_used(self) -> &'b mut [u8] {
        unsafe { core::mem::transmute(self.inner.accuire_front_mut(self.used)) }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_basic_push() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

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
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

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
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();

        let mut_slice = borrowed_buffer.as_mut_slice();
        mut_slice[1] = 99;

        assert_eq!(borrowed_buffer.as_slice(), &[1, 99, 3]);
    }

    #[test]
    fn test_empty_buffer() {
        let mut buffer = [0u8; 0];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 0);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
        assert!(borrowed_buffer.push(1).is_err());
        assert!(borrowed_buffer.append_from_slice(&[1]).is_err());
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_single_byte_buffer() {
        let mut buffer = [0u8; 1];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        assert_eq!(borrowed_buffer.capacity(), 1);
        assert_eq!(borrowed_buffer.remaining_capacity(), 1);

        borrowed_buffer.push(42).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[42]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        assert!(borrowed_buffer.push(1).is_err());
        assert!(borrowed_buffer.append_from_slice(&[1]).is_err());
    }

    #[test]
    fn test_clear_and_reuse() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        borrowed_buffer.append_from_slice(&[4, 5, 6, 7, 8]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[4, 5, 6, 7, 8]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
    }

    #[test]
    fn test_extend_at_exact_capacity() {
        let mut buffer = [0u8; 3];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        assert!(borrowed_buffer.append_from_slice(&[1, 2, 3]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        assert!(borrowed_buffer.append_from_slice(&[]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
    }

    #[test]
    fn test_take_used() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[10, 20, 30]).unwrap();
        let used = borrowed_buffer.take_used();

        assert_eq!(used, &[10, 20, 30]);
        used[0] = 255;
        assert_eq!(used, &[255, 20, 30]);
    }

    #[test]
    fn test_extend_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[]).unwrap();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.as_slice(), &[]);
    }

    #[test]
    fn test_capacity_overflow() {
        let mut buffer = [0u8; 2];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        assert!(borrowed_buffer.append_from_slice(&[1, 2, 3]).is_err());
        assert_eq!(borrowed_buffer.len(), 0);

        borrowed_buffer.append_from_slice(&[1, 2]).unwrap();
        assert!(borrowed_buffer.push(3).is_err());
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2]);
    }

    #[test]
    fn test_multiple_extension() {
        let mut buffer = [0u8; 10];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3]);

        borrowed_buffer.append_from_slice(&[4, 5]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);

        borrowed_buffer.append_from_slice(&[6, 7, 8, 9, 10]).unwrap();
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        assert!(borrowed_buffer.append_from_slice(&[11]).is_err());
    }

    #[test]
    fn test_append_from_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        // Append with data larger than remaining capacity
        let filled = borrowed_buffer.try_append_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);

        // Try to append when buffer is full
        let filled = borrowed_buffer.try_append_from_slice(&[8, 9]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_append_from_slice_partial() {
        let mut buffer = [0u8; 10];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        // Add some data first
        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.remaining_capacity(), 7);

        // Append with smaller slice
        let filled = borrowed_buffer.try_append_from_slice(&[4, 5]);
        assert_eq!(filled, 2);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 5);

        // Append remaining with exact size
        let filled = borrowed_buffer.try_append_from_slice(&[6, 7, 8, 9, 10]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(borrowed_buffer.remaining_capacity(), 0);
    }

    #[test]
    fn test_fill_remaining_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);

        let filled = borrowed_buffer.try_append_from_slice(&[]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.len(), 0);
    }

    #[test]
    fn test_as_slice_lifetime() {
        let mut buffer = [1, 2, 3, 4, 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = BorrowedBuffer::new(&mut allocator);
        borrowed_buffer.append_from_slice(&[1, 2, 3]).unwrap();

        let slice1 = borrowed_buffer.as_slice();
        let slice2 = borrowed_buffer.as_slice();

        assert_eq!(slice1, slice2);
        assert_eq!(slice1, &[1, 2, 3]);
    }
}
