use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// A simple bump allocator that allows detaching front portions of a buffer.
/// This is useful for scenarios where data is read into a buffer
/// and then parts of it need to be processed or handed off without copying.
/// The remaining buffer can still be used for further allocations.
/// Another words, take all leve what you need from the front, and the rest will be still available for future use.
///
/// ### Note
/// The `HeadArena` does not track initialization of data; it simply manages
/// the buffer space.
pub struct HeadArena<'buf> {
    arena: UnsafeCell<&'buf mut [MaybeUninit<u8>]>,
}

impl<'buf> HeadArena<'buf> {
    #[inline(always)]
    pub const fn new(arena: &'buf mut [u8]) -> Self {
        // SAFETY: Transmuting a mutable slice of u8 to a mutable slice of MaybeUninit<u8> is safe
        // because MaybeUninit<u8> has the same memory layout as u8 and does not require any special handling.
        Self::from_uninitialized(unsafe { core::mem::transmute(arena) })
    }

    #[inline]
    pub const fn from_uninitialized(arena: &'buf mut [MaybeUninit<u8>]) -> Self {
        Self {
            arena: UnsafeCell::new(arena),
        }
    }

    /// Returns the length of free arena.
    #[inline]
    pub const fn len(&self) -> usize {
        let arena = unsafe { &*self.arena.get() };
        arena.len()
    }

    /// Returns true if the arena is empty.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Creates a new `TempBuffer` that borrows the entire arena space.
    pub const fn temporary<'arena>(&'arena mut self) -> TempBuffer<'arena, 'buf> {
        TempBuffer::<'arena, 'buf>::new_uninitialized(&mut self.arena)
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    /// This primitive doesn't keep track whether data were actually initialized or not within front n bytes.
    /// ### Note: The returned slice should be considered as not initialized, even if the original buffer contained some data.
    /// The caller is responsible for initializing it before use.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the current buffer size.
    ///
    pub fn take_front_mut(&self, n: usize) -> &'buf mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { &mut *self.arena.get() };
        let (used, remainig) = buffer.split_at_mut(n);
        unsafe { *self.arena.get() = remainig };
        used
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    /// The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    /// This primitive doesn't keep track whether data were actually initialized or not within front n bytes.
    /// ### Note: The returned slice should be considered as not initialized, even if the original buffer contained some data.
    /// The caller is responsible for initializing it before use.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the current buffer size.
    ///
    #[inline(always)]
    pub unsafe fn take_front_mut_unchecked(&self, n: usize) -> &'buf mut [u8] {
        unsafe { core::mem::transmute(self.take_front_mut(n)) }
    }

    /// Takes the remaining arena space as a mutable slice. This consumes the HeadArena.
    /// The caller is responsible for initializing the returned slice before use.
    pub fn take_remaining_mut(self) -> &'buf mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        self.arena.into_inner()
    }
}

/// A temporary buffer that can be initialized and then used to acquire mutable slices from the front.
/// This struct is designed to work with the `HeadArena` and provides a way to manage initialization state.
pub struct TempBuffer<'arena, 'buf>
where
    'buf: 'arena,
{
    arena: &'arena mut UnsafeCell<&'buf mut [MaybeUninit<u8>]>,
}

impl<'arena, 'buf> TempBuffer<'arena, 'buf> {
    const fn new_uninitialized(arena: &'arena mut UnsafeCell<&'buf mut [MaybeUninit<u8>]>) -> Self {
        Self { arena }
    }

    /// Returns the length of free arena.
    #[inline]
    pub const fn len(&self) -> usize {
        // SAFETY: self.arena is guaranteed to be Some
        let arena = unsafe { &*self.arena.get() };
        arena.len()
    }

    /// Returns true if the arena is empty.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrows the all available arenna space as an immutable slice.
    #[inline]
    pub fn as_slice(&self) -> &[MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { &*self.arena.get() }
    }

    /// Borrows the all available arena space as a mutable slice.
    /// The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
    #[inline]
    pub unsafe fn as_slice_unchecked(&self) -> &[u8] {
        // SAFETY: The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
        unsafe { core::mem::transmute(self.as_slice()) }
    }

    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { &mut *self.arena.get() }
    }

    #[inline]
    pub unsafe fn as_slice_mut_unchecked(&mut self) -> &mut [u8] {
        // SAFETY: The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
        unsafe { core::mem::transmute(self.as_slice_mut()) }
    }

    /// Initializes a portion of the arena temporary buffer using the provided function and returns it as a mutable slice.
    pub fn init_slice_with<F, E>(&mut self, f: F) -> Result<&mut [u8], E>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, E>,
    {
        let slice: &mut [u8] = unsafe { self.as_slice_mut_unchecked() };
        let initialized_len = f(slice)?;
        Ok(&mut slice[..initialized_len])
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    /// This primitive doesn't keep track whether data were actually initialized or not within front n bytes.
    /// ### Note: The returned slice should be considered as not initialized, even if the original buffer contained some data.
    /// The caller is responsible for initializing it before use.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the current buffer size.
    ///
    pub fn accuire_front_mut(self, n: usize) -> &'buf mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { &mut *self.arena.get() };
        let (used, remainig) = buffer.split_at_mut(n);
        unsafe { *self.arena.get() = remainig };
        used
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    /// The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    /// This primitive doesn't keep track whether data were actually initialized or not within front n bytes.
    /// ### Note: The returned slice should be considered as not initialized, even if the original buffer contained some data.
    /// The caller is responsible for initializing it before use.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the current buffer size.
    ///
    #[inline(always)]
    pub unsafe fn accuire_front_mut_unchecked(self, n: usize) -> &'buf mut [u8]
    where
        'buf: 'arena,
    {
        unsafe { core::mem::transmute(self.accuire_front_mut(n)) }
    }
}

impl<'buf> From<&'buf mut [u8]> for HeadArena<'buf> {
    fn from(buffer: &'buf mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'buf> From<&'buf mut [MaybeUninit<u8>]> for HeadArena<'buf> {
    fn from(buffer: &'buf mut [MaybeUninit<u8>]) -> Self {
        Self::from_uninitialized(buffer)
    }
}

impl<'buf, const N: usize> From<&'buf mut [u8; N]> for HeadArena<'buf> {
    fn from(buffer: &'buf mut [u8; N]) -> Self {
        Self::new(&mut buffer[..])
    }
}

impl<'buf, const N: usize> From<&'buf mut [MaybeUninit<u8>; N]> for HeadArena<'buf> {
    fn from(buffer: &'buf mut [MaybeUninit<u8>; N]) -> Self {
        Self::from_uninitialized(&mut buffer[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_from_initialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_init_from_uninitialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena =
            HeadArena::from_uninitialized(unsafe { core::mem::transmute::<_, &mut [MaybeUninit<u8>; 5]>(&mut data) });
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data[..]);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining_mut()) },
            &[1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn test_from_uninitialized_slice() {
        let data: [u8; 5] = [1, 2, 3, 4, 5];
        let mut uninit_data = unsafe { core::mem::transmute::<_, [MaybeUninit<u8>; 5]>(data) };

        let head_arena = HeadArena::from_uninitialized(&mut uninit_data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_array() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::from(&mut data);
        assert_eq!(head_arena.len(), 5);
    }

    #[test]
    fn test_from_array_for_temporary_buffer() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::from(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(head_arena.temporary().len(), 5);
    }

    #[test]
    fn test_empty_buffer() {
        let mut data = [];
        let head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_empty_buffer_for_temporary_buffer() {
        let mut data = [];
        let mut head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
        let temp_buffer = head_arena.temporary();
        assert_eq!(temp_buffer.len(), 0);
        assert!(temp_buffer.is_empty());
    }

    #[test]
    fn test_temp_buffer_as_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);
        let mut temp_buffer = head_arena.temporary();
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(temp_buffer.as_slice_mut()) },
            &[1, 2, 3, 4, 5]
        );
    }
    #[test]
    fn test_temp_buffer_as_slice_unchecked() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);
        let temp_buffer = head_arena.temporary();
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_temp_buffer_as_mut_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);
        let mut temp_buffer = head_arena.temporary();
        let slice = temp_buffer.as_slice_mut();
        slice[0].write(10);
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_temp_buffer_as_mut_slice_unchecked() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);
        let mut temp_buffer = head_arena.temporary();
        let slice = unsafe { temp_buffer.as_slice_mut_unchecked() };
        slice[0] = 10;
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_take_front_partial() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);

        let detached = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(detached, &[1, 2]);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining_mut()) },
            &[3, 4, 5]
        );
    }

    #[test]
    fn test_take_front_all() {
        let mut data = [1, 2, 3];
        let head_arena = HeadArena::new(&mut data);

        let detached = unsafe { head_arena.take_front_mut_unchecked(3) };
        assert_eq!(detached, &[1, 2, 3]);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_multiple_take_fronts() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let head_arena = HeadArena::new(&mut data);
        let first = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(first, &[1, 2]);
        assert_eq!(head_arena.len(), 4);

        let second = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(second, &[3, 4]);
        assert_eq!(head_arena.len(), 2);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining_mut()) },
            &[5, 6]
        );
    }

    #[test]
    #[should_panic]
    fn test_take_front_too_large() {
        let mut data = [1, 2, 3];
        let head_arena = HeadArena::new(&mut data);
        head_arena.take_front_mut(4);
    }

    #[test]
    fn test_take_front_zero() {
        let mut data = [1, 2, 3];
        let head_arena = HeadArena::new(&mut data);

        let detached = head_arena.take_front_mut(0);
        assert_eq!(detached.len(), 0);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining_mut()) },
            &[1, 2, 3]
        );
    }

    #[test]
    fn test_usage_in_a_loop() {
        const PART_SIZE: usize = 3;
        const BUFFER_SIZE: usize = 10;
        const EXPECTED_PARTS: [u8; BUFFER_SIZE] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut data: [u8; BUFFER_SIZE] = EXPECTED_PARTS;

        let head_arena = HeadArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = unsafe { head_arena.take_front_mut_unchecked(to_detach) };
            detached_parts.push(detached);
        }

        let mut expected_it = EXPECTED_PARTS.iter();
        for detached in detached_parts {
            detached.iter().for_each(|&byte| {
                assert_eq!(byte, *expected_it.next().unwrap());
            });
        }
    }

    #[test]
    fn test_no_borrowing_glue() {
        const PART_SIZE: usize = 3;
        const BUFFER_SIZE: usize = 10;
        const EXPECTED_PARTS: [u8; BUFFER_SIZE] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let mut data = EXPECTED_PARTS;
        let head_arena = HeadArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = unsafe { head_arena.take_front_mut_unchecked(to_detach) };
            detached_parts.push(detached);
        }

        let mut expected_it = EXPECTED_PARTS.iter();
        for detached in detached_parts {
            detached.iter().for_each(|&byte| {
                assert_eq!(byte, *expected_it.next().unwrap());
            });
        }
    }

    #[test]
    fn test_no_borrowing_glue_with_temp_buffer_detach() {
        const PART_SIZE: usize = 3;
        const BUFFER_SIZE: usize = 10;
        const EXPECTED_PARTS: [u8; BUFFER_SIZE] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let mut data = EXPECTED_PARTS;
        let mut head_arena = HeadArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let temp_buffer = head_arena.temporary();
            let to_detach = core::cmp::min(temp_buffer.len(), PART_SIZE);
            let detached = unsafe { temp_buffer.accuire_front_mut_unchecked(to_detach) };
            detached_parts.push(detached);
        }

        let mut expected_it = EXPECTED_PARTS.iter();
        for detached in detached_parts {
            detached.iter().for_each(|&byte| {
                assert_eq!(byte, *expected_it.next().unwrap());
            });
        }
    }

    /// Test take_remaining method
    #[test]
    fn test_take_remaining() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining_mut()) };
        assert_eq!(remaining, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn subsequent_take_remaining() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);
        let _ = head_arena.take_front_mut(2); // Detach first 2 bytes
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining_mut()) };
        assert_eq!(remaining, &[3, 4, 5]);
    }
}
