use crate::arena::{ArenaView, PrefixArena};

/// A staging buffer over the remaining bytes of a [`PrefixArena`].
///
/// `StagingBuffer` lets callers append bytes into temporary arena-backed
/// storage, inspect the written prefix, and finally detach that written prefix
/// from the underlying arena.
pub struct StagingBuffer<'arena, 'b> {
    inner: ArenaView<'arena, 'b>,
    used: usize,
}

impl<'arena, 'b> StagingBuffer<'arena, 'b>
where
    'b: 'arena,
{
    /// Creates a staging buffer over the remaining space of the given arena.
    pub const fn new(arena: &'arena mut PrefixArena<'b>) -> Self {
        Self {
            inner: arena.view(),
            used: 0,
        }
    }

    /// Appends one byte to the written prefix.
    ///
    /// Returns `Ok(())` on success and `Err(())` if no capacity remains.
    pub fn push_byte(&mut self, byte: u8) -> Result<(), ()> {
        if self.used >= self.inner.len() {
            return Err(());
        }
        self.inner.as_slice_mut()[self.used].write(byte);
        self.used += 1;
        Ok(())
    }

    /// Appends the entire slice to the written prefix.
    ///
    /// Returns `Ok(())` on success and `Err(())` if the full slice does not fit.
    pub fn extend_from_slice(&mut self, slice: &[u8]) -> Result<(), ()> {
        if self.used + slice.len() > self.inner.len() {
            return Err(());
        }
        self.inner.as_slice_mut()[self.used..self.used + slice.len()]
            .copy_from_slice(unsafe { core::mem::transmute(slice) });
        self.used += slice.len();
        Ok(())
    }

    /// Appends as much of the slice as fits and returns the number of bytes written.
    pub fn extend_from_slice_capped(&mut self, slice: &[u8]) -> usize {
        let to_fill = core::cmp::min(self.spare_capacity(), slice.len());
        self.inner.as_slice_mut()[self.used..self.used + to_fill]
            .copy_from_slice(unsafe { core::mem::transmute(&slice[..to_fill]) });
        self.used += to_fill;
        to_fill
    }

    /// Returns the written prefix as an immutable slice.
    pub fn written(&self) -> &[u8] {
        unsafe { core::mem::transmute(&self.inner.as_slice()[..self.used]) }
    }

    /// Returns the written prefix as a mutable slice.
    pub fn written_mut(&mut self) -> &mut [u8] {
        unsafe { core::mem::transmute(&mut self.inner.as_slice_mut()[..self.used]) }
    }

    /// Returns the number of bytes currently stored in the buffer.
    pub const fn len(&self) -> usize {
        self.used
    }

    /// Returns the total capacity of the buffer.
    pub const fn capacity(&self) -> usize {
        self.inner.len()
    }

    /// Returns how many bytes can still be appended without overflowing.
    pub const fn spare_capacity(&self) -> usize {
        self.inner.len() - self.used
    }

    /// Marks the buffer as empty without modifying the underlying bytes.
    pub fn clear(&mut self) {
        self.used = 0;
    }

    /// Detaches the written prefix from the underlying arena.
    pub fn into_written_slice(self) -> &'b mut [u8] {
        unsafe { core::mem::transmute(self.inner.take_prefix(self.used)) }
    }
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::arena::PrefixArena as HeadArena;

    #[test]
    fn test_basic_push() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 5);
        assert_eq!(borrowed_buffer.spare_capacity(), 5);

        borrowed_buffer.push_byte(42).unwrap();
        assert_eq!(borrowed_buffer.len(), 1);
        assert_eq!(borrowed_buffer.written(), &[42]);
        assert_eq!(borrowed_buffer.spare_capacity(), 4);
    }

    #[test]
    fn test_push_until_full() {
        let mut buffer = [0u8; 3];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.push_byte(1).unwrap();
        borrowed_buffer.push_byte(2).unwrap();
        borrowed_buffer.push_byte(3).unwrap();

        assert_eq!(borrowed_buffer.written(), &[1, 2, 3]);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);
        assert!(borrowed_buffer.push_byte(4).is_err());
    }

    #[test]
    fn test_clear() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.spare_capacity(), 5);
        assert_eq!(borrowed_buffer.written(), &[]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();

        let mut_slice = borrowed_buffer.written_mut();
        mut_slice[1] = 99;

        assert_eq!(borrowed_buffer.written(), &[1, 99, 3]);
    }

    #[test]
    fn test_empty_buffer() {
        let mut buffer = [0u8; 0];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.capacity(), 0);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);
        assert!(borrowed_buffer.push_byte(1).is_err());
        assert!(borrowed_buffer.extend_from_slice(&[1]).is_err());
        assert_eq!(borrowed_buffer.written(), &[]);
    }

    #[test]
    fn test_single_byte_buffer() {
        let mut buffer = [0u8; 1];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        assert_eq!(borrowed_buffer.capacity(), 1);
        assert_eq!(borrowed_buffer.spare_capacity(), 1);

        borrowed_buffer.push_byte(42).unwrap();
        assert_eq!(borrowed_buffer.written(), &[42]);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);

        assert!(borrowed_buffer.push_byte(1).is_err());
        assert!(borrowed_buffer.extend_from_slice(&[1]).is_err());
    }

    #[test]
    fn test_clear_and_reuse() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.len(), 3);

        borrowed_buffer.clear();
        borrowed_buffer.extend_from_slice(&[4, 5, 6, 7, 8]).unwrap();
        assert_eq!(borrowed_buffer.written(), &[4, 5, 6, 7, 8]);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);
    }

    #[test]
    fn test_extend_at_exact_capacity() {
        let mut buffer = [0u8; 3];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        assert!(borrowed_buffer.extend_from_slice(&[1, 2, 3]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);

        assert!(borrowed_buffer.extend_from_slice(&[]).is_ok());
        assert_eq!(borrowed_buffer.len(), 3);
    }

    #[test]
    fn test_take_used() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[10, 20, 30]).unwrap();
        let used = borrowed_buffer.into_written_slice();

        assert_eq!(used, &[10, 20, 30]);
        used[0] = 255;
        assert_eq!(used, &[255, 20, 30]);
    }

    #[test]
    fn test_extend_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[]).unwrap();
        assert_eq!(borrowed_buffer.len(), 0);
        assert_eq!(borrowed_buffer.written(), &[]);
    }

    #[test]
    fn test_capacity_overflow() {
        let mut buffer = [0u8; 2];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        assert!(borrowed_buffer.extend_from_slice(&[1, 2, 3]).is_err());
        assert_eq!(borrowed_buffer.len(), 0);

        borrowed_buffer.extend_from_slice(&[1, 2]).unwrap();
        assert!(borrowed_buffer.push_byte(3).is_err());
        assert_eq!(borrowed_buffer.written(), &[1, 2]);
    }

    #[test]
    fn test_multiple_extension() {
        let mut buffer = [0u8; 10];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3]);

        borrowed_buffer.extend_from_slice(&[4, 5]).unwrap();
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5]);

        borrowed_buffer.extend_from_slice(&[6, 7, 8, 9, 10]).unwrap();
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        assert!(borrowed_buffer.extend_from_slice(&[11]).is_err());
    }

    #[test]
    fn test_append_from_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        // Append with data larger than remaining capacity
        let filled = borrowed_buffer.extend_from_slice_capped(&[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);

        // Try to append when buffer is full
        let filled = borrowed_buffer.extend_from_slice_capped(&[8, 9]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_append_from_slice_partial() {
        let mut buffer = [0u8; 10];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        // Add some data first
        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();
        assert_eq!(borrowed_buffer.spare_capacity(), 7);

        // Append with smaller slice
        let filled = borrowed_buffer.extend_from_slice_capped(&[4, 5]);
        assert_eq!(filled, 2);
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5]);
        assert_eq!(borrowed_buffer.spare_capacity(), 5);

        // Append remaining with exact size
        let filled = borrowed_buffer.extend_from_slice_capped(&[6, 7, 8, 9, 10]);
        assert_eq!(filled, 5);
        assert_eq!(borrowed_buffer.written(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(borrowed_buffer.spare_capacity(), 0);
    }

    #[test]
    fn test_fill_remaining_empty_slice() {
        let mut buffer = [0u8; 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);

        let filled = borrowed_buffer.extend_from_slice_capped(&[]);
        assert_eq!(filled, 0);
        assert_eq!(borrowed_buffer.len(), 0);
    }

    #[test]
    fn test_as_slice_lifetime() {
        let mut buffer = [1, 2, 3, 4, 5];
        let mut allocator = HeadArena::new(&mut buffer);
        let mut borrowed_buffer = StagingBuffer::new(&mut allocator);
        borrowed_buffer.extend_from_slice(&[1, 2, 3]).unwrap();

        let slice1 = borrowed_buffer.written();
        let slice2 = borrowed_buffer.written();

        assert_eq!(slice1, slice2);
        assert_eq!(slice1, &[1, 2, 3]);
    }
}
