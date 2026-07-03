//! SIMD abstraction layer for cross-platform traversal acceleration.
//!
//! Provides trait-based SIMD backends with runtime feature detection.
//! Backends batch-test dense neighbor indices against a visited bitset:
//! `word = n >> 6`, `bit = n & 63`, unvisited when `(words[word] >> bit) & 1 == 0`.
//!
//! Implemented backends:
//! - `scalar` — portable reference implementation (always available)
//! - `neon` — `aarch64` intrinsics, 4 indices per iteration
//! - `avx2` — `x86_64` intrinsics with gather + variable shifts, 8 per iteration
//! - `avx512` — declared for dispatch; currently delegates to scalar
//!   (deferred until profiling on AVX-512 hardware, see ROADMAP M4)
//!
//! `unsafe` is confined to the platform intrinsic kernels in this module;
//! every backend is equivalence-tested against the scalar reference.
#![allow(unsafe_code)]

use crate::types::NodeId;

/// Converts a public [`NodeId`] into the dense internal index used by SIMD
/// traversal kernels.
///
/// SIMD backends operate on compact `u32` indices into CSR rows, not the
/// public `NodeId(u64)` API type. Callers should perform this conversion at
/// the traversal boundary after validating the graph fits the `u32` CSR
/// representation.
#[must_use]
pub fn node_id_to_dense_index(node: NodeId) -> Option<u32> {
    u32::try_from(node.as_u64()).ok()
}

/// Converts a dense internal SIMD index back into a public [`NodeId`].
#[must_use]
pub fn dense_index_to_node_id(index: u32) -> NodeId {
    NodeId::new(u64::from(index))
}

/// SIMD backend for batch neighbor evaluation during traversal.
///
/// Implementations use platform-specific SIMD instructions to process
/// multiple dense CSR node indices per cycle, reducing branch mispredictions.
pub trait SimdBackend: Send + Sync {
    /// Returns the name of this backend (e.g., "avx512", "neon", "scalar").
    fn name(&self) -> &'static str;

    /// Returns the batch size this backend processes at once.
    fn batch_size(&self) -> usize;

    /// Batch-evaluates dense neighbor indices against a visited bitset.
    ///
    /// # Arguments
    /// * `neighbors` - Dense `u32` CSR node indices to evaluate, not public
    ///   [`NodeId`] values
    /// * `visited` - Bitset of already-visited dense indices
    ///
    /// # Returns
    /// Vector of unvisited dense neighbor indices
    fn filter_unvisited(&self, neighbors: &[u32], visited: &[u64]) -> Vec<u32> {
        let mut out = Vec::with_capacity(neighbors.len());
        self.filter_unvisited_into(neighbors, visited, &mut out);
        out
    }

    /// Allocation-free variant of [`Self::filter_unvisited`]: clears `out`
    /// and appends the unvisited indices. This is the traversal hot path.
    fn filter_unvisited_into(&self, neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>);

    /// Batch-sets visited bits for a slice of dense node indices.
    ///
    /// # Arguments
    /// * `nodes` - Dense `u32` CSR node indices to mark as visited
    /// * `visited` - Mutable bitset to update
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

#[inline]
fn scalar_filter_into(neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
    out.clear();
    out.reserve(neighbors.len());
    for &n in neighbors {
        let word_idx = (n / 64) as usize;
        let bit_idx = n % 64;
        if word_idx < visited.len() && (visited[word_idx] & (1u64 << bit_idx)) == 0 {
            out.push(n);
        }
    }
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

    fn filter_unvisited_into(&self, neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
        scalar_filter_into(neighbors, visited, out);
    }
}

/// ARM Neon SIMD backend (128-bit registers, 4 x u32 per iteration).
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

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn filter_unvisited_into(&self, neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
        use std::arch::aarch64::{
            vandq_u64, vdupq_n_u64, vgetq_lane_u64, vld1q_u32, vmovl_high_u32, vmovl_u32,
            vnegq_s64, vreinterpretq_s64_u64, vshlq_u64,
        };

        out.clear();
        out.reserve(neighbors.len());

        let words_len = visited.len() as u32;
        let mut chunks = neighbors.chunks_exact(4);

        // NEON is always available on aarch64; the intrinsics used here are
        // safe on every supported CPU. `unsafe` covers the raw pointer load.
        for chunk in chunks.by_ref() {
            // Word indices; bail to scalar for the (rare) out-of-range case.
            let w0 = chunk[0] >> 6;
            let w1 = chunk[1] >> 6;
            let w2 = chunk[2] >> 6;
            let w3 = chunk[3] >> 6;
            if w0 >= words_len || w1 >= words_len || w2 >= words_len || w3 >= words_len {
                scalar_filter_into_no_clear(chunk, visited, out);
                continue;
            }

            // SAFETY: chunk has exactly 4 elements; word indices were bounds
            // checked above; NEON is baseline on aarch64.
            unsafe {
                let idx = vld1q_u32(chunk.as_ptr());

                // bit position within each word: n & 63, widened to u64 lanes
                let bit_mask = vdupq_n_u64(63);
                let bits_lo = vandq_u64(vmovl_u32(std::arch::aarch64::vget_low_u32(idx)), bit_mask);
                let bits_hi = vandq_u64(vmovl_high_u32(idx), bit_mask);

                // gather the 4 words (NEON has no gather; scalar loads)
                let w_lo = std::arch::aarch64::vsetq_lane_u64(
                    *visited.get_unchecked(w1 as usize),
                    vdupq_n_u64(*visited.get_unchecked(w0 as usize)),
                    1,
                );
                let w_hi = std::arch::aarch64::vsetq_lane_u64(
                    *visited.get_unchecked(w3 as usize),
                    vdupq_n_u64(*visited.get_unchecked(w2 as usize)),
                    1,
                );

                // right-shift each word by its bit position (vshl by negative)
                let sh_lo = vnegq_s64(vreinterpretq_s64_u64(bits_lo));
                let sh_hi = vnegq_s64(vreinterpretq_s64_u64(bits_hi));
                let one = vdupq_n_u64(1);
                let t_lo = vandq_u64(vshlq_u64(w_lo, sh_lo), one);
                let t_hi = vandq_u64(vshlq_u64(w_hi, sh_hi), one);

                if vgetq_lane_u64(t_lo, 0) == 0 {
                    out.push(chunk[0]);
                }
                if vgetq_lane_u64(t_lo, 1) == 0 {
                    out.push(chunk[1]);
                }
                if vgetq_lane_u64(t_hi, 0) == 0 {
                    out.push(chunk[2]);
                }
                if vgetq_lane_u64(t_hi, 1) == 0 {
                    out.push(chunk[3]);
                }
            }
        }

        scalar_filter_into_no_clear(chunks.remainder(), visited, out);
    }
}

/// Scalar filter that appends without clearing `out` (tail/fallback helper).
#[inline]
fn scalar_filter_into_no_clear(neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
    for &n in neighbors {
        let word_idx = (n / 64) as usize;
        let bit_idx = n % 64;
        if word_idx < visited.len() && (visited[word_idx] & (1u64 << bit_idx)) == 0 {
            out.push(n);
        }
    }
}

/// AVX2 SIMD backend (256-bit registers, 8 x u32 per iteration).
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Default)]
pub struct Avx2Backend;

#[cfg(target_arch = "x86_64")]
impl Avx2Backend {
    /// # Safety
    /// Caller must ensure AVX2 is available (checked in [`select_backend`]).
    #[target_feature(enable = "avx2")]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        // movemask returns i32 holding a 4-bit lane mask; sign loss impossible.
        clippy::cast_sign_loss
    )]
    unsafe fn filter_avx2(neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
        use std::arch::x86_64::{
            _mm256_and_si256, _mm256_castsi256_pd, _mm256_castsi256_si128, _mm256_cvtepu32_epi64,
            _mm256_extracti128_si256, _mm256_i32gather_epi64, _mm256_loadu_si256,
            _mm256_movemask_pd, _mm256_set1_epi32, _mm256_set1_epi64x, _mm256_srli_epi32,
            _mm256_srlv_epi64, _mm256_sub_epi64,
        };

        let words_len = visited.len() as u32;
        let base = visited.as_ptr().cast::<i64>();
        let mut chunks = neighbors.chunks_exact(8);

        for chunk in chunks.by_ref() {
            // Bounds guard: gather with an out-of-range index would read OOB.
            if chunk.iter().any(|&n| (n >> 6) >= words_len) {
                scalar_filter_into_no_clear(chunk, visited, out);
                continue;
            }

            let idx = _mm256_loadu_si256(chunk.as_ptr().cast());
            let word_idx = _mm256_srli_epi32::<6>(idx);
            let bit_idx = _mm256_and_si256(idx, _mm256_set1_epi32(63));

            // Gather 8 u64 words in two halves (4 i32 indices each).
            let wi_lo = _mm256_castsi256_si128(word_idx);
            let wi_hi = _mm256_extracti128_si256::<1>(word_idx);
            let words_lo = _mm256_i32gather_epi64::<8>(base, wi_lo);
            let words_hi = _mm256_i32gather_epi64::<8>(base, wi_hi);

            // Widen bit positions to u64 lanes and shift each word right.
            let bi_lo = _mm256_cvtepu32_epi64(_mm256_castsi256_si128(bit_idx));
            let bi_hi = _mm256_cvtepu32_epi64(_mm256_extracti128_si256::<1>(bit_idx));
            let one = _mm256_set1_epi64x(1);
            let t_lo = _mm256_and_si256(_mm256_srlv_epi64(words_lo, bi_lo), one);
            let t_hi = _mm256_and_si256(_mm256_srlv_epi64(words_hi, bi_hi), one);

            // Lane == 1 -> visited. Build a "visited" mask: (t - 1) has the
            // sign bit set exactly when t == 0 (unvisited).
            let m_lo = _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_sub_epi64(t_lo, one)));
            let m_hi = _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_sub_epi64(t_hi, one)));
            let unvisited = (m_lo as u32) | ((m_hi as u32) << 4);

            for (lane, &n) in chunk.iter().enumerate() {
                if unvisited & (1 << lane) != 0 {
                    out.push(n);
                }
            }
        }

        scalar_filter_into_no_clear(chunks.remainder(), visited, out);
    }
}

#[cfg(target_arch = "x86_64")]
impl SimdBackend for Avx2Backend {
    fn name(&self) -> &'static str {
        "avx2"
    }

    fn batch_size(&self) -> usize {
        8
    }

    fn filter_unvisited_into(&self, neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
        out.clear();
        out.reserve(neighbors.len());
        if is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 availability verified at runtime.
            unsafe { Self::filter_avx2(neighbors, visited, out) }
        } else {
            scalar_filter_into_no_clear(neighbors, visited, out);
        }
    }
}

/// AVX-512 SIMD backend (declared for dispatch; delegates to scalar until
/// profiling on AVX-512 hardware justifies a dedicated kernel — ROADMAP M4).
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

    fn filter_unvisited_into(&self, neighbors: &[u32], visited: &[u64], out: &mut Vec<u32>) {
        // Use the AVX2 kernel (an AVX-512 machine always supports AVX2).
        Avx2Backend.filter_unvisited_into(neighbors, visited, out);
    }
}

/// Selects the best available SIMD backend for the current platform.
///
/// Runtime detection order:
/// - `x86_64`: AVX-512 > AVX2 > Scalar
/// - `aarch64`: Neon (always available)
/// - other: Scalar
#[cfg(target_arch = "x86_64")]
#[must_use]
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
///
/// On `aarch64` this returns the **scalar** backend: benchmarks on Apple
/// Silicon (R-MAT scale-18/ef-16, 3-hop BFS) measured NEON ~5% slower than
/// scalar because `filter_unvisited` is gather-bound — the scattered per-lane
/// `u64` word loads dominate and NEON has no gather instruction. The
/// [`NeonBackend`] remains available for explicit use and re-evaluation on
/// other ARM cores (see ROADMAP M4).
#[cfg(target_arch = "aarch64")]
#[must_use]
pub fn select_backend() -> Box<dyn SimdBackend> {
    Box::new(ScalarBackend)
}

/// Selects the best available SIMD backend for the current platform.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[must_use]
pub fn select_backend() -> Box<dyn SimdBackend> {
    Box::new(ScalarBackend)
}

/// Returns the name of the selected SIMD backend without allocating.
#[cfg(target_arch = "x86_64")]
#[must_use]
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
#[must_use]
pub const fn backend_name() -> &'static str {
    "scalar"
}

/// Returns the name of the selected SIMD backend without allocating.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[must_use]
pub const fn backend_name() -> &'static str {
    "scalar"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_visited(bits: &[u32], capacity_words: usize) -> Vec<u64> {
        let mut words = vec![0u64; capacity_words];
        for &b in bits {
            words[(b / 64) as usize] |= 1u64 << (b % 64);
        }
        words
    }

    /// Deterministic pseudo-random input covering word boundaries.
    fn pseudo_random_indices(count: usize, max: u32, mut seed: u64) -> Vec<u32> {
        (0..count)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                #[allow(clippy::cast_possible_truncation)]
                {
                    (seed % u64::from(max)) as u32
                }
            })
            .collect()
    }

    #[test]
    fn scalar_filters_visited() {
        let visited = make_visited(&[1, 64, 130], 3);
        let result = ScalarBackend.filter_unvisited(&[0, 1, 63, 64, 65, 130, 191], &visited);
        assert_eq!(result, vec![0, 63, 65, 191]);
    }

    #[test]
    fn scalar_out_of_range_indices_are_dropped() {
        let visited = make_visited(&[], 1); // covers indices 0..64
        let result = ScalarBackend.filter_unvisited(&[10, 64, 200], &visited);
        assert_eq!(result, vec![10]);
    }

    #[test]
    fn set_visited_batch_roundtrip() {
        let mut visited = vec![0u64; 4];
        ScalarBackend.set_visited_batch(&[3, 64, 200], &mut visited);
        let result = ScalarBackend.filter_unvisited(&[3, 4, 64, 200, 201], &visited);
        assert_eq!(result, vec![4, 201]);
    }

    /// Every implemented backend must agree with the scalar reference on
    /// random input, including within-word, cross-word, and remainder cases.
    #[test]
    fn platform_backend_matches_scalar_reference() {
        #[cfg(target_arch = "aarch64")]
        let backend: Box<dyn SimdBackend> = Box::new(NeonBackend);
        #[cfg(not(target_arch = "aarch64"))]
        let backend = select_backend();
        for size in [0usize, 1, 3, 4, 5, 8, 9, 100, 1000, 1003] {
            let neighbors = pseudo_random_indices(size, 4096, 0xF00D + size as u64);
            let visited_bits = pseudo_random_indices(size / 2 + 1, 4096, 0xBEEF + size as u64);
            let visited = make_visited(&visited_bits, 64);

            let expected = ScalarBackend.filter_unvisited(&neighbors, &visited);
            let actual = backend.filter_unvisited(&neighbors, &visited);
            assert_eq!(
                actual,
                expected,
                "backend {} diverged from scalar at size {size}",
                backend.name()
            );
        }
    }

    #[test]
    fn platform_backend_handles_out_of_range() {
        #[cfg(target_arch = "aarch64")]
        let backend: Box<dyn SimdBackend> = Box::new(NeonBackend);
        #[cfg(not(target_arch = "aarch64"))]
        let backend = select_backend();
        let visited = make_visited(&[1], 1);
        // 100 and 4000 exceed the 64-bit capacity -> dropped, matching scalar.
        let expected = ScalarBackend.filter_unvisited(&[0, 1, 2, 100, 4000, 63, 5, 6], &visited);
        let actual = backend.filter_unvisited(&[0, 1, 2, 100, 4000, 63, 5, 6], &visited);
        assert_eq!(actual, expected);
    }

    #[test]
    fn backend_selection_returns_valid_backend() {
        let backend = select_backend();
        assert!(!backend.name().is_empty());
        assert!(backend.batch_size() >= 1);
        assert_eq!(backend_name(), backend.name());
    }

    #[test]
    fn node_id_dense_index_roundtrip() {
        assert_eq!(node_id_to_dense_index(NodeId::new(42)), Some(42));
        assert_eq!(node_id_to_dense_index(NodeId::new(u64::MAX)), None);
        assert_eq!(dense_index_to_node_id(7), NodeId::new(7));
    }
}
