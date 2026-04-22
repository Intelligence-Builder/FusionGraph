//! SIMD abstraction layer for cross-platform traversal acceleration.
//!
//! Provides trait-based SIMD backends with runtime feature detection.
//! Supports AVX-512, AVX2, Neon, and scalar fallback.

/// SIMD backend for batch neighbor evaluation during traversal.
///
/// Implementations use platform-specific SIMD instructions to process
/// multiple neighbors per cycle, reducing branch mispredictions.
pub trait SimdBackend: Send + Sync {
    /// Returns the name of this backend (e.g., "avx512", "neon", "scalar").
    fn name(&self) -> &'static str;

    /// Returns the batch size this backend processes at once.
    fn batch_size(&self) -> usize;

    /// Batch-evaluates neighbors against a visited bitset.
    ///
    /// # Arguments
    /// * `neighbors` - Slice of neighbor node IDs to evaluate
    /// * `visited` - Bitset of already-visited nodes (bit index = node ID)
    ///
    /// # Returns
    /// Vector of unvisited neighbor IDs
    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32>;

    /// Batch-sets visited bits for a slice of node IDs.
    ///
    /// # Arguments
    /// * `nodes` - Node IDs to mark as visited
    /// * `visited` - Mutable bitset to update
    fn set_visited_batch(&self, nodes: &[u32], visited: &mut [u64]);
}

/// Scalar (non-SIMD) fallback implementation.
///
/// Works on all platforms but processes one neighbor at a time.
#[derive(Debug, Default)]
pub struct ScalarBackend;

impl SimdBackend for ScalarBackend {
    fn name(&self) -> &'static str {
        "scalar"
    }

    fn batch_size(&self) -> usize {
        1
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32> {
        neighbors
            .iter()
            .filter(|&&n| {
                let word_idx = (n / 64) as usize;
                let bit_idx = n % 64;
                word_idx < visited.len() && (visited[word_idx] & (1u64 << bit_idx)) == 0
            })
            .copied()
            .collect()
    }

    fn set_visited_batch(&self, nodes: &[u32], visited: &mut [u64]) {
        for &n in nodes {
            let word_idx = (n / 64) as usize;
            let bit_idx = n % 64;
            if word_idx < visited.len() {
                visited[word_idx] |= 1u64 << bit_idx;
            }
        }
    }
}

/// AVX2 SIMD backend (256-bit registers, 8 x u32 per cycle).
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Default)]
pub struct Avx2Backend;

#[cfg(target_arch = "x86_64")]
impl SimdBackend for Avx2Backend {
    fn name(&self) -> &'static str {
        "avx2"
    }

    fn batch_size(&self) -> usize {
        8
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32> {
        // TODO: Implement AVX2 intrinsics for batch filtering
        ScalarBackend.filter_unvisited(neighbors, visited)
    }

    fn set_visited_batch(&self, nodes: &[u32], visited: &mut [u64]) {
        // TODO: Implement AVX2 intrinsics for batch bit-setting
        ScalarBackend.set_visited_batch(nodes, visited)
    }
}

/// AVX-512 SIMD backend (512-bit registers, 16 x u32 per cycle).
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Default)]
pub struct Avx512Backend;

#[cfg(target_arch = "x86_64")]
impl SimdBackend for Avx512Backend {
    fn name(&self) -> &'static str {
        "avx512"
    }

    fn batch_size(&self) -> usize {
        16
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32> {
        // TODO: Implement AVX-512 intrinsics for batch filtering
        ScalarBackend.filter_unvisited(neighbors, visited)
    }

    fn set_visited_batch(&self, nodes: &[u32], visited: &mut [u64]) {
        // TODO: Implement AVX-512 intrinsics for batch bit-setting
        ScalarBackend.set_visited_batch(nodes, visited)
    }
}

/// ARM Neon SIMD backend (128-bit registers, 4 x u32 per cycle).
#[cfg(target_arch = "aarch64")]
#[derive(Debug, Default)]
pub struct NeonBackend;

#[cfg(target_arch = "aarch64")]
impl SimdBackend for NeonBackend {
    fn name(&self) -> &'static str {
        "neon"
    }

    fn batch_size(&self) -> usize {
        4
    }

    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32> {
        // TODO: Implement Neon intrinsics for batch filtering
        ScalarBackend.filter_unvisited(neighbors, visited)
    }

    fn set_visited_batch(&self, nodes: &[u32], visited: &mut [u64]) {
        // TODO: Implement Neon intrinsics for batch bit-setting
        ScalarBackend.set_visited_batch(nodes, visited)
    }
}

/// Selects the best available SIMD backend for the current platform.
///
/// Runtime detection order:
/// - x86_64: AVX-512 > AVX2 > Scalar
/// - aarch64: Neon (always available)
/// - other: Scalar
#[cfg(target_arch = "x86_64")]
pub fn select_backend() -> Box<dyn SimdBackend> {
    if is_x86_feature_detected!("avx512f") {
        return Box::new(Avx512Backend);
    }
    if is_x86_feature_detected!("avx2") {
        return Box::new(Avx2Backend);
    }
    Box::new(ScalarBackend)
}

/// Selects the best available SIMD backend for the current platform.
#[cfg(target_arch = "aarch64")]
pub fn select_backend() -> Box<dyn SimdBackend> {
    Box::new(NeonBackend)
}

/// Selects the best available SIMD backend for the current platform.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn select_backend() -> Box<dyn SimdBackend> {
    Box::new(ScalarBackend)
}

/// Returns the name of the selected SIMD backend without allocating.
#[cfg(target_arch = "x86_64")]
pub fn backend_name() -> &'static str {
    if is_x86_feature_detected!("avx512f") {
        return "avx512";
    }
    if is_x86_feature_detected!("avx2") {
        return "avx2";
    }
    "scalar"
}

/// Returns the name of the selected SIMD backend without allocating.
#[cfg(target_arch = "aarch64")]
pub fn backend_name() -> &'static str {
    "neon"
}

/// Returns the name of the selected SIMD backend without allocating.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn backend_name() -> &'static str {
    "scalar"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_filter_unvisited() {
        let backend = ScalarBackend;
        let neighbors = [0u32, 1, 2, 3, 64, 65, 128];
        let mut visited = vec![0u64; 3];

        // Mark nodes 1 and 65 as visited
        visited[0] = 1u64 << 1; // node 1
        visited[1] = 1u64 << 1; // node 65

        let unvisited = backend.filter_unvisited(&neighbors, &visited);
        assert_eq!(unvisited, vec![0, 2, 3, 64, 128]);
    }

    #[test]
    fn scalar_set_visited_batch() {
        let backend = ScalarBackend;
        let nodes = [0u32, 1, 63, 64, 127];
        let mut visited = vec![0u64; 2];

        backend.set_visited_batch(&nodes, &mut visited);

        assert_eq!(visited[0], (1u64 << 0) | (1u64 << 1) | (1u64 << 63));
        assert_eq!(visited[1], (1u64 << 0) | (1u64 << 63));
    }

    #[test]
    fn backend_selection_works() {
        let backend = select_backend();
        let name = backend.name();
        assert!(["scalar", "avx2", "avx512", "neon"].contains(&name));
    }

    #[test]
    fn backend_name_consistent() {
        let backend = select_backend();
        assert_eq!(backend.name(), backend_name());
    }

    #[test]
    fn all_backends_produce_same_results() {
        let neighbors = [0u32, 5, 10, 15, 63, 64, 100];
        let mut visited = vec![0u64; 2];
        visited[0] = (1u64 << 5) | (1u64 << 15);

        let scalar = ScalarBackend;
        let expected = scalar.filter_unvisited(&neighbors, &visited);

        let backend = select_backend();
        let actual = backend.filter_unvisited(&neighbors, &visited);

        assert_eq!(actual, expected, "Backend {} differs from scalar", backend.name());
    }
}
