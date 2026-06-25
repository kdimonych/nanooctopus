#![allow(dead_code)]

/// A stateful masker for applying a WebSocket mask key across payload bytes.
pub struct PayloadMasker {
    offset: usize,
    mask_key: [u8; 4],
}

impl PayloadMasker {
    /// Create a new PayloadMasker with the given mask key.
    ///
    /// ### Parameters
    /// - `mask_key`: The 32-bit mask key used to generate the mask bytes.
    ///
    /// ### Returns
    /// A new PayloadMasker instance initialized with the provided mask key.
    #[inline]
    pub const fn new(mask_key: u32) -> Self {
        Self {
            mask_key: mask_key.to_be_bytes(),
            offset: 0,
        }
    }

    /// Create a new PayloadMasker with the given mask key and initial offset.
    ///
    /// ### Parameters
    /// - `mask_key`: The 32-bit mask key used to generate the mask bytes.
    /// - `offset`: The initial offset into the mask key.
    ///
    /// ### Returns
    /// A new PayloadMasker instance initialized with the provided mask key and offset.
    #[inline]
    pub const fn new_with_offset(mask_key: u32, offset: usize) -> Self {
        Self {
            mask_key: mask_key.to_be_bytes(),
            offset,
        }
    }

    /// Get the next mask byte, advancing the mask offset by 1.
    ///
    /// ### Returns
    /// The next mask byte to be applied to the payload data.
    #[inline]
    pub const fn next_mask_byte(&mut self) -> u8 {
        let byte = self.mask_key[self.offset % 4];
        self.offset += 1;
        byte
    }

    /// Mask a single byte, advancing the mask offset by 1.
    ///
    /// ### Parameters
    /// - `byte`: The byte to be masked.
    ///
    /// ### Returns
    /// The masked byte.
    #[inline]
    pub const fn mask_byte(&mut self, byte: u8) -> u8 {
        byte ^ self.next_mask_byte()
    }

    /// Mask the given buffer in-place, advancing the mask offset accordingly.
    ///
    /// ### Parameters
    /// - `buf`: The buffer to be masked. This buffer will be modified in-place.
    ///
    /// ### Note:
    /// This is a simple implementation that processes the buffer byte-by-byte, which can be inefficient for
    /// large buffers that are greater than 2 machine words. For better performance on larger buffers, consider
    /// using the `mask_chank` method.
    pub fn mask_small_chank(&mut self, buf: &mut [u8]) {
        for byte in buf {
            *byte = self.mask_byte(*byte);
        }
    }

    /// Mask the given buffer in-place, advancing the mask offset accordingly.
    ///
    /// ### Parameters
    /// - `buf`: The buffer to be masked. This buffer will be modified in-place.
    ///
    /// ### Note:
    /// This is an optimized version of the masking function that can take advantage of word length and hence be much
    /// faster for large buffers, but may have more overhead for small buffers. For small buffers that less than 3
    /// machine words, the `mask_small_chank` method may be more efficient.
    pub fn mask_chank(&mut self, buf: &mut [u8]) {
        self.mask_chank_impl(buf, |aligned, mask| {
            for chunk in aligned {
                *chunk ^= mask;
            }
        });
    }

    /// Mask the given buffer in-place, advancing the mask offset accordingly.
    ///
    /// ### Parameters
    /// - `buf`: The buffer to be masked. This buffer will be modified in-place.
    ///
    /// ### Note:
    /// This is an optimized version of the masking function that can take advantage of SIMD instructions on supported platforms
    /// and can be much faster for large buffers, but may have more overhead for small buffers. For small buffers that less than 2
    /// machine words, the `mask_small_chank` method may be more efficient.
    pub fn mask_chank_simd(&mut self, buf: &mut [u8]) {
        self.mask_chank_impl(buf, |aligned, mask| {
            let (simd_chunk, chunks) = aligned.as_chunks_mut();
            for chunk4 in simd_chunk {
                // This pattern can be optimized to a single SIMD instruction on targets that support it,
                // and is still efficient on targets that don't.
                let [a, b, c, d] = chunk4;
                *a ^= mask;
                *b ^= mask;
                *c ^= mask;
                *d ^= mask;
            }

            for chunk in chunks.iter_mut() {
                *chunk ^= mask;
            }
        });
    }

    fn mask_chank_impl<F: FnMut(&mut [usize], usize)>(&mut self, buf: &mut [u8], mut mask_chank_f: F) {
        // Align the buffer to usize boundaries, so we can process the aligned part in chunks of usize.
        let (prefix, aligned, suffix) = unsafe { buf.align_to_mut::<usize>() };

        // Do the prefix byte-by-byte regular way
        self.mask_small_chank(prefix);

        // Align the mask to the aligned part of data, and apply it by chanks
        let mask_32 = rotate(self.mask_key, self.offset);
        let mask = populate_to_usize(mask_32);

        mask_chank_f(aligned, mask);

        // Do the suffix byte-by-byte regular way, and update the offset to account for all the bytes we have processed so far (including the prefix and the aligned part).
        self.offset = prefix.len() + core::mem::size_of_val(aligned);
        self.mask_small_chank(suffix);
    }
}

#[inline]
const fn rotate(mask: [u8; 4], n: usize) -> u32 {
    // This code is equivalent to rotating the mask left by n bytes, but it avoids the need for a loop and is
    // more efficient.
    // This expresses the operation as a u32 bit-rotate and lets LLVM lower it to a native rotate instruction
    // on targets that have one, or to the best scalar sequence on targets that do not.
    let shift = ((n & 0b11) as u32) * 8;
    u32::from_le_bytes(mask).rotate_right(shift)
}

#[cfg(target_pointer_width = "32")]
const fn populate_to_usize(mask: u32) -> usize {
    mask as usize
}

#[cfg(target_pointer_width = "64")]
const fn populate_to_usize(mask: u32) -> usize {
    // This code is equivalent to repeating the mask to fill a usize, but it avoids the need for a loop and is
    // more efficient.
    // This expresses the operation as a single multiplication, which LLVM can optimize to a single instruction
    // on targets that support it.
    let mask = mask as usize;
    mask | (mask << 32)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn create_test_data<const N: usize>() -> [u8; N] {
        core::array::from_fn(|i| {
            // Pseudo random
            (i.wrapping_mul(37).wrapping_add(13) % 256) as u8
        })
    }

    #[test]
    fn test_mask_chank() {
        let mut masker = PayloadMasker::new(0x01020304);
        let mut data = [0u8; 8];
        masker.mask_chank(&mut data);
        assert_eq!(data, [0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_mask_small_chank() {
        let mut masker = PayloadMasker::new(0x01020304);
        let mut data = [0u8; 8];
        masker.mask_small_chank(&mut data);
        assert_eq!(data, [0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_mask_chank_simd() {
        let mut masker = PayloadMasker::new(0x01020304);
        let mut data = [0u8; 8];
        masker.mask_chank_simd(&mut data);
        assert_eq!(data, [0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_mask_chank_is_reversible() {
        const MASK_KEY: u32 = 0x01020304;

        let mut data = [0u8; 11];
        PayloadMasker::new(MASK_KEY).mask_chank(&mut data);
        PayloadMasker::new(MASK_KEY).mask_chank(&mut data);
        assert_eq!(data, [0u8; 11]);
    }

    #[test]
    fn test_mask_small_chank_is_reversible() {
        const MASK_KEY: u32 = 0x01020304;

        let mut data = [0u8; 11];
        PayloadMasker::new(MASK_KEY).mask_small_chank(&mut data);
        PayloadMasker::new(MASK_KEY).mask_small_chank(&mut data);
        assert_eq!(data, [0u8; 11]);
    }

    #[test]
    fn test_mask_chank_simd_is_reversible() {
        const MASK_KEY: u32 = 0x01020304;

        let mut data = [0u8; 11];
        PayloadMasker::new(MASK_KEY).mask_chank_simd(&mut data);
        PayloadMasker::new(MASK_KEY).mask_chank_simd(&mut data);
        assert_eq!(data, [0u8; 11]);
    }

    #[test]
    fn test_optimized_give_the_same_result() {
        const MASK_KEY: u32 = 0x01020304;
        let data: [u8; 11] = create_test_data();

        let mut masker = PayloadMasker::new(MASK_KEY);
        let mut data1 = data;
        masker.mask_small_chank(&mut data1);

        let mut masker = PayloadMasker::new(MASK_KEY);
        let mut data2: [u8; 11] = data;
        masker.mask_chank(&mut data2);

        assert_eq!(data1, data2);

        let mut masker = PayloadMasker::new(MASK_KEY);
        let mut data3 = data;
        masker.mask_chank_simd(&mut data3);

        assert_eq!(data1, data3);
    }

    #[test]
    fn test_mask_chank_and_mask_chank_equal_for_any_offset() {
        const MASK_KEY: u32 = 0x01020304;

        let original_data: [u8; 255] = create_test_data();

        for offset in 0..original_data.len() {
            let mut data1 = original_data.clone();
            let mut data2 = original_data.clone();
            let mut data3 = original_data.clone();

            let data1 = &mut data1[offset..];
            let data2 = &mut data2[offset..];
            let data3 = &mut data3[offset..];
            let original_data = &original_data[offset..];

            PayloadMasker::new(MASK_KEY).mask_small_chank(data1);
            PayloadMasker::new(MASK_KEY).mask_chank(data2);
            PayloadMasker::new(MASK_KEY).mask_chank_simd(data3);

            assert_eq!(data1, data2);
            assert_eq!(data1, data3);

            // Demasking should also give the same result, and return us to the original data
            PayloadMasker::new(MASK_KEY).mask_small_chank(data1);
            PayloadMasker::new(MASK_KEY).mask_chank(data2);
            PayloadMasker::new(MASK_KEY).mask_chank_simd(data3);

            assert_eq!(data1, original_data);
            assert_eq!(data2, original_data);
            assert_eq!(data3, original_data);
        }
    }

    #[test]
    fn test_mask_chank_and_mask_chank_equal_for_any_offset_chunked() {
        const MASK_KEY: u32 = 0x11223344;
        const CHUNK_SIZE: usize = 9;

        let data: [u8; 11] = create_test_data();

        for offset in 0..1 {
            let mut data1 = data.clone();
            let mut data2 = data.clone();
            let mut data3 = data.clone();

            let data1_ref = &mut data1[offset..];
            let data2_ref = &mut data2[offset..];
            let data3_ref = &mut data3[offset..];
            let data_ref = &data[offset..];

            let mut mask_it1 = PayloadMasker::new(MASK_KEY);
            for chunk in data1_ref.chunks_mut(CHUNK_SIZE) {
                mask_it1.mask_small_chank(chunk);
            }

            let mut mask_it2 = PayloadMasker::new(MASK_KEY);
            for chunk in data2_ref.chunks_mut(CHUNK_SIZE) {
                mask_it2.mask_chank(chunk);
            }

            let mut mask_it3 = PayloadMasker::new(MASK_KEY);
            for chunk in data3_ref.chunks_mut(CHUNK_SIZE) {
                mask_it3.mask_chank_simd(chunk);
            }

            assert_eq!(data1_ref, data2_ref);
            assert_eq!(data1_ref, data3_ref);

            // Demasking should also give the same result, and return us to the original data
            let mut mask_it1 = PayloadMasker::new(MASK_KEY);
            for chunk in data1_ref.chunks_mut(CHUNK_SIZE) {
                mask_it1.mask_small_chank(chunk);
            }

            let mut mask_it2 = PayloadMasker::new(MASK_KEY);
            for chunk in data2_ref.chunks_mut(CHUNK_SIZE) {
                mask_it2.mask_chank(chunk);
            }

            let mut mask_it3 = PayloadMasker::new(MASK_KEY);
            for chunk in data3_ref.chunks_mut(CHUNK_SIZE) {
                mask_it3.mask_chank_simd(chunk);
            }

            assert_eq!(data1_ref, data_ref);
            assert_eq!(data2_ref, data_ref);
            assert_eq!(data3_ref, data_ref);
        }
    }

    fn benchmark_and_check<const TEST_BLOCK_SIZE: usize, F: FnMut(&mut PayloadMasker, &mut [u8])>(
        mut test_f: F,
    ) -> Duration {
        const MASK_KEY: u32 = 0x01020304;
        /// Must be even to ensure that we the value at the end check is the same as original.
        /// This allow use the performance test as an additional correctness test.
        const EVEN_TRY_COUNT: usize = 100;
        static_assertions::const_assert!(EVEN_TRY_COUNT % 2 == 0);

        let data_original: [u8; TEST_BLOCK_SIZE] = create_test_data();
        let mut data_a = data_original.clone();

        let start_time = std::time::Instant::now();
        for offset in 0..data_a.len() {
            let data_ref = &mut data_a[offset..];

            for _ in 0..EVEN_TRY_COUNT {
                test_f(&mut PayloadMasker::new(MASK_KEY), data_ref);
            }
        }

        std::time::Instant::now() - start_time
    }

    #[test]
    #[ignore]
    fn performance_test_1237b_buffer() {
        const TEST_BLOCK_SIZE: usize = 1237;

        let not_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_small_chank);
        let optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank);
        let simd_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank_simd);

        println!("Not-optimized masking elapsed time: {not_optimized_time:?}");
        println!("Optimized masking elapsed time: {optimized_time:?}");
        println!("SIMD optimized masking elapsed time: {simd_optimized_time:?}");

        // Guarantee that the optimized version is more than 3x faster than the non-optimized version,
        // which is a reasonable threshold for the overhead of the optimized version on small buffers where
        // it doesn't have much opportunity to shine. In practice, we expect it to be much faster than the
        // non-optimized version on larger buffers, and not significantly slower on smaller buffers.
        assert!((optimized_time * 3) < not_optimized_time);

        // Guarantee that the SIMD optimized version is at least 1.5x faster than the optimized version.
        assert!((simd_optimized_time.mul_f32(1.4)) < optimized_time);
    }

    #[test]
    #[ignore]
    fn performance_test_small_512b_buffer() {
        const TEST_BLOCK_SIZE: usize = 512;

        let not_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_small_chank);
        let optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank);
        let simd_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank_simd);

        println!("Not-optimized masking elapsed time: {not_optimized_time:?}");
        println!("Optimized masking elapsed time: {optimized_time:?}");
        println!("SIMD optimized masking elapsed time: {simd_optimized_time:?}");

        // Guarantee that the optimized version is more than 2x faster than the non-optimized version,
        // which is a reasonable threshold for the overhead of the optimized version on small buffers where
        // it doesn't have much opportunity to shine. In practice, we expect it to be much faster than the
        // non-optimized version on larger buffers, and not significantly slower on smaller buffers.
        assert!((optimized_time * 6) < not_optimized_time);

        // The SIMD optimized version can be slower than the optimized version on small buffers.
        assert!(simd_optimized_time.mul_f32(1.1) < optimized_time);
    }

    #[test]
    #[ignore]
    fn performance_test_small_128b_buffer() {
        const TEST_BLOCK_SIZE: usize = 128;

        let not_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_small_chank);
        let optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank);
        let simd_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank_simd);

        println!("Not-optimized masking elapsed time: {not_optimized_time:?}");
        println!("Optimized masking elapsed time: {optimized_time:?}");
        println!("SIMD optimized masking elapsed time: {simd_optimized_time:?}");

        // Guarantee that the optimized version is more than 2x faster than the non-optimized version,
        // which is a reasonable threshold for the overhead of the optimized version on small buffers where
        // it doesn't have much opportunity to shine. In practice, we expect it to be much faster than the
        // non-optimized version on larger buffers, and not significantly slower on smaller buffers.
        assert!((optimized_time * 2) < not_optimized_time);

        // The SIMD optimized version can be slower than the optimized version on small buffers.
        assert!(simd_optimized_time < optimized_time.mul_f32(1.3));
    }

    #[test]
    #[ignore]
    fn performance_test_small_10b_buffer() {
        const TEST_BLOCK_SIZE: usize = 10;

        let not_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_small_chank);
        let optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank);
        let simd_optimized_time = benchmark_and_check::<TEST_BLOCK_SIZE, _>(PayloadMasker::mask_chank_simd);

        println!("Not-optimized masking elapsed time: {not_optimized_time:?}");
        println!("Optimized masking elapsed time: {optimized_time:?}");
        println!("SIMD optimized masking elapsed time: {simd_optimized_time:?}");

        // Guarantee that the optimized version is slower than the non-optimized version,
        // which is a reasonable threshold for the overhead of the optimized version on very small buffers.
        assert!(optimized_time < not_optimized_time.mul_f32(2.6));

        // The SIMD optimized version can be slower than the optimized version on small buffers.
        assert!(simd_optimized_time < optimized_time.mul_f32(1.5));
    }
}
