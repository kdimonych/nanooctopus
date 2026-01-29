/// A simple bump allocator that allows detaching front portions of a buffer.
/// This is useful for scenarios where data is read into a buffer
/// and then parts of it need to be processed or handed off without copying.
/// The remaining buffer can still be used for further allocations.
///
/// ### Note
/// The `HeadArena` does not track initialization of data; it simply manages
/// the buffer space.
pub struct HeadArena<'a> {
    arena: Option<&'a mut [u8]>,
}

impl<'a> HeadArena<'a> {
    /// Creates a new HeadArena allocator over the provided arena buffer.
    pub const fn new(arena: &'a mut [u8]) -> Self {
        Self { arena: Some(arena) }
    }

    /// Returns the length of free arena.
    pub const fn len(&self) -> usize {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_ref().unwrap_unchecked().len() }
    }

    /// Returns true if the arena is empty.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a slice to the entire arena.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.as_ref().unwrap_unchecked() }
    }

    /// Returns a mutable slice to the entire arena.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: self.buffer is guaranteed to be Some
        unsafe { self.arena.as_mut().unwrap_unchecked() }
    }

    /// Takes a front portion of the arena of size `used_size` and returns it.
    /// The remaining arena will be adjusted accordingly.
    ///
    /// ### Note: The remaining arena will be treated as uninitialized even if there were some data.
    /// This primitive doesn't keep track whether data were actually initialized or not within front n bytes.
    ///
    /// ### Panics
    /// Panics if `n` is greater than the current buffer size.
    ///
    pub fn take_front(&mut self, n: usize) -> &'a mut [u8] {
        // SAFETY: self.arena is guaranteed to be Some
        let buffer = unsafe { self.arena.take().unwrap_unchecked() };
        let (used, remainig) = buffer.split_at_mut(n);
        self.arena.replace(remainig);
        used
    }

    /// Takes the remaining arena space. This consumes the HeadArena.
    pub fn take_remaining(mut self) -> &'a mut [u8] {
        // SAFETY: self.arena is guaranteed to be Some
        unsafe { self.arena.take().unwrap_unchecked() }
    }
}

impl<'a> From<&'a mut [u8]> for HeadArena<'a> {
    fn from(buffer: &'a mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'a, const N: usize> From<&'a mut [u8; N]> for HeadArena<'a> {
    fn from(buffer: &'a mut [u8; N]) -> Self {
        Self::new(&mut buffer[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_head_arena() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.len(), 5);
        assert!(!head_arena.is_empty());
    }

    #[test]
    fn test_from_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::from(&mut data[..]);
        assert_eq!(head_arena.len(), 5);
        assert_eq!(head_arena.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_from_array() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::from(&mut data);
        assert_eq!(head_arena.len(), 5);
    }

    #[test]
    fn test_empty_buffer() {
        let mut data = [];
        let head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_as_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let head_arena = HeadArena::new(&mut data);
        assert_eq!(head_arena.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);
        let slice = head_arena.as_mut_slice();
        slice[0] = 10;
        assert_eq!(head_arena.as_slice(), &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_take_front_partial() {
        let mut data = [1, 2, 3, 4, 5];
        let mut head_arena = HeadArena::new(&mut data);

        let detached = head_arena.take_front(2);
        assert_eq!(detached, &[1, 2]);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(head_arena.as_slice(), &[3, 4, 5]);
    }

    #[test]
    fn test_take_front_all() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArena::new(&mut data);

        let detached = head_arena.take_front(3);
        assert_eq!(detached, &[1, 2, 3]);
        assert_eq!(head_arena.len(), 0);
        assert!(head_arena.is_empty());
    }

    #[test]
    fn test_multiple_take_fronts() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut head_arena = HeadArena::new(&mut data);
        let first = head_arena.take_front(2);
        assert_eq!(first, &[1, 2]);
        assert_eq!(head_arena.len(), 4);

        let second = head_arena.take_front(2);
        assert_eq!(second, &[3, 4]);
        assert_eq!(head_arena.len(), 2);
        assert_eq!(head_arena.as_slice(), &[5, 6]);
    }

    #[test]
    #[should_panic]
    fn test_take_front_too_large() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArena::new(&mut data);
        head_arena.take_front(4);
    }

    #[test]
    fn test_take_front_zero() {
        let mut data = [1, 2, 3];
        let mut head_arena = HeadArena::new(&mut data);

        let detached = head_arena.take_front(0);
        assert_eq!(detached.len(), 0);
        assert_eq!(head_arena.len(), 3);
        assert_eq!(head_arena.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_usage_in_a_loop() {
        const PART_SIZE: usize = 3;
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut head_arena = HeadArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            let detached = head_arena.take_front(to_detach);
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
        let mut head_arena = HeadArena::new(&mut data);

        let mut detached_parts = Vec::new();

        while !head_arena.is_empty() {
            let to_detach = core::cmp::min(head_arena.len(), PART_SIZE);
            // Mutate buffer only within detach to avoid borrowing issues
            head_arena.as_mut_slice()[0] += 1; // Example mutation
            let detached = head_arena.take_front(to_detach);
            if !head_arena.is_empty() {
                head_arena.as_mut_slice()[0] += 1; // Example mutation
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
        let head_arena = HeadArena::new(&mut data);
        let remaining = head_arena.take_remaining();
        assert_eq!(remaining, &[1, 2, 3, 4, 5]);
    }
}
