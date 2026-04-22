# FusionGraph API Reference

**Version:** 1.0  
**Status:** Draft

## 1. Overview

This document specifies the public API surface of FusionGraph:
- Rust traits and structs
- FFI contracts (Arrow C Data Interface)
- SQL extensions
- Configuration interfaces

---

## 2. Core Traits

### 2.1 `GraphTableProvider`

The primary trait extending DataFusion's `TableProvider` for graph-aware table access.

```rust
use arrow::datatypes::SchemaRef;
use async_trait::async_trait;
use datafusion::catalog::TableProvider;
use datafusion::execution::context::SessionState;
use datafusion::logical_expr::{Expr, TableType};
use datafusion::physical_plan::ExecutionPlan;
use std::sync::Arc;

/// A TableProvider that exposes graph topology alongside relational data.
#[async_trait]
pub trait GraphTableProvider: TableProvider + Send + Sync {
    /// Returns the ontology schema governing this graph projection.
    fn ontology(&self) -> &Ontology;

    /// Returns node labels available in this graph.
    fn node_labels(&self) -> Vec<&str>;

    /// Returns edge labels available in this graph.
    fn edge_labels(&self) -> Vec<&str>;

    /// Returns the schema for a specific node type.
    fn node_schema(&self, label: &str) -> Option<SchemaRef>;

    /// Returns the schema for a specific edge type.
    fn edge_schema(&self, label: &str) -> Option<SchemaRef>;

    /// Indicates whether the CSR is currently materialized.
    fn is_materialized(&self) -> bool;

    /// Forces materialization of the CSR from underlying tables.
    async fn materialize(&self, state: &SessionState) -> Result<(), GraphError>;

    /// Returns statistics about the current graph state.
    fn statistics(&self) -> GraphStatistics;

    /// Creates a traversal execution plan.
    async fn create_traversal_plan(
        &self,
        state: &SessionState,
        traversal: TraversalSpec,
        filters: &[Expr],
    ) -> Result<Arc<dyn ExecutionPlan>, GraphError>;
}
```

### 2.2 `Ontology`

Parsed representation of the ontology schema (TOML/JSON).

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Ontology {
    pub name: String,
    pub version: String,
    pub settings: OntologySettings,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
    pub properties: Vec<ComputedProperty>,
}

#[derive(Debug, Clone)]
pub struct OntologySettings {
    pub default_node_id_type: IdType,
    pub edge_direction: EdgeDirection,
    pub allow_self_loops: bool,
    pub allow_parallel_edges: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdType {
    U32,
    U64,
    U128,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeDirection {
    Directed,
    Undirected,
}

#[derive(Debug, Clone)]
pub struct NodeDefinition {
    pub label: String,
    pub source: String,
    pub id_column: IdColumn,
    pub id_transform: IdTransform,
    pub properties: Vec<String>,
    pub filter: Option<String>,
}

#[derive(Debug, Clone)]
pub enum IdColumn {
    Single(String),
    Composite { columns: Vec<String>, separator: String },
}

#[derive(Debug, Clone, Copy)]
pub enum IdTransform {
    Passthrough,
    HashU64,
    HashU32,
    UuidToU128,
    ExtractNumeric,
}

#[derive(Debug, Clone)]
pub struct EdgeDefinition {
    pub label: String,
    pub source: String,
    pub from_node: String,
    pub from_column: String,
    pub to_node: String,
    pub to_column: String,
    pub properties: Vec<String>,
    pub weight_column: Option<String>,
    pub weight_default: f64,
    pub implicit: bool,
    pub skip_null_targets: bool,
    pub valid_from_column: Option<String>,
    pub valid_to_column: Option<String>,
}

impl Ontology {
    /// Parse from TOML string.
    pub fn from_toml(content: &str) -> Result<Self, OntologyParseError>;

    /// Parse from JSON string.
    pub fn from_json(content: &str) -> Result<Self, OntologyParseError>;

    /// Load from file (auto-detects format).
    pub fn from_file(path: &Path) -> Result<Self, OntologyParseError>;

    /// Validate against a catalog (checks table/column existence).
    pub fn validate(&self, catalog: &dyn CatalogProvider) -> Vec<ValidationError>;
}
```

### 2.3 `CsrGraph`

The in-memory CSR representation.

```rust
use std::sync::Arc;

/// Compressed Sparse Row graph representation.
pub struct CsrGraph {
    /// Node count per label.
    node_counts: HashMap<String, usize>,
    
    /// Edge count per label.
    edge_counts: HashMap<String, usize>,
    
    /// Sharded CSR storage.
    shards: Vec<Arc<CsrShard>>,
    
    /// Delta layer for real-time updates.
    delta: Arc<DeltaLayer>,
    
    /// Dirty bitset for hybrid traversal.
    dirty: Arc<AtomicBitset>,
}

impl CsrGraph {
    /// Total node count across all labels.
    pub fn node_count(&self) -> usize;

    /// Total edge count across all labels.
    pub fn edge_count(&self) -> usize;

    /// Memory footprint in bytes.
    pub fn memory_usage(&self) -> usize;

    /// Get neighbors of a node (returns iterator over NodeId).
    pub fn neighbors(&self, node: NodeId) -> NeighborIter<'_>;

    /// Get neighbors with edge properties.
    pub fn neighbors_with_props(&self, node: NodeId) -> NeighborPropIter<'_>;

    /// Get incoming edges (for directed graphs).
    pub fn in_neighbors(&self, node: NodeId) -> NeighborIter<'_>;

    /// Check if edge exists.
    pub fn has_edge(&self, from: NodeId, to: NodeId) -> bool;

    /// Get edge weight (returns default if unweighted).
    pub fn edge_weight(&self, from: NodeId, to: NodeId) -> Option<f64>;
}

/// A single 64MB shard of the CSR.
pub struct CsrShard {
    /// Shard ID (0..N).
    pub id: u32,
    
    /// Node ID range covered by this shard.
    pub node_range: Range<NodeId>,
    
    /// CSR row pointers (offsets into edges array).
    pub row_ptrs: Arc<[u32]>,
    
    /// CSR column indices (target node IDs).
    pub col_indices: Arc<[u32]>,
    
    /// Optional edge weights.
    pub weights: Option<Arc<[f32]>>,
    
    /// Optional edge properties (Arrow RecordBatch).
    pub properties: Option<RecordBatch>,
}

/// Lock-free delta layer for real-time updates.
pub struct DeltaLayer {
    /// New edges not yet compacted.
    insertions: DashMap<(NodeId, NodeId), EdgeData>,
    
    /// Tombstones for deleted edges.
    deletions: DashSet<(NodeId, NodeId)>,
}

impl DeltaLayer {
    /// Insert an edge (real-time, lock-free).
    pub fn insert(&self, from: NodeId, to: NodeId, data: EdgeData);

    /// Delete an edge (marks as tombstone).
    pub fn delete(&self, from: NodeId, to: NodeId);

    /// Check if edge is tombstoned.
    pub fn is_deleted(&self, from: NodeId, to: NodeId) -> bool;

    /// Pending insertion count.
    pub fn insertion_count(&self) -> usize;

    /// Pending deletion count.
    pub fn deletion_count(&self) -> usize;
}
```

---

## 3. Traversal API

### 3.1 `TraversalSpec`

Specification for graph traversals.

```rust
#[derive(Debug, Clone)]
pub struct TraversalSpec {
    /// Starting nodes (by ID or filter expression).
    pub start: StartNodes,
    
    /// Traversal algorithm.
    pub algorithm: TraversalAlgorithm,
    
    /// Edge labels to follow (empty = all).
    pub edge_labels: Vec<String>,
    
    /// Edge direction to follow.
    pub direction: TraversalDirection,
    
    /// Maximum depth (hops).
    pub max_depth: u32,
    
    /// Maximum results to return.
    pub limit: Option<usize>,
    
    /// Node filter applied during traversal.
    pub node_filter: Option<Expr>,
    
    /// Edge filter applied during traversal.
    pub edge_filter: Option<Expr>,
    
    /// Output format.
    pub output: TraversalOutput,
}

#[derive(Debug, Clone)]
pub enum StartNodes {
    /// Explicit node IDs.
    Ids(Vec<NodeId>),
    
    /// Nodes matching a filter expression.
    Filter { label: String, predicate: Expr },
    
    /// All nodes of a label.
    AllOfLabel(String),
}

#[derive(Debug, Clone, Copy)]
pub enum TraversalAlgorithm {
    /// Breadth-first search.
    Bfs,
    
    /// Depth-first search.
    Dfs,
    
    /// Dijkstra's shortest path.
    Dijkstra,
    
    /// Bellman-Ford (handles negative weights).
    BellmanFord,
    
    /// A* with heuristic.
    AStar { heuristic: HeuristicType },
    
    /// Bidirectional BFS.
    BidirectionalBfs,
}

#[derive(Debug, Clone, Copy)]
pub enum TraversalDirection {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone)]
pub enum TraversalOutput {
    /// Return visited nodes.
    Nodes { include_properties: bool },
    
    /// Return paths as arrays.
    Paths { include_edges: bool },
    
    /// Return (node, depth) pairs.
    NodesWithDepth,
    
    /// Return shortest path distances.
    Distances,
    
    /// Return blast radius score.
    BlastRadius { decay_factor: f64 },
}
```

### 3.2 `TraversalResult`

```rust
/// Result of a graph traversal, returned as Arrow RecordBatches.
pub struct TraversalResult {
    /// Schema of the result.
    pub schema: SchemaRef,
    
    /// Result batches (streamed).
    pub batches: SendableRecordBatchStream,
    
    /// Traversal statistics.
    pub stats: TraversalStats,
}

#[derive(Debug, Clone)]
pub struct TraversalStats {
    /// Nodes visited during traversal.
    pub nodes_visited: usize,
    
    /// Edges traversed.
    pub edges_traversed: usize,
    
    /// Maximum depth reached.
    pub max_depth_reached: u32,
    
    /// Execution time in microseconds.
    pub execution_time_us: u64,
    
    /// Whether SIMD hot path was used.
    pub used_simd: bool,
    
    /// Cache hit ratio for CSR lookups.
    pub cache_hit_ratio: f64,
}
```

---

## 4. DataFusion Execution Plan Operators

### 4.1 `CSRBuilderExec`

Physical operator that builds CSR from Arrow streams.

```rust
use datafusion::physical_plan::{
    DisplayAs, ExecutionPlan, Partitioning, SendableRecordBatchStream,
};

#[derive(Debug)]
pub struct CSRBuilderExec {
    /// Input execution plan (source data).
    input: Arc<dyn ExecutionPlan>,
    
    /// Ontology for this build.
    ontology: Arc<Ontology>,
    
    /// Target CSR graph (populated during execution).
    target: Arc<CsrGraph>,
    
    /// Build configuration.
    config: CsrBuildConfig,
}

#[derive(Debug, Clone)]
pub struct CsrBuildConfig {
    /// Shard size in bytes (default: 64MB).
    pub shard_size: usize,
    
    /// Parallelism for sorting/compacting.
    pub build_parallelism: usize,
    
    /// Memory limit for build operation.
    pub memory_limit: usize,
    
    /// Spill to disk if memory exceeded.
    pub allow_spill: bool,
}

impl ExecutionPlan for CSRBuilderExec {
    fn as_any(&self) -> &dyn Any;
    fn schema(&self) -> SchemaRef;
    fn output_partitioning(&self) -> Partitioning;
    fn output_ordering(&self) -> Option<&[PhysicalSortExpr]>;
    fn children(&self) -> Vec<Arc<dyn ExecutionPlan>>;
    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> Result<Arc<dyn ExecutionPlan>>;
    
    fn execute(
        &self,
        partition: usize,
        context: Arc<TaskContext>,
    ) -> Result<SendableRecordBatchStream>;
}
```

### 4.2 `GraphTraversalExec`

Physical operator for graph traversals.

```rust
#[derive(Debug)]
pub struct GraphTraversalExec {
    /// The CSR graph to traverse.
    graph: Arc<CsrGraph>,
    
    /// Traversal specification.
    spec: TraversalSpec,
    
    /// Output schema.
    schema: SchemaRef,
    
    /// SIMD configuration.
    simd_config: SimdConfig,
}

#[derive(Debug, Clone)]
pub struct SimdConfig {
    /// Enable SIMD acceleration.
    pub enabled: bool,
    
    /// Preferred SIMD width (auto-detected if None).
    pub preferred_width: Option<SimdWidth>,
    
    /// Minimum batch size for SIMD (below this, use scalar).
    pub min_batch_size: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum SimdWidth {
    Scalar,      // No SIMD
    Sse42,       // 128-bit (4x u32)
    Avx2,        // 256-bit (8x u32)
    Avx512,      // 512-bit (16x u32)
    Neon,        // ARM 128-bit
}

impl ExecutionPlan for GraphTraversalExec {
    // ... standard ExecutionPlan trait implementation
}
```

### 4.3 `GraphJoinExec`

Physical operator for Leapfrog Triejoin pattern matching.

```rust
#[derive(Debug)]
pub struct GraphJoinExec {
    /// The CSR graph.
    graph: Arc<CsrGraph>,
    
    /// Pattern to match (expressed as a mini-graph).
    pattern: GraphPattern,
    
    /// Variable bindings.
    bindings: Vec<PatternBinding>,
    
    /// Output schema.
    schema: SchemaRef,
}

#[derive(Debug, Clone)]
pub struct GraphPattern {
    /// Pattern nodes with optional label constraints.
    pub nodes: Vec<PatternNode>,
    
    /// Pattern edges with optional label/property constraints.
    pub edges: Vec<PatternEdge>,
}

#[derive(Debug, Clone)]
pub struct PatternNode {
    pub variable: String,
    pub label: Option<String>,
    pub properties: HashMap<String, Expr>,
}

#[derive(Debug, Clone)]
pub struct PatternEdge {
    pub from_variable: String,
    pub to_variable: String,
    pub label: Option<String>,
    pub min_hops: u32,
    pub max_hops: u32,
}
```

---

## 5. Arrow C Data Interface (FFI)

### 5.1 Zero-Copy Handoff

```rust
use arrow::ffi::{FFI_ArrowArray, FFI_ArrowSchema};

/// Import Arrow data from external source (e.g., Python, Java).
/// 
/// # Safety
/// Caller must ensure pointers are valid Arrow C Data Interface structs.
pub unsafe fn import_record_batch(
    array: *const FFI_ArrowArray,
    schema: *const FFI_ArrowSchema,
) -> Result<RecordBatch, ArrowError>;

/// Export Arrow data to external consumer.
pub fn export_record_batch(
    batch: &RecordBatch,
) -> Result<(FFI_ArrowArray, FFI_ArrowSchema), ArrowError>;

/// C-compatible struct for graph query results.
#[repr(C)]
pub struct FusionGraphResult {
    /// Arrow array containing result data.
    pub array: FFI_ArrowArray,
    
    /// Arrow schema for result.
    pub schema: FFI_ArrowSchema,
    
    /// Error message (null if success).
    pub error: *const c_char,
    
    /// Traversal statistics.
    pub stats: FusionGraphStats,
}

#[repr(C)]
pub struct FusionGraphStats {
    pub nodes_visited: u64,
    pub edges_traversed: u64,
    pub execution_time_us: u64,
}
```

### 5.2 C API

```c
// fusiongraph.h

typedef struct FusionGraphContext FusionGraphContext;
typedef struct FusionGraphResult FusionGraphResult;

// Initialize a FusionGraph context from ontology file.
FusionGraphContext* fusiongraph_init(const char* ontology_path);

// Free context.
void fusiongraph_free(FusionGraphContext* ctx);

// Execute a traversal query.
FusionGraphResult* fusiongraph_traverse(
    FusionGraphContext* ctx,
    const char* start_label,
    uint64_t start_id,
    uint32_t max_depth,
    const char* edge_labels  // comma-separated, NULL for all
);

// Free result.
void fusiongraph_result_free(FusionGraphResult* result);

// Get error message (NULL if no error).
const char* fusiongraph_last_error(FusionGraphContext* ctx);
```

---

## 6. SQL Extensions

### 6.1 Graph Functions

```sql
-- Traverse N hops from a starting node
SELECT * FROM TABLE(
    graph_traverse(
        start_node => 'User:12345',
        max_depth => 3,
        edge_labels => ARRAY['CAN_ASSUME', 'HAS_POLICY'],
        direction => 'OUTGOING'
    )
);

-- Shortest path between two nodes
SELECT * FROM TABLE(
    graph_shortest_path(
        from_node => 'User:12345',
        to_node => 'Resource:arn:aws:s3:::sensitive-bucket',
        algorithm => 'dijkstra'
    )
);

-- Blast radius scoring
SELECT * FROM TABLE(
    graph_blast_radius(
        start_node => 'Role:arn:aws:iam::123456789:role/AdminRole',
        max_depth => 5,
        decay_factor => 0.8
    )
);

-- Pattern matching (Cypher-like)
SELECT * FROM TABLE(
    graph_match(
        pattern => '(u:User)-[:CAN_ASSUME]->(r:Role)-[:HAS_POLICY]->(p:Policy)',
        where_clause => 'p.policy_name LIKE ''%Admin%'''
    )
);
```

### 6.2 Graph DDL

```sql
-- Register an ontology
CALL graph.register_ontology('/path/to/fusiongraph.toml');

-- Materialize the CSR (optional, normally lazy)
CALL graph.materialize();

-- Force compaction of delta layer
CALL graph.compact();

-- Get graph statistics
SELECT * FROM graph.statistics();

-- Refresh from new Iceberg snapshots
CALL graph.refresh();
```

---

## 7. Configuration

### 7.1 Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FUSIONGRAPH_SHARD_SIZE` | `67108864` (64MB) | CSR shard size in bytes |
| `FUSIONGRAPH_SIMD` | `auto` | SIMD mode: `auto`, `avx512`, `avx2`, `neon`, `scalar` |
| `FUSIONGRAPH_DELTA_THRESHOLD` | `100000` | Delta entries before auto-compact |
| `FUSIONGRAPH_MEMORY_LIMIT` | `0` (unlimited) | Max memory for CSR build |
| `FUSIONGRAPH_SPILL_DIR` | `/tmp/fusiongraph` | Directory for spill files |
| `FUSIONGRAPH_LOG_LEVEL` | `info` | Logging level |

### 7.2 Programmatic Configuration

```rust
use fusiongraph::{FusionGraphConfig, FusionGraphContext};

let config = FusionGraphConfig::builder()
    .shard_size(64 * 1024 * 1024)
    .simd_mode(SimdMode::Auto)
    .delta_threshold(100_000)
    .memory_limit(Some(8 * 1024 * 1024 * 1024)) // 8GB
    .enable_metrics(true)
    .build();

let ctx = FusionGraphContext::new(config)?;
```

---

## 8. Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Ontology parse error: {0}")]
    OntologyParse(#[from] OntologyParseError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Table not found: {0}")]
    TableNotFound(String),

    #[error("Column not found: {table}.{column}")]
    ColumnNotFound { table: String, column: String },

    #[error("Node label not found: {0}")]
    NodeLabelNotFound(String),

    #[error("Edge label not found: {0}")]
    EdgeLabelNotFound(String),

    #[error("CSR build failed: {0}")]
    CsrBuildFailed(String),

    #[error("Out of memory: requested {requested} bytes, available {available}")]
    OutOfMemory { requested: usize, available: usize },

    #[error("Traversal failed: {0}")]
    TraversalFailed(String),

    #[error("SIMD not available: {0}")]
    SimdNotAvailable(String),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 9. Metrics & Observability

### 9.1 Exported Metrics (Prometheus Format)

```
# HELP fusiongraph_nodes_total Total nodes in graph
# TYPE fusiongraph_nodes_total gauge
fusiongraph_nodes_total{label="User"} 1234567

# HELP fusiongraph_edges_total Total edges in graph
# TYPE fusiongraph_edges_total gauge
fusiongraph_edges_total{label="CAN_ASSUME"} 9876543

# HELP fusiongraph_traversal_duration_seconds Traversal execution time
# TYPE fusiongraph_traversal_duration_seconds histogram
fusiongraph_traversal_duration_seconds_bucket{algorithm="bfs",le="0.001"} 1523

# HELP fusiongraph_delta_entries Current delta layer size
# TYPE fusiongraph_delta_entries gauge
fusiongraph_delta_entries 4521

# HELP fusiongraph_memory_bytes Memory usage by component
# TYPE fusiongraph_memory_bytes gauge
fusiongraph_memory_bytes{component="csr_base"} 1073741824
fusiongraph_memory_bytes{component="csr_delta"} 8388608
```

### 9.2 OpenTelemetry Tracing

```rust
// Span names and attributes
fusiongraph.csr_build          // attributes: node_count, edge_count, duration_ms
fusiongraph.traverse.bfs       // attributes: start_node, max_depth, nodes_visited
fusiongraph.traverse.simd      // attributes: simd_width, batches_processed
fusiongraph.delta.compact      // attributes: entries_compacted, duration_ms
fusiongraph.iceberg.refresh    // attributes: snapshots_processed, files_scanned
```
