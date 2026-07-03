//! Deterministic synthetic graph generators for benchmarks, tests, and demos.
//!
//! Two families:
//! - [`uniform`] — Erdős–Rényi-style fixed out-degree, uniform targets
//! - [`rmat`] — R-MAT recursive-matrix graphs with Graph500 parameters,
//!   producing the skewed (power-law-like) degree distributions seen in
//!   real-world graphs (social, fraud, IAM)
//!
//! Both are seeded and dependency-free (xorshift64), so benchmark inputs are
//! reproducible across runs and machines.

/// Deterministic xorshift64 PRNG step.
#[inline]
const fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Uniform random directed graph: `nodes` nodes, each with exactly `degree`
/// out-edges to uniformly random targets.
#[must_use]
pub fn uniform(nodes: u64, degree: u64, mut seed: u64) -> Vec<(u64, u64)> {
    let mut edges = Vec::with_capacity(usize::try_from(nodes * degree).unwrap_or(0));
    for src in 0..nodes {
        for _ in 0..degree {
            edges.push((src, xorshift(&mut seed) % nodes));
        }
    }
    edges
}

/// R-MAT recursive-matrix graph with Graph500 partition probabilities
/// (a, b, c, d) = (0.57, 0.19, 0.19, 0.05).
///
/// Produces `edge_factor * 2^scale` directed edges over `2^scale` nodes with
/// a skewed degree distribution: a small number of hub nodes accumulate a
/// large share of the edges, matching real-world graph topology far better
/// than uniform generation.
///
/// # Panics
///
/// Panics if `scale >= 64`.
#[must_use]
pub fn rmat(scale: u32, edge_factor: usize, mut seed: u64) -> Vec<(u64, u64)> {
    // Graph500 probabilities scaled to 16-bit fixed point.
    // a = 0.57, a+b = 0.76, a+b+c = 0.95.
    const A: u64 = 37_355; // 0.57  * 65536
    const AB: u64 = 49_807; // 0.76 * 65536
    const ABC: u64 = 62_259; // 0.95 * 65536

    assert!(scale < 64, "scale must be < 64");
    let nodes = 1u64 << scale;
    let edge_count = edge_factor * usize::try_from(nodes).expect("node count fits usize");
    let mut edges = Vec::with_capacity(edge_count);

    for _ in 0..edge_count {
        let mut src = 0u64;
        let mut dst = 0u64;
        for level in (0..scale).rev() {
            let r = xorshift(&mut seed) & 0xFFFF;
            let (src_bit, dst_bit) = if r < A {
                (0, 0)
            } else if r < AB {
                (0, 1)
            } else if r < ABC {
                (1, 0)
            } else {
                (1, 1)
            };
            src |= src_bit << level;
            dst |= dst_bit << level;
        }
        edges.push((src, dst));
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_shape() {
        let edges = uniform(100, 4, 42);
        assert_eq!(edges.len(), 400);
        assert!(edges.iter().all(|&(s, t)| s < 100 && t < 100));
    }

    #[test]
    fn uniform_is_deterministic() {
        assert_eq!(uniform(50, 3, 7), uniform(50, 3, 7));
        assert_ne!(uniform(50, 3, 7), uniform(50, 3, 8));
    }

    #[test]
    fn rmat_shape() {
        let edges = rmat(10, 8, 42);
        assert_eq!(edges.len(), 8 << 10);
        assert!(edges.iter().all(|&(s, t)| s < 1024 && t < 1024));
    }

    #[test]
    fn rmat_is_skewed() {
        // The max out-degree of an R-MAT graph must dwarf the mean; a uniform
        // graph's max degree stays close to the mean.
        let edges = rmat(12, 8, 42);
        let mut degrees = vec![0u32; 1 << 12];
        for &(s, _) in &edges {
            degrees[usize::try_from(s).unwrap()] += 1;
        }
        let max = degrees.iter().copied().max().unwrap();
        // mean degree = 8; Graph500 params yield hubs far above 8 * 4.
        assert!(max > 32, "expected skew, max degree was {max}");
    }

    #[test]
    fn rmat_is_deterministic() {
        assert_eq!(rmat(8, 4, 1), rmat(8, 4, 1));
    }
}
