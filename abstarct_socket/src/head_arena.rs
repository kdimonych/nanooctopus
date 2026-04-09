use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// A bump-style arena over caller-provided byte storage.
///
/// `PrefixArena` hands out slices from the front of the remaining buffer and keeps
/// the rest available for later use. It only manages buffer boundaries; it does
/// not track which bytes are initialized.
pub struct PrefixArena<'buf> {
    remaining: UnsafeCell<&'buf mut [MaybeUninit<u8>]>,
}

impl<'buf> PrefixArena<'buf> {
    #[inline(always)]
    pub const fn new(arena: &'buf mut [u8]) -> Self {
        // SAFETY: Transmuting a mutable slice of u8 to a mutable slice of MaybeUninit<u8> is safe
        // because MaybeUninit<u8> has the same memory layout as u8 and does not require any special handling.
        Self::from_uninit(unsafe { core::mem::transmute(arena) })
    }

    #[inline]
    pub const fn from_uninit(arena: &'buf mut [MaybeUninit<u8>]) -> Self {
        Self {
            remaining: UnsafeCell::new(arena),
        }
    }

    /// Returns the number of bytes still available in the arena.
    #[inline]
    pub const fn len(&self) -> usize {
        let remaining = unsafe { &*self.remaining.get() };
        remaining.len()
    }

    /// Returns `true` when no bytes remain available.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrows the currently remaining arena space through a temporary view.
    pub const fn view<'arena>(&'arena mut self) -> ArenaView<'arena, 'buf> {
        // SAFETY: `self.remaining` points at the remaining part of the arena for `'buf`.
        ArenaView::<'arena, 'buf>::new(self.remaining.get())
    }

    /// Removes the first `n` bytes from the remaining arena and returns them.
    ///
    /// The returned slice is uninitialized storage. The caller must initialize
    /// it before reading from it.
    ///
    /// # Panics
    /// Panics if `n > self.len()`.
    pub fn take_prefix(&self, n: usize) -> &'buf mut [MaybeUninit<u8>] {
        let buffer = unsafe { &mut *self.remaining.get() };
        let (used, remaining) = buffer.split_at_mut(n);
        unsafe { *self.remaining.get() = remaining };
        used
    }

    /// Removes the first `n` bytes from the remaining arena and returns them as `u8`.
    ///
    /// # Safety
    /// The caller must ensure both of the following:
    /// - `n <= self.len()`.
    /// - Every returned byte is initialized before it is read as `u8`.
    #[inline(always)]
    pub unsafe fn take_prefix_unchecked(&self, n: usize) -> &'buf mut [u8] {
        let buffer = unsafe { &mut *self.remaining.get() };
        let (used, remaining) = unsafe { buffer.split_at_mut_unchecked(n) };
        unsafe { *self.remaining.get() = remaining };
        unsafe { core::mem::transmute(used) }
    }

    /// Returns all bytes that still remain in the arena and consumes `self`.
    ///
    /// The returned slice is uninitialized storage. The caller must initialize
    /// it before reading from it.
    pub fn take_remaining(self) -> &'buf mut [MaybeUninit<u8>] {
        self.remaining.into_inner()
    }

    /// Exposes the remaining arena as `&mut [u8]`, lets `f` initialize a prefix,
    /// and then detaches that initialized prefix from the arena.
    ///
    /// If `f` returns `Err`, the arena remains unchanged.
    ///
    /// # Safety
    /// `f` must return the length of a prefix that it actually initialized.
    ///
    /// # Panics
    /// Panics if `f` returns a length greater than the currently remaining size.
    pub fn init_prefix_with<F, E>(self, f: F) -> Result<&'buf mut [u8], E>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, E>,
    {
        let buffer: &mut [MaybeUninit<u8>] = unsafe { self.remaining.get().as_mut().unwrap_unchecked() };
        let slice: &mut [u8] = unsafe { core::mem::transmute(buffer) };
        let initialized_len = f(slice)?;
        if initialized_len > slice.len() {
            panic!("Initializer function returned a length greater than the current buffer size");
        }

        Ok(unsafe { self.take_prefix_unchecked(initialized_len) })
    }
}

/// A temporary view over the remaining bytes of a [`PrefixArena`].
///
/// This is useful when code needs to fill an unknown-sized prefix of the
/// remaining arena and then detach only the initialized portion.
pub struct ArenaView<'arena, 'buf>
where
    'buf: 'arena,
{
    remaining: *mut &'buf mut [MaybeUninit<u8>],
    _marker: core::marker::PhantomData<&'arena ()>,
}

impl<'arena, 'buf> ArenaView<'arena, 'buf> {
    /// Creates a temporary view over the remaining arena bytes.
    const fn new(remaining: *mut &'buf mut [MaybeUninit<u8>]) -> Self {
        Self {
            remaining,
            _marker: core::marker::PhantomData,
        }
    }

    /// Returns the number of bytes visible through this temporary view.
    #[inline]
    pub const fn len(&self) -> usize {
        let remaining = unsafe { &*self.remaining.as_mut().unwrap_unchecked() };
        remaining.len()
    }

    /// Returns `true` when no bytes are visible through this view.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the remaining arena bytes as uninitialized storage.
    #[inline]
    pub const fn as_uninit_slice(&self) -> &[MaybeUninit<u8>] {
        self.as_slice()
    }

    /// Returns the remaining arena bytes as uninitialized storage.
    #[inline]
    pub const fn as_slice(&self) -> &[MaybeUninit<u8>] {
        unsafe { &*self.remaining.as_mut().unwrap_unchecked() }
    }

    /// Returns the remaining arena bytes as `u8`.
    ///
    /// # Safety
    /// Every returned byte must be initialized before it is read as `u8`.
    #[inline]
    pub const unsafe fn as_slice_unchecked(&self) -> &[u8] {
        unsafe { core::mem::transmute(self.as_slice()) }
    }

    /// Returns the remaining arena bytes as mutable uninitialized storage.
    #[inline]
    pub const fn as_uninit_slice_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.as_slice_mut()
    }

    /// Returns the remaining arena bytes as mutable uninitialized storage.
    #[inline]
    pub const fn as_slice_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe { &mut *self.remaining.as_mut().unwrap_unchecked() }
    }

    /// Returns the remaining arena bytes as mutable `u8`.
    ///
    /// # Safety
    /// Every returned byte must be initialized before it is read as `u8`.
    #[inline]
    pub const unsafe fn as_slice_mut_unchecked(&mut self) -> &mut [u8] {
        unsafe { core::mem::transmute(self.as_slice_mut()) }
    }

    /// Lets `f` initialize a prefix of the temporary view and returns that prefix.
    ///
    /// This method does not shrink the underlying arena.
    ///
    /// # Panics
    /// Panics if `f` returns a length greater than `self.len()`.
    pub fn init_with<F, E>(&mut self, f: F) -> Result<&mut [u8], E>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, E>,
    {
        let slice: &mut [u8] = unsafe { self.as_slice_mut_unchecked() };
        let initialized_len = f(slice)?;
        if initialized_len > slice.len() {
            panic!("Initializer function returned a length greater than the current buffer size");
        }
        Ok(&mut slice[..initialized_len])
    }

    /// Lets `f` initialize a prefix of the temporary view and then detaches that
    /// prefix from the underlying arena.
    ///
    /// If `f` returns `Err`, the underlying arena remains unchanged.
    ///
    /// # Safety
    /// `f` must return the length of a prefix that it actually initialized.
    ///
    /// # Panics
    /// Panics if `f` returns a length greater than `self.len()`.
    pub fn init_prefix_with<F, E>(mut self, f: F) -> Result<&'buf mut [u8], E>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, E>,
    {
        let slice: &mut [u8] = unsafe { self.as_slice_mut_unchecked() };
        let initialized_len = f(slice)?;
        if initialized_len > slice.len() {
            panic!("Initializer function returned a length greater than the current buffer size");
        }

        Ok(unsafe { self.take_prefix_unchecked(initialized_len) })
    }

    /// Removes the first `n` bytes from the underlying arena and returns them.
    ///
    /// The returned slice is uninitialized storage. The caller must initialize
    /// it before reading from it.
    ///
    /// # Panics
    /// Panics if `n > self.len()`.
    pub fn take_prefix(self, n: usize) -> &'buf mut [MaybeUninit<u8>] {
        let buffer = unsafe { &mut *self.remaining.as_mut().unwrap_unchecked() };
        let (used, remaining) = buffer.split_at_mut(n);
        unsafe { *self.remaining.as_mut().unwrap_unchecked() = remaining };
        used
    }

    /// Removes the first `n` bytes from the underlying arena and returns them as `u8`.
    ///
    /// # Safety
    /// The caller must ensure both of the following:
    /// - `n <= self.len()`.
    /// - Every returned byte is initialized before it is read as `u8`.
    #[inline(always)]
    pub unsafe fn take_prefix_unchecked(self, n: usize) -> &'buf mut [u8]
    where
        'buf: 'arena,
    {
        let buffer = unsafe { &mut *self.remaining.as_mut().unwrap_unchecked() };
        let (used, remaining) = unsafe { buffer.split_at_mut_unchecked(n) };
        unsafe { *self.remaining.as_mut().unwrap_unchecked() = remaining };
        unsafe { core::mem::transmute(used) }
    }
}

impl<'buf> From<&'buf mut [u8]> for PrefixArena<'buf> {
    fn from(buffer: &'buf mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'buf> From<&'buf mut [MaybeUninit<u8>]> for PrefixArena<'buf> {
    fn from(buffer: &'buf mut [MaybeUninit<u8>]) -> Self {
        Self::from_uninit(buffer)
    }
}

impl<'buf, const N: usize> From<&'buf mut [u8; N]> for PrefixArena<'buf> {
    fn from(buffer: &'buf mut [u8; N]) -> Self {
        Self::new(&mut buffer[..])
    }
}

impl<'buf, const N: usize> From<&'buf mut [MaybeUninit<u8>; N]> for PrefixArena<'buf> {
    fn from(buffer: &'buf mut [MaybeUninit<u8>; N]) -> Self {
        Self::from_uninit(&mut buffer[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestError {
        Expected,
    }

    #[test]
    fn test_init_from_initialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = PrefixArena::new(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_init_from_uninitialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena =
            PrefixArena::from_uninit(unsafe { core::mem::transmute::<_, &mut [MaybeUninit<u8>; 5]>(&mut data) });
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = PrefixArena::new(&mut data[..]);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn test_from_uninitialized_slice() {
        let data: [u8; 5] = [1, 2, 3, 4, 5];
        let mut uninit_data = unsafe { core::mem::transmute::<_, [MaybeUninit<u8>; 5]>(data) };

        let head_arena = PrefixArena::from_uninit(&mut uninit_data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_array() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = PrefixArena::from(&mut data);
        assert_eq!(head_arena.len(), 5);
    }

    #[test]
    fn test_from_array_for_temporary_buffer() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::from(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(head_arena.view().len(), 5);
    }

    #[test]
    fn test_empty_buffer() {
        let mut data = [];
        let head_arena = PrefixArena::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_empty_buffer_for_temporary_buffer() {
        let mut data = [];
        let mut head_arena = PrefixArena::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
        let temp_buffer = head_arena.view();
        assert_eq!(temp_buffer.len(), 0);
        assert!(temp_buffer.is_empty());
    }

    #[test]
    fn test_temp_buffer_as_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let mut temp_buffer = head_arena.view();
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(temp_buffer.as_slice_mut()) },
            &[1, 2, 3, 4, 5]
        );
    }
    #[test]
    fn test_temp_buffer_as_slice_unchecked() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let temp_buffer = head_arena.view();
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_temp_buffer_as_mut_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let mut temp_buffer = head_arena.view();
        let slice = temp_buffer.as_slice_mut();
        slice[0].write(10);
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_temp_buffer_as_mut_slice_unchecked() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let mut temp_buffer = head_arena.view();
        let slice = unsafe { temp_buffer.as_slice_mut_unchecked() };
        slice[0] = 10;
        assert_eq!(unsafe { temp_buffer.as_slice_unchecked() }, &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_head_arena_init_then_acquire_with() {
        let mut data = [0u8; 5];
        let head_arena = PrefixArena::new(&mut data);

        let detached = head_arena
            .init_prefix_with(|buffer| {
                buffer[..3].copy_from_slice(&[7, 8, 9]);
                Ok::<usize, TestError>(3)
            })
            .unwrap();

        assert_eq!(detached, &[7, 8, 9]);
    }

    #[test]
    #[should_panic(expected = "Initializer function returned a length greater than the current buffer size")]
    fn test_head_arena_init_then_acquire_with_panics_on_invalid_len() {
        let mut data = [0u8; 3];
        let head_arena = PrefixArena::new(&mut data);

        let _ = head_arena.init_prefix_with(|_| Ok::<usize, TestError>(4));
    }

    #[test]
    fn test_temp_buffer_as_mut_with_init() {
        let mut data = [0u8; 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let mut temp_buffer = head_arena.view();

        let initialized = temp_buffer
            .init_with(|buffer| {
                buffer[..2].copy_from_slice(&[11, 12]);
                Ok::<usize, TestError>(2)
            })
            .unwrap();

        assert_eq!(initialized, &[11, 12]);
        assert_eq!(temp_buffer.len(), 5);
        assert_eq!(head_arena.len(), 5);
    }

    #[test]
    fn test_temp_buffer_init_then_acquire_with() {
        let mut data = [0u8; 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let temp_buffer = head_arena.view();

        let detached = temp_buffer
            .init_prefix_with(|buffer| {
                buffer[..2].copy_from_slice(&[21, 22]);
                Ok::<usize, TestError>(2)
            })
            .unwrap();

        assert_eq!(detached, &[21, 22]);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[0, 0, 0]
        );
    }

    #[test]
    fn test_temp_buffer_init_then_acquire_with_error_preserves_arena() {
        let mut data = [1u8, 2, 3, 4, 5];
        let mut head_arena = PrefixArena::new(&mut data);
        let temp_buffer = head_arena.view();

        let error = temp_buffer
            .init_prefix_with(|buffer| {
                buffer[0] = 99;
                Err::<usize, TestError>(TestError::Expected)
            })
            .unwrap_err();

        assert_eq!(error, TestError::Expected);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[99, 2, 3, 4, 5]
        );
    }

    #[test]
    #[should_panic(expected = "Initializer function returned a length greater than the current buffer size")]
    fn test_temp_buffer_as_mut_with_init_panics_on_invalid_len() {
        let mut data = [0u8; 3];
        let mut head_arena = PrefixArena::new(&mut data);
        let mut temp_buffer = head_arena.view();

        let _ = temp_buffer.init_with(|_| Ok::<usize, TestError>(4));
    }

    #[test]
    #[should_panic(expected = "Initializer function returned a length greater than the current buffer size")]
    fn test_temp_buffer_init_then_acquire_with_panics_on_invalid_len() {
        let mut data = [0u8; 3];
        let mut head_arena = PrefixArena::new(&mut data);
        let temp_buffer = head_arena.view();

        let _ = temp_buffer.init_prefix_with(|_| Ok::<usize, TestError>(4));
    }

    #[test]
    fn test_take_front_partial() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = PrefixArena::new(&mut data);

        let detached = unsafe { head_arena.take_prefix_unchecked(2) };
        assert_eq!(detached, &[1, 2]);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[3, 4, 5]
        );
    }

    #[test]
    fn test_take_front_all() {
        let mut data = [1, 2, 3];
        let head_arena = PrefixArena::new(&mut data);

        let detached = unsafe { head_arena.take_prefix_unchecked(3) };
        assert_eq!(detached, &[1, 2, 3]);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_multiple_take_fronts() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let head_arena = PrefixArena::new(&mut data);
        let first = unsafe { head_arena.take_prefix_unchecked(2) };
        assert_eq!(first, &[1, 2]);
        assert_eq!(head_arena.len(), 4);

        let second = unsafe { head_arena.take_prefix_unchecked(2) };
        assert_eq!(second, &[3, 4]);
        assert_eq!(head_arena.len(), 2);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[5, 6]
        );
    }

    #[test]
    #[should_panic]
    fn test_take_front_too_large() {
        let mut data = [1, 2, 3];
        let head_arena = PrefixArena::new(&mut data);
        head_arena.take_prefix(4);
    }

    #[test]
    fn test_take_front_zero() {
        let mut data = [1, 2, 3];
        let head_arena = PrefixArena::new(&mut data);

        let detached = head_arena.take_prefix(0);
        assert_eq!(detached.len(), 0);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(
            unsafe { core::mem::transmute::<_, &[u8]>(head_arena.take_remaining()) },
            &[1, 2, 3]
        );
    }

    #[test]
    fn test_usage_in_a_loop() {
        const PART_SIZE: usize = 3;
        const BUFFER_SIZE: usize = 10;
        const EXPECTED_PARTS: [u8; BUFFER_SIZE] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut data: [u8; BUFFER_SIZE] = EXPECTED_PARTS;

        let head_arena = PrefixArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = unsafe { head_arena.take_prefix_unchecked(to_detach) };
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
        let head_arena = PrefixArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = unsafe { head_arena.take_prefix_unchecked(to_detach) };
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
        let mut head_arena = PrefixArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let temp_buffer = head_arena.view();
            let to_detach = core::cmp::min(temp_buffer.len(), PART_SIZE);
            let detached = unsafe { temp_buffer.take_prefix_unchecked(to_detach) };
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
        let head_arena = PrefixArena::new(&mut data);
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining()) };
        assert_eq!(remaining, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn subsequent_take_remaining() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = PrefixArena::new(&mut data);
        let _ = head_arena.take_prefix(2); // Detach first 2 bytes
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining()) };
        assert_eq!(remaining, &[3, 4, 5]);
    }
}
