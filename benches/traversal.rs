//! Traversal benchmarks.

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

fn bench_bfs(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs");

    for size in [100, 1_000, 10_000] {
        let chain = generate_chain_graph(size);
        group.bench_with_input(BenchmarkId::new("chain", size), &chain, |b, g| {
            b.iter(|| bfs(black_box(g), black_box(NodeId(0)), black_box(100)))
        });

        let star = generate_star_graph(size);
        group.bench_with_input(BenchmarkId::new("star", size), &star, |b, g| {
            b.iter(|| bfs(black_box(g), black_box(NodeId(0)), black_box(100)))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_bfs);
criterion_main!(benches);
