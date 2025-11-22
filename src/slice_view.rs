/// A simple serializer that writes JSON data into a fixed-size buffer.
/// Errors if the buffer is full.
pub enum SerializerError {
    /// The buffer is full and cannot accommodate more data.
    BufferFull,
}

/// A structure that serializes Rust values as JSON into a buffer.
pub struct SliceView<'a> {
    buf: &'a mut [u8],
    current_length: usize,
}

impl<'a> SliceView<'a> {
    /// Create a new `Serializer`
    pub fn new(buf: &'a mut [u8]) -> Self {
        SliceView {
            buf,
            current_length: 0,
        }
    }

    /// Push a byte into the buffer
    pub fn push(&mut self, c: u8) -> Result<(), SerializerError> {
        if self.current_length < self.buf.len() {
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
        self.buf.len()
    }

    /// Returns the remaining capacity of the buffer.
    pub fn remaining_capacity(&self) -> usize {
        self.buf.len() - self.current_length
    }

    /// Push a byte into the buffer without checking capacity.
    unsafe fn push_unchecked(&mut self, c: u8) {
        self.buf[self.current_length] = c;
        self.current_length += 1;
    }

    /// Extend the buffer with a slice
    pub fn extend_from_slice(&mut self, other: &[u8]) -> Result<(), SerializerError> {
        if self.current_length + other.len() > self.buf.len() {
            // won't fit in the buf; don't modify anything and return an error
            Err(SerializerError::BufferFull)
        } else {
            for c in other {
                unsafe { self.push_unchecked(*c) };
            }
            Ok(())
        }
    }

    /// Allocate a slice of the given size from the buffer without checking capacity.
    #[must_use]
    unsafe fn allocate_unchecked(&mut self, size: usize) -> &mut [u8] {
        let start = self.current_length;
        self.current_length += size;
        &mut self.buf[start..start + size]
    }

    /// Try to allocate a slice of the given size from the buffer.
    pub fn try_allocate(&mut self, size: usize) -> Result<&mut [u8], SerializerError> {
        if self.current_length + size > self.buf.len() {
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
        &mut self.buf[self.current_length..]
    }

    /// Modify the inner buffer part that is initialized.
    pub fn modify_inner<Modifier>(&mut self, modifier: Modifier)
    where
        Modifier: FnOnce(&mut [u8]),
    {
        modifier(&mut self.buf[..self.current_length]);
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
        Filler: FnOnce(&mut [u8]) -> Result<usize, Error>,
    {
        let capacity = self.capacity();
        let filled_size = filler(&mut self.buf[self.current_length..])?;
        if filled_size + self.current_length > capacity {
            panic!("Filler function wrote beyond buffer capacity");
        }
        self.current_length += filled_size;
        Ok(filled_size)
    }

    /// Returns the serialized data as a byte slice.
    pub fn as_slice(self) -> &'a [u8] {
        &self.buf[..self.current_length]
    }
}
