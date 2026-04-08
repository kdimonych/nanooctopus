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
pub struct HeadArenaBuffer<'a> {
    arena: Option<&'a mut [MaybeUninit<u8>]>,
}

impl<'a> HeadArenaBuffer<'a> {
    #[inline(always)]
    pub const fn new(arena: &'a mut [u8]) -> Self {
        // SAFETY: Transmuting a mutable slice of u8 to a mutable slice of MaybeUninit<u8> is safe
        // because MaybeUninit<u8> has the same memory layout as u8 and does not require any special handling.
        Self::from_uninitialized(unsafe { core::mem::transmute(arena) })
    }

    #[inline]
    pub const fn from_uninitialized(arena: &'a mut [MaybeUninit<u8>]) -> Self {
        Self { arena: Some(arena) }
    }

    /// Returns the length of free arena.
    #[inline]
    pub const fn len(&self) -> usize {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_ref().unwrap_unchecked().len() }
    }

    /// Returns true if the arena is empty.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a slice to the entire arena.
    #[inline]
    pub fn as_slice(&self) -> &[MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_ref().unwrap_unchecked() }
    }

    /// Returns a slice to the entire arena.
    /// The caller is responsible for ensuring that the data is properly initialized before use.
    #[inline(always)]
    pub unsafe fn as_slice_unchecked(&self) -> &[u8] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { core::mem::transmute(self.as_slice()) }
    }

    /// Borrows an inner space as a mutable slice.
    #[inline]
    pub fn borrow_mut_slice(&mut self) -> &mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_mut().unwrap_unchecked() }
    }

    /// Borrows an inner space as a mutable slice.
    /// The caller is responsible for ensuring that the data is properly initialized before use.
    #[inline(always)]
    pub unsafe fn borrow_mut_slice_unchecked(&mut self) -> &mut [u8] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { core::mem::transmute(self.borrow_mut_slice()) }
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
    pub fn take_front_mut(&mut self, n: usize) -> &'a mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { self.arena.take().unwrap_unchecked() };
        let (used, remainig) = buffer.split_at_mut(n);
        self.arena.replace(remainig);
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
    pub unsafe fn take_front_mut_unchecked(&mut self, n: usize) -> &'a mut [u8] {
        unsafe { core::mem::transmute(self.take_front_mut(n)) }
    }

    /// Borrows the rest of as a subordinate arena. This allows for nested allocations within the remaining buffer.
    /// The returned arena shares the same underlying buffer but has its own independent state.
    /// The original arena will be consumed and should not be used after this call.
    pub fn nested<'b>(&'b mut self) -> HeadArenaBuffer<'b> {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { self.arena.as_mut().unwrap_unchecked() };

        HeadArenaBuffer::from_uninitialized(buffer)
    }

    /// Borrows the n-bytes of as a subordinate arena. This allows for nested allocations within the remaining buffer.
    /// The returned arena shares the same underlying buffer but has its own independent state.
    /// The original arena will be consumed and should not be used after this call.
    pub fn subarena<'b>(&'b mut self, n: usize) -> HeadArenaBuffer<'b> {
        let buffer = self.take_front_mut(n);
        HeadArenaBuffer::from_uninitialized(buffer)
    }

    /// Takes the remaining arena space as a mutable slice. This consumes the HeadArena.
    /// The caller is responsible for initializing the returned slice before use.
    pub fn take_remaining_mut(mut self) -> &'a mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.take().unwrap_unchecked() }
    }
}

/// State marker for the `TempBuffer` to track whether the buffer has been initialized or not.
pub struct Uninitialized;

/// State marker for the `TempBuffer` to track whether the buffer has been initialized or not.
pub struct Initialized {
    size: usize,
}

/// A temporary buffer that can be initialized and then used to acquire mutable slices from the front.
/// This struct is designed to work with the `HeadArena` and provides a way to manage initialization state.
pub struct TempBuffer<'a, 'arena, State = Uninitialized> {
    arena: &'arena mut Option<&'a mut [MaybeUninit<u8>]>,
    state: State,
}

impl<'a, 'arena> TempBuffer<'a, 'arena, Uninitialized> {
    fn new_uninitialized(arena: &'arena mut Option<&'a mut [MaybeUninit<u8>]>) -> Self {
        Self {
            arena,
            state: Uninitialized,
        }
    }

    /// Initializes the first size bytes of the borrowed arena space with zeros and transitions to the Initialized state.
    ///
    /// ### Panics
    /// Panics if the requested size exceeds the arena capacity.
    pub fn initialize_with_zero_all(self) -> TempBuffer<'a, 'arena, Initialized> {
        let size = self.arena.as_ref().unwrap().len();
        self.initialize_with_zero(size)
    }

    /// Initializes the first size bytes of the borrowed arena space with zeros and transitions to the Initialized state.
    ///
    /// ### Panics
    /// Panics if the requested size exceeds the arena capacity.
    pub fn initialize_with_zero(self, size: usize) -> TempBuffer<'a, 'arena, Initialized> {
        if size > self.arena.as_ref().unwrap().len() {
            panic!("Requested size exceeds arena capacity");
        }

        self.arena.as_mut().unwrap().iter_mut().for_each(|byte| {
            byte.write(0);
        });
        TempBuffer {
            arena: self.arena,
            state: Initialized { size },
        }
    }

    /// Initializes the first size bytes of the borrowed arena space using the provided predicate function and transitions to the Initialized state.
    /// The predicate function should write the initialization data into the provided slice and return the number of bytes initialized.
    /// ### Note:
    /// - The predicate function should ensure that it does not write beyond the arena capacity, as this will lead to undefined behavior.
    /// - The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
    ///
    /// ### Panics
    /// Panics if the returned size exceeds the arena capacity.
    pub fn initialize<P>(self, predicate: P) -> TempBuffer<'a, 'arena, Initialized>
    where
        P: FnOnce(&mut [MaybeUninit<u8>]) -> usize,
    {
        let size = predicate(self.arena.as_mut().unwrap());
        if size > self.arena.as_ref().unwrap().len() {
            panic!("Returned size exceeds arena capacity");
        }
        TempBuffer {
            arena: self.arena,
            state: Initialized { size },
        }
    }

    /// Borrows the all available arenna space as an immutable slice. The caller is responsible for ensuring that the data in the returned slice is properly initialized before use.
    #[inline]
    pub fn as_slice(&self) -> &[MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_ref().unwrap_unchecked() }
    }

    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { *self.arena.as_mut().unwrap_unchecked() }
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
    pub fn accuire_mut(self, n: usize) -> &'a mut [MaybeUninit<u8>] {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { self.arena.take().unwrap_unchecked() };
        let (used, remainig) = buffer.split_at_mut(n);
        self.arena.replace(remainig);
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
    pub unsafe fn accuire_mut_unchecked(self, n: usize) -> &'a mut [u8] {
        unsafe { core::mem::transmute(self.accuire_mut(n)) }
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// ### Note:
    /// - The the caller is responsible for ensuring that the data in the returned slice is properly initialized.
    ///
    /// ### Panics
    /// Panics if the returned size exceeds the arena capacity.
    ///
    /// The remaining arena will be adjusted accordingly.
    pub fn accuire_mut_with_init<P>(mut self, predicate: P) -> &'a mut [u8]
    where
        P: FnOnce(&mut [MaybeUninit<u8>]) -> usize,
    {
        let size = predicate(self.as_slice_mut());
        unsafe { self.accuire_mut_unchecked(size) }
    }
}

impl<'a, 'arena> TempBuffer<'a, 'arena, Initialized> {
    /// Borrows the all available arena space as an immutable slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: self.arena is guaranteed to be Some
        let s: &[MaybeUninit<u8>] = unsafe { self.arena.as_ref().unwrap_unchecked() };
        unsafe { core::mem::transmute(&s[..self.state.size]) }
    }

    /// Borrows the all available TempBuffer space as a mutable slice.
    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        // SAFETY: self.arena is guaranteed to be Some
        let s: &mut [MaybeUninit<u8>] = unsafe { self.arena.as_mut().unwrap_unchecked() };
        unsafe { core::mem::transmute(&mut s[..self.state.size]) }
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the initialized data size.
    ///
    pub fn accuire_mut(self, n: usize) -> &'a mut [u8] {
        if n > self.state.size {
            panic!("Requested size exceeds initialized data");
        }

        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { self.arena.take().unwrap_unchecked() };

        // SAFETY: The n value is guaranteed to be less than or equal to the initialized size
        // which in turn less than or equal to the buffer size.
        let (used, remainig) = unsafe { buffer.split_at_mut_unchecked(n) };
        self.arena.replace(remainig);

        //SAFETY: The used block is garanteed to be initialized because the state is Initialized
        //which means that the returned slice is. initialize atleast with zeros.
        unsafe { core::mem::transmute(used) }
    }

    /// Takes a front portion of the arena of size `used_size` and returns it as a mutable slice.
    /// The remaining arena will be adjusted accordingly.
    pub fn accuire_mut_with_init<P>(mut self, predicate: P) -> &'a mut [u8]
    where
        P: FnOnce(&mut [u8]) -> usize,
    {
        let size = predicate(self.as_slice_mut());
        self.accuire_mut(size)
    }
}

impl<'a> From<&'a mut [u8]> for HeadArenaBuffer<'a> {
    fn from(buffer: &'a mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'a> From<&'a mut [MaybeUninit<u8>]> for HeadArenaBuffer<'a> {
    fn from(buffer: &'a mut [MaybeUninit<u8>]) -> Self {
        Self::from_uninitialized(buffer)
    }
}

impl<'a, const N: usize> From<&'a mut [u8; N]> for HeadArenaBuffer<'a> {
    fn from(buffer: &'a mut [u8; N]) -> Self {
        Self::new(&mut buffer[..])
    }
}

impl<'a, const N: usize> From<&'a mut [MaybeUninit<u8>; N]> for HeadArenaBuffer<'a> {
    fn from(buffer: &'a mut [MaybeUninit<u8>; N]) -> Self {
        Self::from_uninitialized(&mut buffer[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_from_initialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::new(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_init_from_uninitialized() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::from_uninitialized(unsafe {
            core::mem::transmute::<_, &mut [MaybeUninit<u8>; 5]>(&mut data)
        });
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::new(&mut data[..]);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_from_uninitialized_slice() {
        let data: [u8; 5] = [1, 2, 3, 4, 5];
        let mut uninit_data = unsafe { core::mem::transmute::<_, [MaybeUninit<u8>; 5]>(data) };

        let head_arena = HeadArenaBuffer::from_uninitialized(&mut uninit_data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_array() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::from(&mut data);
        assert_eq!(head_arena.len(), 5);
    }

    #[test]
    fn test_empty_buffer() {
        let mut data = [];
        let head_arena = HeadArenaBuffer::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_as_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::new(&mut data);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let slice = unsafe { head_arena.borrow_mut_slice_unchecked() };
        slice[0] = 10;
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_take_front_partial() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArenaBuffer::new(&mut data);

        let detached = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(detached, &[1, 2]);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[3, 4, 5]);
    }

    #[test]
    fn test_take_front_all() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArenaBuffer::new(&mut data);

        let detached = unsafe { head_arena.take_front_mut_unchecked(3) };
        assert_eq!(detached, &[1, 2, 3]);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_multiple_take_fronts() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let first = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(first, &[1, 2]);
        assert_eq!(head_arena.len(), 4);

        let second = unsafe { head_arena.take_front_mut_unchecked(2) };
        assert_eq!(second, &[3, 4]);
        assert_eq!(head_arena.len(), 2);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[5, 6]);
    }

    #[test]
    #[should_panic]
    fn test_take_front_too_large() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        head_arena.take_front_mut(4);
    }

    #[test]
    fn test_take_front_zero() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArenaBuffer::new(&mut data);

        let detached = head_arena.take_front_mut(0);
        assert_eq!(detached.len(), 0);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[1, 2, 3]);
    }

    #[test]
    fn test_usage_in_a_loop() {
        const PART_SIZE: usize = 3;
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut head_arena = HeadArenaBuffer::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = unsafe { head_arena.take_front_mut_unchecked(to_detach) };
            detached_parts.push(detached);
        }

        for detached in detached_parts {
            // Process detached data (here we just print it)
            println!("Detached: {:?}", detached);
        }
    }

    #[test]
    fn test_no_borrowing_glue() {
        const PART_SIZE: usize = 3;

        let mut data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut head_arena = HeadArenaBuffer::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            // Mutate buffer only within detach to avoid borrowing issues
            unsafe {
                head_arena.borrow_mut_slice_unchecked()[0] += 1;
            } // Example mutation
            let detached = unsafe { head_arena.take_front_mut_unchecked(to_detach) };
            if !head_arena.is_empty() {
                unsafe {
                    head_arena.borrow_mut_slice_unchecked()[0] += 1;
                } // Example mutation
            }
            detached_parts.push(detached);
        }

        for detached in detached_parts {
            // Process detached data (here we just print it)
            detached[0] -= 1; // Example mutation on detached

            println!("Detached: {:?}", detached);
        }
    }

    /// Test take_remaining method
    #[test]
    fn test_take_remaining() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArenaBuffer::new(&mut data);
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining_mut()) };
        assert_eq!(remaining, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn subsequent_take_remaining() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let _ = head_arena.take_front_mut(2); // Detach first 2 bytes
        let remaining = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining_mut()) };
        assert_eq!(remaining, &[3, 4, 5]);
    }

    #[test]
    fn test_nested_arena() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let _ = head_arena.take_front_mut(2); // Detach first 2 bytes

        let mut nested_arena = head_arena.nested();
        assert_eq!(unsafe { nested_arena.as_slice_unchecked() }, &[3, 4, 5, 6]);

        let detached = unsafe { nested_arena.take_front_mut_unchecked(3) };
        assert_eq!(detached, &[3, 4, 5]);
        assert_eq!(unsafe { nested_arena.as_slice_unchecked() }, &[6]);
    }

    #[test]
    fn test_nested_arena_could_mutate_delegated_memory_of_original() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let _ = head_arena.take_front_mut(2); // Detach first 2 bytes

        {
            let mut nested_arena = head_arena.nested();
            unsafe {
                nested_arena.take_front_mut_unchecked(2)[0] = 10; // Mutate original arena's buffer
            }
            assert_eq!(unsafe { nested_arena.as_slice_unchecked() }, &[5, 6]);
        }

        let detached = unsafe { core::mem::transmute::<_, &mut [u8]>(head_arena.take_remaining_mut()) };
        assert_eq!(detached, &[10, 4, 5, 6]);
    }

    #[test]
    fn test_nested_arena_returns_space_to_original_when_goes_out_of_scope() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut head_arena = HeadArenaBuffer::new(&mut data);
        let _ = head_arena.take_front_mut(2); // Detach first 2 bytes

        {
            let mut nested_arena = head_arena.nested();
            assert_eq!(nested_arena.len(), 4);
            assert_eq!(unsafe { nested_arena.as_slice_unchecked() }, &[3, 4, 5, 6]);

            let detached = unsafe { nested_arena.take_front_mut_unchecked(3) };
            assert_eq!(detached, &[3, 4, 5]);
            assert_eq!(unsafe { nested_arena.as_slice_unchecked() }, &[6]);
        }

        assert_eq!(head_arena.len(), 4);
        assert_eq!(unsafe { head_arena.as_slice_unchecked() }, &[3, 4, 5, 6]);
    }
}
