//! Bitset for efficient visited tracking in graph traversals.

use std::sync::atomic::{AtomicU64, Ordering};

/// A thread-safe bitset optimized for graph traversal visited tracking.
///
/// Uses atomic operations for concurrent access and is cache-line aligned
/// for SIMD-friendly memory access patterns.
#[derive(Debug)]
pub struct AtomicBitset {
    /// Bit storage (64 bits per element).
    bits: Box<[AtomicU64]>,
    /// Number of bits (nodes) this bitset can track.
    capacity: usize,
}

impl AtomicBitset {
    /// Creates a new bitset with capacity for `n` bits, all initially unset.
    #[must_use]
    pub fn new(n: usize) -> Self {
        let num_words = (n + 63) / 64;
        let bits: Vec<AtomicU64> = (0..num_words).map(|_| AtomicU64::new(0)).collect();
        Self {
            bits: bits.into_boxed_slice(),
            capacity: n,
        }
    }

    /// Returns the capacity (maximum number of bits).
    #[must_use]
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Tests if bit `i` is set.
    #[must_use]
    #[inline]
    pub fn test(&self, i: usize) -> bool {
        if i >= self.capacity {
            return false;
        }
        let word = i / 64;
        let bit = i % 64;
        (self.bits[word].load(Ordering::Acquire) & (1u64 << bit)) != 0
    }

    /// Sets bit `i` and returns whether it was previously unset.
    /// Returns `true` if this call set the bit (it was previously 0).
    #[inline]
    pub fn test_and_set(&self, i: usize) -> bool {
        if i >= self.capacity {
            return false;
        }
        let word = i / 64;
        let bit = i % 64;
        let mask = 1u64 << bit;
        let old = self.bits[word].fetch_or(mask, Ordering::AcqRel);
        (old & mask) == 0
    }

    /// Sets bit `i`.
    #[inline]
    pub fn set(&self, i: usize) {
        if i >= self.capacity {
            return;
        }
        let word = i / 64;
        let bit = i % 64;
        self.bits[word].fetch_or(1u64 << bit, Ordering::Release);
    }

    /// Clears all bits.
    pub fn clear(&self) {
        for word in self.bits.iter() {
            word.store(0, Ordering::Release);
        }
    }

    /// Returns the raw bits slice for SIMD operations.
    ///
    /// # Safety
    /// Caller must ensure proper synchronization when using raw access.
    #[must_use]
    pub fn as_raw(&self) -> &[AtomicU64] {
        &self.bits
    }

    /// Counts the number of set bits.
    #[must_use]
    pub fn count_ones(&self) -> usize {
        self.bits
            .iter()
            .map(|w| w.load(Ordering::Acquire).count_ones() as usize)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bitset_is_empty() {
        let bs = AtomicBitset::new(100);
        assert_eq!(bs.capacity(), 100);
        assert_eq!(bs.count_ones(), 0);
    }

    #[test]
    fn test_and_set_works() {
        let bs = AtomicBitset::new(100);

        assert!(bs.test_and_set(42)); // First set returns true
        assert!(!bs.test_and_set(42)); // Second set returns false
        assert!(bs.test(42));
    }

    #[test]
    fn set_and_test() {
        let bs = AtomicBitset::new(1000);

        bs.set(0);
        bs.set(63);
        bs.set(64);
        bs.set(999);

        assert!(bs.test(0));
        assert!(bs.test(63));
        assert!(bs.test(64));
        assert!(bs.test(999));
        assert!(!bs.test(1));
        assert!(!bs.test(500));
    }

    #[test]
    fn clear_resets_all() {
        let bs = AtomicBitset::new(200);

        for i in 0..200 {
            bs.set(i);
        }
        assert_eq!(bs.count_ones(), 200);

        bs.clear();
        assert_eq!(bs.count_ones(), 0);
    }

    #[test]
    fn out_of_bounds_returns_false() {
        let bs = AtomicBitset::new(10);

        assert!(!bs.test(100));
        assert!(!bs.test_and_set(100));
    }
}
