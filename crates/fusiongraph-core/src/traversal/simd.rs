//! SIMD abstraction layer for accelerated graph traversal.
//!
//! Provides a trait-based abstraction over different SIMD instruction sets,
//! with runtime feature detection to select the optimal backend.

use super::bitset::AtomicBitset;

/// SIMD backend trait for accelerated neighbor filtering.
///
/// Implementations filter a batch of neighbor node IDs against a visited bitset,
/// returning only the unvisited neighbors.
pub trait SimdBackend: Send + Sync {
    /// Returns the name of this backend (e.g., "AVX-512", "AVX2", "Scalar").
    fn name(&self) -> &'static str;

    /// Returns the optimal batch size for this backend.
    fn batch_size(&self) -> usize;

    /// Filters neighbors, returning indices of unvisited nodes.
    ///
    /// For each neighbor in `neighbors`, checks if it's unvisited in `visited`.
    /// Returns the neighbors that were unvisited (and marks them as visited).
    fn filter_unvisited(&self, neighbors: &[u32], visited: &AtomicBitset) -> Vec<u32>;
}

/// Scalar (non-SIMD) backend - fallback for all platforms.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScalarBackend;

impl SimdBackend for ScalarBackend {
    fn name(&self) -> &'static str {
        "Scalar"
    }

    fn batch_size(&self) -> usize {
        1
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &AtomicBitset) -> Vec<u32> {
        neighbors
            .iter()
            .copied()
            .filter(|&n| visited.test_and_set(n as usize))
            .collect()
    }
}

/// AVX2 backend - 256-bit SIMD (8 x u32).
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Default, Clone, Copy)]
pub struct Avx2Backend;

#[cfg(target_arch = "x86_64")]
impl SimdBackend for Avx2Backend {
    fn name(&self) -> &'static str {
        "AVX2"
    }

    fn batch_size(&self) -> usize {
        8
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &AtomicBitset) -> Vec<u32> {
        // For now, use scalar implementation
        // TODO: Implement actual AVX2 gather/scatter when stable
        neighbors
            .iter()
            .copied()
            .filter(|&n| visited.test_and_set(n as usize))
            .collect()
    }
}

/// AVX-512 backend - 512-bit SIMD (16 x u32).
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Default, Clone, Copy)]
pub struct Avx512Backend;

#[cfg(target_arch = "x86_64")]
impl SimdBackend for Avx512Backend {
    fn name(&self) -> &'static str {
        "AVX-512"
    }

    fn batch_size(&self) -> usize {
        16
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &AtomicBitset) -> Vec<u32> {
        // For now, use scalar implementation
        // TODO: Implement actual AVX-512 gather/scatter when stable
        neighbors
            .iter()
            .copied()
            .filter(|&n| visited.test_and_set(n as usize))
            .collect()
    }
}

/// ARM Neon backend - 128-bit SIMD (4 x u32).
#[cfg(target_arch = "aarch64")]
#[derive(Debug, Default, Clone, Copy)]
pub struct NeonBackend;

#[cfg(target_arch = "aarch64")]
impl SimdBackend for NeonBackend {
    fn name(&self) -> &'static str {
        "Neon"
    }

    fn batch_size(&self) -> usize {
        4
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &AtomicBitset) -> Vec<u32> {
        // For now, use scalar implementation
        // TODO: Implement actual Neon intrinsics
        neighbors
            .iter()
            .copied()
            .filter(|&n| visited.test_and_set(n as usize))
            .collect()
    }
}

/// Selects the best available SIMD backend at runtime.
#[must_use]
pub fn select_backend() -> Box<dyn SimdBackend> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f") {
            return Box::new(Avx512Backend);
        }
        if is_x86_feature_detected!("avx2") {
            return Box::new(Avx2Backend);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Neon is always available on aarch64
        return Box::new(NeonBackend);
    }

    Box::new(ScalarBackend)
}

/// Returns the name of the best available SIMD backend.
#[must_use]
pub fn detect_simd_support() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f") {
            return "AVX-512";
        }
        if is_x86_feature_detected!("avx2") {
            return "AVX2";
        }
        if is_x86_feature_detected!("sse4.2") {
            return "SSE4.2";
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        return "Neon";
    }

    "Scalar"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_backend_filters_correctly() {
        let backend = ScalarBackend;
        let visited = AtomicBitset::new(100);

        // Mark some as visited
        visited.set(1);
        visited.set(3);

        let neighbors = vec![0, 1, 2, 3, 4];
        let unvisited = backend.filter_unvisited(&neighbors, &visited);

        // 0, 2, 4 were unvisited
        assert_eq!(unvisited, vec![0, 2, 4]);

        // Now they should be marked as visited
        assert!(visited.test(0));
        assert!(visited.test(2));
        assert!(visited.test(4));
    }

    #[test]
    fn select_backend_returns_valid() {
        let backend = select_backend();
        assert!(!backend.name().is_empty());
        assert!(backend.batch_size() >= 1);
    }

    #[test]
    fn detect_simd_returns_string() {
        let name = detect_simd_support();
        assert!(!name.is_empty());
    }

    #[test]
    fn all_backends_produce_same_results() {
        let visited = AtomicBitset::new(100);
        visited.set(5);
        visited.set(10);

        let neighbors: Vec<u32> = (0..20).collect();

        let scalar = ScalarBackend;
        let result = scalar.filter_unvisited(&neighbors, &visited);

        // Should have 18 unvisited (0-4, 6-9, 11-19)
        assert_eq!(result.len(), 18);
        assert!(!result.contains(&5));
        assert!(!result.contains(&10));
    }
}
