//! Core BFS traversal benchmarks.
//!
//! Tiers:
//! - chain/star/uniform topology microbenchmarks
//! - R-MAT skewed-degree graphs (Graph500 parameters) at ~8M edges
//! - SIMD backend comparison (platform backend vs. scalar reference)
//! - delta slow path (BFS with live delta mutations)
//! - 100M-edge tier, opt-in via `FG_BENCH_LARGE=1` (several GB of RAM):
//!   `FG_BENCH_LARGE=1 cargo bench -p fusiongraph-core`
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation)] // bench sizes are small and fixed
#![allow(clippy::significant_drop_tightening)] // criterion group lifetimes are idiomatic

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use fusiongraph_core::gen::{rmat, uniform};
use fusiongraph_core::traversal::{bfs, bfs_bounded_with_backend, ScalarBackend};
use fusiongraph_core::types::EdgeData;
use fusiongraph_core::{CsrGraph, NodeId};

fn generate_chain_graph(n: usize) -> CsrGraph {
    let edges: Vec<_> = (0..n as u64 - 1).map(|i| (i, i + 1)).collect();
    CsrGraph::from_edges(&edges)
}

fn generate_star_graph(n: usize) -> CsrGraph {
    let edges: Vec<_> = (1..n as u64).map(|i| (0, i)).collect();
    CsrGraph::from_edges(&edges)
}

fn bench_bfs(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs");

    for size in [100, 1_000, 10_000] {
        let chain = generate_chain_graph(size);
        group.bench_with_input(BenchmarkId::new("chain", size), &chain, |b, g| {
            b.iter(|| bfs(black_box(g), black_box(NodeId::new(0)), black_box(100)));
        });

        let star = generate_star_graph(size);
        group.bench_with_input(BenchmarkId::new("star", size), &star, |b, g| {
            b.iter(|| bfs(black_box(g), black_box(NodeId::new(0)), black_box(100)));
        });
    }

    for size in [10_000u64, 100_000] {
        let random = CsrGraph::from_edges(&uniform(size, 8, 0x5EED));
        group.bench_with_input(
            BenchmarkId::new("uniform_d8_3hop", size),
            &random,
            |b, g| {
                b.iter(|| bfs(black_box(g), black_box(NodeId::new(0)), black_box(3)));
            },
        );
    }

    group.finish();
}

/// R-MAT skewed-degree tier: hubs stress the batch filter with long
/// neighbor slices, matching real-world graph shape.
fn bench_rmat(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs_rmat");

    // scale 20 = 1M nodes, edge factor 8 = ~8.4M edges.
    let graph = CsrGraph::from_edges(&rmat(20, 8, 0x5EED));
    // Start from a hub (node 0 is in the densest R-MAT quadrant).
    for depth in [2u32, 3] {
        group.bench_with_input(BenchmarkId::new("scale20_ef8", depth), &depth, |b, &d| {
            b.iter(|| bfs(black_box(&graph), black_box(NodeId::new(0)), black_box(d)));
        });
    }

    group.finish();

    // 100M-edge tier: scale 23 (8.4M nodes) x ef 12 = ~100M edges (~2.5GB
    // peak during build). Opt-in to keep default bench runs fast.
    if std::env::var("FG_BENCH_LARGE").is_ok() {
        let mut large = c.benchmark_group("bfs_rmat_large");
        large.sample_size(10);
        let graph = CsrGraph::from_edges(&rmat(23, 12, 0x5EED));
        for depth in [3u32, 6] {
            large.bench_with_input(
                BenchmarkId::new("scale23_ef12_100m", depth),
                &depth,
                |b, &d| {
                    b.iter(|| bfs(black_box(&graph), black_box(NodeId::new(0)), black_box(d)));
                },
            );
        }
        large.finish();
    }
}

/// Platform SIMD backend vs. scalar reference on the same traversal.
fn bench_backends(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs_backend");

    let graph = CsrGraph::from_edges(&rmat(18, 16, 0x5EED)); // dense hubs

    let mut backends: Vec<(&str, Box<dyn fusiongraph_core::traversal::SimdBackend>)> =
        vec![("scalar", Box::new(ScalarBackend))];
    #[cfg(target_arch = "aarch64")]
    backends.push((
        "neon",
        Box::new(fusiongraph_core::traversal::simd::NeonBackend),
    ));
    #[cfg(target_arch = "x86_64")]
    backends.push((
        "avx2",
        Box::new(fusiongraph_core::traversal::simd::Avx2Backend),
    ));

    for (name, backend) in &backends {
        group.bench_function(*name, |b| {
            b.iter(|| {
                bfs_bounded_with_backend(
                    black_box(&graph),
                    black_box(&[NodeId::new(0)]),
                    black_box(3),
                    None,
                    backend.as_ref(),
                )
            });
        });
    }

    group.finish();
}

/// Delta slow path: identical topology, but with live delta mutations that
/// force the merged base+delta iterator.
fn bench_delta_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs_delta");

    let edges = uniform(100_000, 8, 0x5EED);
    let clean = CsrGraph::from_edges(&edges);

    let dirty = CsrGraph::from_edges(&edges);
    // 1% of nodes get a delta-inserted edge; a handful of tombstones.
    for i in 0..1_000u64 {
        dirty
            .delta()
            .insert(NodeId::new(i * 100), NodeId::new(i), EdgeData::default());
    }
    for i in 0..100u64 {
        dirty.delta().delete(NodeId::new(i), NodeId::new(i + 1));
    }

    group.bench_function("fast_path_clean", |b| {
        b.iter(|| bfs(black_box(&clean), black_box(NodeId::new(0)), black_box(3)));
    });
    group.bench_function("slow_path_with_delta", |b| {
        b.iter(|| bfs(black_box(&dirty), black_box(NodeId::new(0)), black_box(3)));
    });

    // Compaction merges the delta into a new base, restoring the fast path.
    let compacted = dirty.compact().expect("compaction succeeds");
    group.bench_function("fast_path_after_compact", |b| {
        b.iter(|| {
            bfs(
                black_box(&compacted),
                black_box(NodeId::new(0)),
                black_box(3),
            )
        });
    });

    group.finish();
}

fn bench_csr_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("csr_build");

    for size in [10_000u64, 100_000] {
        let edges = uniform(size, 8, 0x5EED);
        group.bench_with_input(BenchmarkId::new("uniform_d8", size), &edges, |b, e| {
            b.iter(|| CsrGraph::from_edges(black_box(e)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_bfs,
    bench_rmat,
    bench_backends,
    bench_delta_path,
    bench_csr_build
);
criterion_main!(benches);
