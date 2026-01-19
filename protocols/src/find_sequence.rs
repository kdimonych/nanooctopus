pub struct FindSequence<'a, Item> {
    sequence: &'a [Item],
    position: usize,
}

impl<'a, Item> FindSequence<'a, Item> {
    /// Create a new FindSequence for the given byte sequence
    pub fn new(sequence: &'a [Item]) -> Self {
        Self {
            sequence,
            position: 0,
        }
    }

    /// Push a byte into the sequence finder
    pub fn push_byte(&mut self, byte: Item) -> bool
    where
        Item: PartialEq + Copy,
    {
        // Safety: position is always less than sequence length
        if byte == unsafe { *self.sequence.get_unchecked(self.position) } {
            self.position += 1;
            if self.position == self.sequence.len() {
                self.position = 0;
                return true;
            }
        } else {
            self.position = 0;
        }
        false
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_find_sequence() {
        let sequence = b"\r\n";
        let mut finder = FindSequence::new(sequence);
        let data = b"Hello, World!\r\nThis is a test.\r\n";
        let mut found_positions = Vec::new();
        for (i, &byte) in data.iter().enumerate() {
            if finder.push_byte(byte) {
                found_positions.push(i + 1 - sequence.len());
            }
        }
        assert_eq!(found_positions, vec![13, 30]);
    }
}
