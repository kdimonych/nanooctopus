/// A buffer that allows detaching used portions while retaining the remaining unused portion.
pub struct DetachableBuffer<'a> {
    buffer: Option<&'a mut [u8]>,
}

impl<'a> DetachableBuffer<'a> {
    /// Creates a new DetachableBuffer wrapping the provided buffer.
    pub const fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer: Some(buffer),
        }
    }

    /// Returns the length of the current buffer.
    pub const fn len(&self) -> usize {
        // SAFETY: self.buffer is guaranteed to be Some
        unsafe { self.buffer.as_ref().unwrap_unchecked().len() }
    }

    /// Returns true if the buffer is empty.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a slice to the entire buffer.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: self.buffer is guaranteed to be Some
        unsafe { self.buffer.as_ref().unwrap_unchecked() }
    }

    /// Returns a mutable slice to the entire buffer.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: self.buffer is guaranteed to be Some
        unsafe { self.buffer.as_mut().unwrap_unchecked() }
    }

    /// Detaches a portion of the buffer of size `used_size` and returns it.
    /// The remaining buffer is kept for future use.
    ///
    /// ### Note: The remaining buffer will be treated as empty even if there were some data.
    /// This primitive doesn't track used vs unused data in the remaining buffer as long as its integrity after detach.
    ///
    /// ### Panics
    /// Panics if `used_size` is greater than the current buffer size.
    ///
    pub fn detach(&mut self, used_size: usize) -> &'a mut [u8] {
        // SAFETY: self.buffer is guaranteed to be Some
        let buffer = unsafe { self.buffer.take().unwrap_unchecked() };
        let (used, remainig) = buffer.split_at_mut(used_size);
        self.buffer.replace(remainig);
        used
    }

    /// Takes the remaining buffer, leaving this DetachableBuffer empty.
    pub fn take_remaining(mut self) -> &'a mut [u8] {
        // SAFETY: self.buffer is guaranteed to be Some
        unsafe { self.buffer.take().unwrap_unchecked() }
    }
}

impl<'a> From<&'a mut [u8]> for DetachableBuffer<'a> {
    fn from(buffer: &'a mut [u8]) -> Self {
        Self::new(buffer)
    }
}

impl<'a, const N: usize> From<&'a mut [u8; N]> for DetachableBuffer<'a> {
    fn from(buffer: &'a mut [u8; N]) -> Self {
        Self::new(&mut buffer[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let mut data = [1, 2, 3, 4, 5];
        let buffer = DetachableBuffer::new(&mut data);
        assert_eq!(buffer.len(), 5);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_from_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let buffer = DetachableBuffer::from(&mut data[..]);
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_from_array() {
        let mut data = [1, 2, 3, 4, 5];
        let buffer = DetachableBuffer::from(&mut data);
        assert_eq!(buffer.len(), 5);
    }

    #[test]
    fn test_empty_buffer() {
        let mut data = [];
        let buffer = DetachableBuffer::new(&mut data);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_as_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let buffer = DetachableBuffer::new(&mut data);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_as_mut_slice() {
        let mut data = [1, 2, 3, 4, 5];
        let mut buffer = DetachableBuffer::new(&mut data);
        let slice = buffer.as_mut_slice();
        slice[0] = 10;
        assert_eq!(buffer.as_slice(), &[10, 2, 3, 4, 5]);
    }

    #[test]
    fn test_detach_partial() {
        let mut data = [1, 2, 3, 4, 5];
        let mut buffer = DetachableBuffer::new(&mut data);

        let detached = buffer.detach(2);
        assert_eq!(detached, &[1, 2]);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.as_slice(), &[3, 4, 5]);
    }

    #[test]
    fn test_detach_all() {
        let mut data = [1, 2, 3];
        let mut buffer = DetachableBuffer::new(&mut data);

        let detached = buffer.detach(3);
        assert_eq!(detached, &[1, 2, 3]);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_multiple_detaches() {
        let mut data = [1, 2, 3, 4, 5, 6];
        let mut buffer = DetachableBuffer::new(&mut data);

        let first = buffer.detach(2);
        assert_eq!(first, &[1, 2]);
        assert_eq!(buffer.len(), 4);

        let second = buffer.detach(2);
        assert_eq!(second, &[3, 4]);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.as_slice(), &[5, 6]);
    }

    #[test]
    #[should_panic]
    fn test_detach_too_large() {
        let mut data = [1, 2, 3];
        let mut buffer = DetachableBuffer::new(&mut data);
        buffer.detach(4);
    }

    #[test]
    fn test_detach_zero() {
        let mut data = [1, 2, 3];
        let mut buffer = DetachableBuffer::new(&mut data);

        let detached = buffer.detach(0);
        assert_eq!(detached.len(), 0);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_usage_in_a_loop() {
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut buffer = DetachableBuffer::new(&mut data);

        let mut detached_parts = Vec::new();

        while !buffer.is_empty() {
            let to_detach = if buffer.len() >= 3 { 3 } else { buffer.len() };
            let detached = buffer.detach(to_detach);
            detached_parts.push(detached);
        }

        for detached in detached_parts {
            // Process detached data (here we just print it)
            println!("Detached: {:?}", detached);
        }
    }

    #[test]
    fn test_no_borrowing_glue() {
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut buffer = DetachableBuffer::new(&mut data);

        let mut detached_parts = Vec::new();

        while !buffer.is_empty() {
            let to_detach = if buffer.len() >= 3 { 3 } else { buffer.len() };
            // Mutate buffer only within detach to avoid borrowing issues
            buffer.as_mut_slice()[0] += 1; // Example mutation
            let detached = buffer.detach(to_detach);
            if !buffer.is_empty() {
                buffer.as_mut_slice()[0] += 1; // Example mutation
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
        let buffer = DetachableBuffer::new(&mut data);
        let remaining = buffer.take_remaining();
        assert_eq!(remaining, &[1, 2, 3, 4, 5]);
    }
}
