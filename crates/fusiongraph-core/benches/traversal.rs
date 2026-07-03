//! Core BFS traversal benchmarks over synthetic graph topologies.
#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation)] // bench sizes are small and fixed

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use fusiongraph_core::traversal::bfs;
use fusiongraph_core::{CsrGraph, NodeId};

fn generate_chain_graph(n: usize) -> CsrGraph {
    let edges: Vec<_> = (0..n as u64 - 1).map(|i| (i, i + 1)).collect();
    CsrGraph::from_edges(&edges)
}

fn generate_star_graph(n: usize) -> CsrGraph {
    let edges: Vec<_> = (1..n as u64).map(|i| (0, i)).collect();
    CsrGraph::from_edges(&edges)
}

/// Uniform random graph with fixed out-degree (xorshift, deterministic).
fn generate_random_graph(nodes: u64, degree: u64, mut seed: u64) -> CsrGraph {
    let mut edges = Vec::with_capacity((nodes * degree) as usize);
    for src in 0..nodes {
        for _ in 0..degree {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            edges.push((src, seed % nodes));
        }
    }
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
        let random = generate_random_graph(size, 8, 0x5EED);
        group.bench_with_input(BenchmarkId::new("random_d8_3hop", size), &random, |b, g| {
            b.iter(|| bfs(black_box(g), black_box(NodeId::new(0)), black_box(3)));
        });
    }

    group.finish();
}

fn bench_csr_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("csr_build");

    for size in [10_000u64, 100_000] {
        let edges: Vec<_> = {
            let mut seed = 0x5EEDu64;
            let mut v = Vec::with_capacity((size * 8) as usize);
            for src in 0..size {
                for _ in 0..8 {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    v.push((src, seed % size));
                }
            }
            v
        };
        group.bench_with_input(BenchmarkId::new("random_d8", size), &edges, |b, e| {
            b.iter(|| CsrGraph::from_edges(black_box(e)));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_bfs, bench_csr_build);
criterion_main!(benches);
