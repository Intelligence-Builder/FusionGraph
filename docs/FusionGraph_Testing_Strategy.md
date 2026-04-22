# FusionGraph Testing Strategy

**Version:** 1.0  
**Status:** Draft

## 1. Overview

This document defines the testing strategy for FusionGraph across all layers: unit, integration, end-to-end, performance, and fuzz testing.

## 2. Testing Pyramid

```
                    ┌─────────────┐
                    │   E2E/UI    │  ← Snowflake SQL, Python SDK
                   ─┴─────────────┴─
                  ┌─────────────────┐
                  │  Integration    │  ← DataFusion + CSR + Iceberg
                 ─┴─────────────────┴─
                ┌───────────────────────┐
                │    Component Tests    │  ← CSR, Delta, Traversal
               ─┴───────────────────────┴─
              ┌─────────────────────────────┐
              │        Unit Tests           │  ← Functions, Structs
             ─┴─────────────────────────────┴─
```

| Layer | Coverage Target | Execution Time | Frequency |
|-------|-----------------|----------------|-----------|
| Unit | 80%+ | < 1 min | Every commit |
| Component | 70%+ | < 5 min | Every PR |
| Integration | 60%+ | < 15 min | Pre-merge |
| E2E | Critical paths | < 30 min | Nightly |
| Performance | Regression | < 1 hour | Weekly |

---

## 3. Unit Tests

### 3.1 Scope

Test individual functions and structs in isolation.

### 3.2 Key Modules

| Module | Test Focus |
|--------|------------|
| `ontology::parser` | TOML/JSON parsing, validation errors |
| `csr::shard` | Shard indexing, boundary conditions |
| `csr::builder` | Sort-compact logic, memory allocation |
| `delta::map` | Lock-free insert/delete, tombstone logic |
| `traversal::bfs` | Neighbor iteration, depth tracking |
| `traversal::dijkstra` | Priority queue, weight handling |
| `simd::*` | SIMD intrinsics (platform-specific) |
| `ffi::arrow` | Arrow import/export correctness |

### 3.3 Test Patterns

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Property-based testing with proptest
    proptest! {
        #[test]
        fn shard_index_roundtrip(node_id in 0u64..10_000_000) {
            let (shard_id, offset) = global_to_shard(node_id);
            let recovered = shard_to_global(shard_id, offset);
            prop_assert_eq!(node_id, recovered);
        }
    }

    // Edge case testing
    #[test]
    fn csr_empty_graph() {
        let csr = CsrGraph::empty();
        assert_eq!(csr.node_count(), 0);
        assert_eq!(csr.edge_count(), 0);
        assert!(csr.neighbors(NodeId(0)).next().is_none());
    }

    // Error path testing
    #[test]
    fn ontology_rejects_dangling_edge() {
        let toml = r#"
            [[nodes]]
            label = "User"
            source = "users"
            id_column = "id"

            [[edges]]
            label = "KNOWS"
            from_node = "User"
            to_node = "NonExistent"  # Should fail
        "#;
        let result = Ontology::from_toml(toml);
        assert!(matches!(result, Err(OntologyParseError::DanglingEdge(_))));
    }
}
```

### 3.4 SIMD-Specific Tests

```rust
#[cfg(test)]
mod simd_tests {
    use super::*;

    // Test all SIMD backends produce identical results
    #[test]
    fn simd_backends_equivalent() {
        let neighbors: Vec<u32> = (0..1000).collect();
        let visited = Bitset::new(1000);
        
        let scalar_result = bfs_step_scalar(&neighbors, &visited);
        
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") {
                let avx512_result = bfs_step_avx512(&neighbors, &visited);
                assert_eq!(scalar_result, avx512_result);
            }
            if is_x86_feature_detected!("avx2") {
                let avx2_result = bfs_step_avx2(&neighbors, &visited);
                assert_eq!(scalar_result, avx2_result);
            }
        }
        
        #[cfg(target_arch = "aarch64")]
        {
            let neon_result = bfs_step_neon(&neighbors, &visited);
            assert_eq!(scalar_result, neon_result);
        }
    }

    // Boundary alignment tests
    #[test]
    fn simd_unaligned_input() {
        // Test with sizes that don't align to SIMD width
        for size in [1, 7, 15, 17, 31, 33, 63, 65] {
            let neighbors: Vec<u32> = (0..size).collect();
            let result = bfs_step_auto(&neighbors, &Bitset::new(size as usize));
            assert_eq!(result.len(), size as usize);
        }
    }
}
```

---

## 4. Component Tests

### 4.1 CSR Builder Component

```rust
#[cfg(test)]
mod csr_builder_tests {
    use arrow::array::{UInt64Array, StringArray};
    use arrow::record_batch::RecordBatch;

    #[tokio::test]
    async fn builds_csr_from_edge_batches() {
        // Create edge data
        let sources = UInt64Array::from(vec![1, 1, 2, 3, 3, 3]);
        let targets = UInt64Array::from(vec![2, 3, 3, 4, 5, 6]);
        let batch = RecordBatch::try_from_iter(vec![
            ("source", Arc::new(sources) as ArrayRef),
            ("target", Arc::new(targets) as ArrayRef),
        ]).unwrap();

        // Build CSR
        let csr = CsrBuilder::new()
            .with_edge_batch(batch)
            .build()
            .await
            .unwrap();

        // Verify topology
        assert_eq!(csr.node_count(), 6);
        assert_eq!(csr.edge_count(), 6);
        assert_eq!(csr.out_degree(NodeId(1)), 2);
        assert_eq!(csr.out_degree(NodeId(3)), 3);
        
        let neighbors_of_1: Vec<_> = csr.neighbors(NodeId(1)).collect();
        assert_eq!(neighbors_of_1, vec![NodeId(2), NodeId(3)]);
    }

    #[tokio::test]
    async fn handles_100m_edges() {
        let batch = generate_random_edge_batch(100_000_000);
        let start = Instant::now();
        
        let csr = CsrBuilder::new()
            .with_shard_size(64 * 1024 * 1024)
            .with_edge_batch(batch)
            .build()
            .await
            .unwrap();
        
        let elapsed = start.elapsed();
        
        // Performance assertion
        assert!(elapsed < Duration::from_secs(60), 
            "100M edge build took {:?}, expected < 60s", elapsed);
        
        // Memory assertion
        let overhead = csr.memory_usage() as f64 / (100_000_000 * 8) as f64;
        assert!(overhead < 1.05, "Memory overhead {:.2}%, expected < 5%", (overhead - 1.0) * 100.0);
    }
}
```

### 4.2 Delta Layer Component

```rust
#[cfg(test)]
mod delta_tests {
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn concurrent_inserts_no_data_loss() {
        let delta = Arc::new(DeltaLayer::new());
        let handles: Vec<_> = (0..10)
            .map(|t| {
                let delta = Arc::clone(&delta);
                thread::spawn(move || {
                    for i in 0..10_000 {
                        let from = NodeId((t * 10_000 + i) as u64);
                        let to = NodeId(i as u64);
                        delta.insert(from, to, EdgeData::default());
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(delta.insertion_count(), 100_000);
    }

    #[test]
    fn tombstone_hides_base_edge() {
        let base = CsrGraph::from_edges(&[(1, 2), (1, 3), (2, 3)]);
        let delta = DeltaLayer::new();
        
        // Delete edge 1->2
        delta.delete(NodeId(1), NodeId(2));
        
        // Traversal should skip deleted edge
        let hybrid = HybridGraph::new(base, delta);
        let neighbors: Vec<_> = hybrid.neighbors(NodeId(1)).collect();
        
        assert_eq!(neighbors, vec![NodeId(3)]); // 2 is hidden
    }
}
```

### 4.3 Traversal Component

```rust
#[cfg(test)]
mod traversal_tests {
    // Known graph for deterministic testing
    //     1 → 2 → 4
    //     ↓   ↓
    //     3 → 5
    fn test_graph() -> CsrGraph {
        CsrGraph::from_edges(&[
            (1, 2), (1, 3), (2, 4), (2, 5), (3, 5)
        ])
    }

    #[test]
    fn bfs_level_order() {
        let graph = test_graph();
        let result = bfs(&graph, NodeId(1), 10);
        
        assert_eq!(result.levels, vec![
            vec![NodeId(1)],           // depth 0
            vec![NodeId(2), NodeId(3)], // depth 1
            vec![NodeId(4), NodeId(5)], // depth 2
        ]);
    }

    #[test]
    fn bfs_respects_max_depth() {
        let graph = test_graph();
        let result = bfs(&graph, NodeId(1), 1);
        
        assert!(!result.visited.contains(NodeId(4)));
        assert!(!result.visited.contains(NodeId(5)));
    }

    #[test]
    fn dijkstra_shortest_path() {
        let graph = CsrGraph::from_weighted_edges(&[
            (1, 2, 1.0), (1, 3, 4.0), (2, 3, 2.0), (3, 4, 1.0)
        ]);
        
        let path = dijkstra(&graph, NodeId(1), NodeId(4));
        
        assert_eq!(path.nodes, vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4)]);
        assert_eq!(path.total_weight, 4.0); // 1 + 2 + 1
    }

    #[test]
    fn blast_radius_decay() {
        let graph = test_graph();
        let scores = blast_radius(&graph, NodeId(1), 0.5);
        
        // Node 1: score = 1.0 (start)
        // Node 2, 3: score = 0.5 (depth 1, decay 0.5)
        // Node 4, 5: score = 0.25 (depth 2, decay 0.5^2)
        assert_eq!(scores.get(&NodeId(1)), Some(&1.0));
        assert_eq!(scores.get(&NodeId(2)), Some(&0.5));
        assert_eq!(scores.get(&NodeId(5)), Some(&0.25));
    }
}
```

---

## 5. Integration Tests

### 5.1 DataFusion Integration

```rust
#[cfg(test)]
mod datafusion_integration {
    use datafusion::prelude::*;
    use fusiongraph::GraphTableProvider;

    #[tokio::test]
    async fn sql_triggers_graph_traversal() {
        let ctx = SessionContext::new();
        
        // Register test tables
        ctx.register_parquet("users", "testdata/users.parquet", Default::default()).await?;
        ctx.register_parquet("friendships", "testdata/friendships.parquet", Default::default()).await?;
        
        // Register graph provider
        let ontology = Ontology::from_file("testdata/social_graph.toml")?;
        let provider = GraphTableProvider::new(ctx.state(), ontology).await?;
        ctx.register_table("social_graph", Arc::new(provider))?;
        
        // Execute traversal via SQL
        let df = ctx.sql(r#"
            SELECT * FROM TABLE(
                graph_traverse(
                    start_node => 'User:1',
                    max_depth => 2,
                    edge_labels => ARRAY['FRIEND_OF']
                )
            )
        "#).await?;
        
        let results = df.collect().await?;
        
        // Verify results
        assert!(!results.is_empty());
        let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
        assert!(total_rows > 0);
    }

    #[tokio::test]
    async fn explain_shows_graph_operators() {
        let ctx = setup_graph_context().await;
        
        let df = ctx.sql(r#"
            EXPLAIN SELECT * FROM TABLE(
                graph_traverse(start_node => 'User:1', max_depth => 3)
            )
        "#).await?;
        
        let plan = df.collect().await?;
        let plan_str = format!("{:?}", plan);
        
        assert!(plan_str.contains("GraphTraversalExec"));
        assert!(!plan_str.contains("HashJoinExec")); // Should not fall back to joins
    }
}
```

### 5.2 Iceberg Integration

```rust
#[cfg(test)]
mod iceberg_integration {
    use iceberg_rust::catalog::Catalog;

    #[tokio::test]
    async fn manifest_pruning_skips_files() {
        let catalog = setup_test_catalog().await;
        let ontology = Ontology::from_file("testdata/iam_graph.toml")?;
        
        let provider = GraphTableProvider::new_with_catalog(catalog, ontology).await?;
        
        // Query for specific account
        let stats = provider.build_csr_with_filter("account_id = '123456789'").await?;
        
        // Should skip 90%+ of files
        assert!(stats.files_skipped as f64 / stats.files_total as f64 > 0.9);
    }
}
```

### 5.3 Arrow FFI Integration

```rust
#[cfg(test)]
mod ffi_integration {
    use arrow::ffi::{FFI_ArrowArray, FFI_ArrowSchema};

    #[test]
    fn roundtrip_through_ffi() {
        // Create test batch
        let original = create_test_batch();
        
        // Export to FFI
        let (array, schema) = export_record_batch(&original).unwrap();
        
        // Import back
        let imported = unsafe {
            import_record_batch(&array as *const _, &schema as *const _)
        }.unwrap();
        
        // Verify equality
        assert_eq!(original.num_rows(), imported.num_rows());
        assert_eq!(original.schema(), imported.schema());
        
        for i in 0..original.num_columns() {
            assert_eq!(original.column(i), imported.column(i));
        }
    }

    #[test]
    fn ffi_memory_safety_under_miri() {
        // This test should be run with: cargo miri test ffi_memory_safety
        let batch = create_test_batch();
        let (array, schema) = export_record_batch(&batch).unwrap();
        
        // Simulate external consumer taking ownership
        let imported = unsafe {
            import_record_batch(&array as *const _, &schema as *const _)
        }.unwrap();
        
        // Drop in various orders
        drop(imported);
        drop(array);
        drop(schema);
        drop(batch);
        
        // Miri will catch any use-after-free or double-free
    }
}
```

---

## 6. End-to-End Tests

### 6.1 SQL Workflow Tests

```sql
-- e2e/test_iam_blast_radius.sql

-- Setup: Load test data
CALL graph.register_ontology('testdata/iam_graph.toml');
CALL graph.materialize();

-- Test 1: Basic traversal
SELECT COUNT(*) as reachable_resources
FROM TABLE(
    graph_traverse(
        start_node => 'User:test-admin',
        max_depth => 5,
        edge_labels => ARRAY['CAN_ASSUME', 'HAS_POLICY', 'ALLOWS_ACTION']
    )
)
WHERE node_label = 'Resource';
-- EXPECTED: reachable_resources > 100

-- Test 2: Blast radius scoring
SELECT node_id, blast_score
FROM TABLE(
    graph_blast_radius(
        start_node => 'Role:arn:aws:iam::123456789:role/AdminRole',
        max_depth => 5,
        decay_factor => 0.8
    )
)
ORDER BY blast_score DESC
LIMIT 10;
-- EXPECTED: Top result has blast_score = 1.0

-- Test 3: Pattern matching
SELECT u.user_name, r.role_name, p.policy_name
FROM TABLE(
    graph_match(
        pattern => '(u:User)-[:CAN_ASSUME]->(r:Role)-[:HAS_POLICY]->(p:Policy)'
    )
) AS matches
JOIN users u ON matches.u_id = u.user_id
JOIN roles r ON matches.r_id = r.role_id
JOIN policies p ON matches.p_id = p.policy_id
WHERE p.policy_name LIKE '%Admin%';
-- EXPECTED: Returns privileged access paths
```

### 6.2 Python SDK Tests

```python
# e2e/test_python_sdk.py
import pytest
import fusiongraph
import pyarrow as pa

@pytest.fixture
def graph_ctx():
    ctx = fusiongraph.Context("testdata/social_graph.toml")
    ctx.materialize()
    yield ctx
    ctx.close()

def test_traverse_returns_arrow_table(graph_ctx):
    result = graph_ctx.traverse(
        start_node="User:1",
        max_depth=3,
        edge_labels=["FRIEND_OF"]
    )
    
    assert isinstance(result, pa.Table)
    assert len(result) > 0
    assert "node_id" in result.column_names
    assert "depth" in result.column_names

def test_blast_radius_scoring(graph_ctx):
    scores = graph_ctx.blast_radius(
        start_node="User:1",
        max_depth=5,
        decay_factor=0.8
    )
    
    # Start node should have score 1.0
    start_score = scores.filter(pa.compute.equal(scores["node_id"], "User:1"))
    assert start_score["score"][0].as_py() == 1.0

def test_concurrent_traversals(graph_ctx):
    import concurrent.futures
    
    def run_traversal(start):
        return graph_ctx.traverse(start_node=f"User:{start}", max_depth=3)
    
    with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
        futures = [executor.submit(run_traversal, i) for i in range(100)]
        results = [f.result() for f in futures]
    
    # All traversals should complete without error
    assert all(len(r) >= 0 for r in results)
```

---

## 7. Performance Tests

### 7.1 Benchmark Harness

```rust
// benches/traversal_bench.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_bfs_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs_traversal");
    
    for size in [1_000, 10_000, 100_000, 1_000_000, 10_000_000] {
        let graph = generate_scale_free_graph(size);
        
        group.bench_with_input(
            BenchmarkId::new("scalar", size),
            &graph,
            |b, g| b.iter(|| bfs_scalar(g, NodeId(0), 5))
        );
        
        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx512f") {
            group.bench_with_input(
                BenchmarkId::new("avx512", size),
                &graph,
                |b, g| b.iter(|| bfs_avx512(g, NodeId(0), 5))
            );
        }
        
        #[cfg(target_arch = "aarch64")]
        group.bench_with_input(
            BenchmarkId::new("neon", size),
            &graph,
            |b, g| b.iter(|| bfs_neon(g, NodeId(0), 5))
        );
    }
    
    group.finish();
}

fn bench_csr_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("csr_build");
    group.sample_size(10); // Fewer samples for slow benchmarks
    
    for edge_count in [1_000_000, 10_000_000, 100_000_000] {
        let edges = generate_random_edges(edge_count);
        
        group.bench_with_input(
            BenchmarkId::from_parameter(edge_count),
            &edges,
            |b, e| b.iter(|| CsrBuilder::new().with_edges(e.clone()).build())
        );
    }
    
    group.finish();
}

criterion_group!(benches, bench_bfs_traversal, bench_csr_build);
criterion_main!(benches);
```

### 7.2 Reference Hardware

All performance assertions are validated against:

| Component | Spec |
|-----------|------|
| Instance | AWS r6i.xlarge |
| CPU | Intel Xeon 8375C (4 vCPU) |
| RAM | 32 GB |
| Storage | gp3 SSD (3000 IOPS) |
| OS | Amazon Linux 2023 |

### 7.3 Performance Baselines

| Operation | Input Size | Target | Fail Threshold |
|-----------|------------|--------|----------------|
| CSR Build | 100M edges | < 30s | > 60s |
| BFS 3-hop (scalar) | 10M nodes | < 100ms | > 500ms |
| BFS 3-hop (SIMD) | 10M nodes | < 15ms | > 50ms |
| Delta insert | 1 edge | < 1μs | > 10μs |
| Delta insert (batch) | 500k edges | < 1s | > 5s |

### 7.4 Regression Detection

```yaml
# .github/workflows/perf.yml
name: Performance Regression
on:
  pull_request:
    paths:
      - 'src/**'
      - 'benches/**'

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Run benchmarks
        run: cargo bench -- --save-baseline pr
        
      - name: Compare with main
        run: |
          git fetch origin main
          git checkout origin/main
          cargo bench -- --save-baseline main
          git checkout -
          cargo bench -- --baseline main --load-baseline pr
        
      - name: Check for regression
        run: |
          # Fail if any benchmark regressed by > 10%
          cargo bench -- --baseline main --load-baseline pr 2>&1 | \
            grep -E "regressed by [0-9]+\.[0-9]+%" | \
            awk -F'regressed by ' '{if ($2 > 10.0) exit 1}'
```

---

## 8. Fuzz Testing

### 8.1 Ontology Parser Fuzzing

```rust
// fuzz/fuzz_targets/ontology_parser.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use fusiongraph::ontology::Ontology;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Should never panic, only return errors
        let _ = Ontology::from_toml(s);
        let _ = Ontology::from_json(s);
    }
});
```

### 8.2 CSR Builder Fuzzing

```rust
// fuzz/fuzz_targets/csr_builder.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct FuzzEdges {
    edges: Vec<(u32, u32)>,
}

fuzz_target!(|input: FuzzEdges| {
    if input.edges.len() > 100_000 {
        return; // Limit size for fuzzing speed
    }
    
    // Should never panic
    let result = CsrBuilder::new()
        .with_edges(input.edges)
        .build();
    
    // If build succeeds, verify invariants
    if let Ok(csr) = result {
        assert!(csr.memory_usage() > 0);
        // Every edge should be findable
        for (from, to) in &input.edges {
            assert!(csr.has_edge(NodeId(*from as u64), NodeId(*to as u64)));
        }
    }
});
```

### 8.3 Traversal Fuzzing

```rust
// fuzz/fuzz_targets/traversal.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct FuzzTraversal {
    edges: Vec<(u32, u32)>,
    start: u32,
    max_depth: u8,
}

fuzz_target!(|input: FuzzTraversal| {
    if input.edges.is_empty() || input.edges.len() > 10_000 {
        return;
    }
    
    let csr = CsrBuilder::new().with_edges(input.edges.clone()).build().unwrap();
    let start = NodeId(input.start as u64 % (csr.node_count() as u64).max(1));
    let max_depth = (input.max_depth % 10) as u32;
    
    // Should never panic or hang
    let result = bfs(&csr, start, max_depth);
    
    // Verify invariants
    assert!(result.visited.contains(start));
    assert!(result.max_depth_reached <= max_depth);
});
```

---

## 9. Test Data Management

### 9.1 Synthetic Data Generators

```rust
pub mod testdata {
    /// Generate a scale-free graph (power-law degree distribution)
    pub fn scale_free_graph(n: usize, m: usize) -> Vec<(u64, u64)>;
    
    /// Generate a random graph (Erdős–Rényi model)
    pub fn random_graph(n: usize, p: f64) -> Vec<(u64, u64)>;
    
    /// Generate a complete graph (every node connected to every other)
    pub fn complete_graph(n: usize) -> Vec<(u64, u64)>;
    
    /// Generate a star graph (one hub connected to all others)
    pub fn star_graph(n: usize) -> Vec<(u64, u64)>;
    
    /// Generate a chain graph (linear path)
    pub fn chain_graph(n: usize) -> Vec<(u64, u64)>;
}
```

### 9.2 Fixture Files

```
testdata/
├── ontologies/
│   ├── social_graph.toml
│   ├── iam_graph.toml
│   └── invalid/
│       ├── dangling_edge.toml
│       ├── missing_column.toml
│       └── cycle_implicit.toml
├── parquet/
│   ├── users_1k.parquet
│   ├── users_1m.parquet
│   ├── friendships_10k.parquet
│   └── friendships_10m.parquet
└── iceberg/
    └── test_catalog/
        ├── iam.users/
        └── iam.roles/
```

---

## 10. CI/CD Integration

### 10.1 GitHub Actions Workflow

```yaml
# .github/workflows/test.yml
name: Test Suite

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --lib

  component:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --tests

  integration:
    runs-on: ubuntu-latest
    services:
      minio:
        image: minio/minio
        env:
          MINIO_ROOT_USER: minioadmin
          MINIO_ROOT_PASSWORD: minioadmin
        options: >-
          server /data
        ports:
          - 9000:9000
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --features integration

  miri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - run: cargo miri test --lib -- ffi

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-llvm-cov
      - run: cargo llvm-cov --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v3
        with:
          files: lcov.info
```

---

## 11. Test Ownership

| Area | Owner | Review Required |
|------|-------|-----------------|
| Unit tests | Feature author | 1 reviewer |
| Component tests | Feature author | 1 reviewer |
| Integration tests | Core team | 2 reviewers |
| E2E tests | QA lead | QA + Core |
| Performance tests | Performance lead | Performance + Core |
| Fuzz tests | Security lead | Security + Core |
