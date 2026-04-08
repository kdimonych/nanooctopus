use core::mem::MaybeUninit;

/// A simple serializer that writes JSON data into a fixed-size buffer.
/// Errors if the buffer is full.
pub enum SerializerError {
    /// The buffer is full and cannot accommodate more data.
    BufferFull,
}

/// A structure that serializes Rust values as JSON into a buffer.
pub struct SliceView<'buf> {
    buffer: &'buf mut [MaybeUninit<u8>],
    current_length: usize,
}

impl<'buf> SliceView<'buf> {
    /// Create a new `Serializer`

    pub fn new(buffer: &'buf mut [MaybeUninit<u8>]) -> Self {
        SliceView {
            buffer,
            current_length: 0,
        }
    }

    /// Push a byte into the buffer
    pub fn push(&mut self, c: u8) -> Result<(), SerializerError> {
        if self.current_length < self.buffer.len() {
            unsafe { self.push_unchecked(c) };
            Ok(())
        } else {
            Err(SerializerError::BufferFull)
        }
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.current_length == 0
    }

    /// Returns the current length of the buffer.
    pub fn len(&self) -> usize {
        self.current_length
    }

    /// Returns the total capacity of the buffer.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the remaining capacity of the buffer.
    pub fn remaining_capacity(&self) -> usize {
        self.buffer.len() - self.current_length
    }

    /// Push a byte into the buffer without checking capacity.
    unsafe fn push_unchecked(&mut self, c: u8) {
        self.buffer[self.current_length].write(c);
        self.current_length += 1;
    }

    /// Extend the buffer with a slice
    pub fn extend_from_slice(&mut self, other: &[u8]) -> Result<(), SerializerError> {
        if self.current_length + other.len() > self.buffer.len() {
            // won't fit in the buf; don't modify anything and return an error
            Err(SerializerError::BufferFull)
        } else {
            for c in other {
                unsafe { self.push_unchecked(*c) };
            }
            Ok(())
        }
    }

    /// Extend the buffer with a string slice
    #[inline]
    pub fn extend_from_str(&mut self, s: &str) -> Result<(), SerializerError> {
        self.extend_from_slice(s.as_bytes())
    }

    /// Allocate a slice of the given size from the buffer without checking capacity.
    #[must_use]
    unsafe fn allocate_unchecked(&mut self, size: usize) -> &mut [u8] {
        let start = self.current_length;
        self.current_length += size;
        unsafe { core::mem::transmute(&mut self.buffer[start..start + size]) }
    }

    /// Try to allocate a slice of the given size from the buffer.
    pub fn try_allocate(&mut self, size: usize) -> Result<&mut [u8], SerializerError> {
        if self.current_length + size > self.buffer.len() {
            // won't fit in the buf; don't modify anything and return an error
            Err(SerializerError::BufferFull)
        } else {
            Ok(unsafe { self.allocate_unchecked(size) })
        }
    }

    /// Returns a mutable slice to the remaining buffer space.
    #[must_use]
    pub fn remaining_buffer(&mut self) -> &mut [u8] {
        self.current_length = self.capacity();
        unsafe { core::mem::transmute(&mut self.buffer[self.current_length..]) }
    }

    /// Modify the inner buffer part that is initialized.
    pub fn modify_inner<Modifier>(&mut self, modifier: Modifier)
    where
        Modifier: FnOnce(&mut [u8]),
    {
        modifier(unsafe { core::mem::transmute(&mut self.buffer[..self.current_length]) });
    }

    /// Fill the remaining buffer space using the provided filler function.
    ///
    /// Returns an error if the filler function fails.
    /// # Panics
    /// Panics if the filler function writes beyond the buffer capacity.
    /// ## Note:
    ///
    /// The filler function should write the body content into the provided slice
    /// and return the number of bytes written.
    pub fn fill_with<Filler, Error>(&mut self, filler: Filler) -> Result<usize, Error>
    where
        Filler: FnOnce(&mut [MaybeUninit<u8>]) -> Result<usize, Error>,
    {
        let capacity = self.capacity();
        let filled_size = filler(&mut self.buffer[self.current_length..])?;
        if filled_size + self.current_length > capacity {
            panic!("Filler function wrote beyond buffer capacity");
        }
        self.current_length += filled_size;
        Ok(filled_size)
    }

    /// Resets the buffer to an empty state without modifying the underlying memory.
    pub fn reset(&mut self) {
        self.current_length = 0;
    }

    /// Returns a slice of the initialized portion of the buffer.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::mem::transmute(&self.buffer[..self.current_length]) }
    }

    /// Returns the serialized data as a byte slice.
    pub fn take_slice(self) -> &'buf [u8] {
        unsafe { core::mem::transmute(&self.buffer[..self.current_length]) }
    }
}
