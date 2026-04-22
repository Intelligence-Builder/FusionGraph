# FusionGraph Error Taxonomy

**Version:** 1.0  
**Status:** Draft

## 1. Overview

This document catalogs all error conditions in FusionGraph, their causes, detection mechanisms, and recovery strategies. Errors are organized by subsystem and severity.

## 2. Error Severity Levels

| Level | Code | Description | User Impact | System Response |
|-------|------|-------------|-------------|-----------------|
| **Fatal** | `F` | Unrecoverable state corruption | Service unavailable | Shutdown + alert |
| **Error** | `E` | Operation failed | Request rejected | Return error, log |
| **Warning** | `W` | Degraded but functional | Possible perf impact | Log, continue |
| **Info** | `I` | Notable condition | None | Log only |

---

## 3. Error Code Format

```
FG-{SUBSYSTEM}-{SEVERITY}{NUMBER}

Examples:
  FG-ONT-E001  → Ontology subsystem, Error #001
  FG-CSR-F001  → CSR subsystem, Fatal #001
  FG-TRV-W003  → Traversal subsystem, Warning #003
```

### Subsystem Codes

| Code | Subsystem |
|------|-----------|
| `ONT` | Ontology parsing/validation |
| `CSR` | CSR build and storage |
| `DLT` | Delta layer |
| `TRV` | Traversal execution |
| `FFI` | Arrow FFI / foreign interface |
| `ICE` | Iceberg integration |
| `DFN` | DataFusion integration |
| `MEM` | Memory management |
| `SIM` | SIMD operations |
| `NET` | Network / Flight |
| `SEC` | Security / authorization |

---

## 4. Ontology Errors (ONT)

### FG-ONT-E001: Parse Error

**Cause:** Malformed TOML/JSON syntax  
**Detection:** Parser exception during `Ontology::from_*`  
**User Message:** `Ontology parse error at line {line}: {details}`  
**Recovery:** Fix syntax, reload

```rust
#[error("FG-ONT-E001: Parse error at line {line}: {message}")]
OntologyParseError { line: usize, message: String }
```

### FG-ONT-E002: Missing Required Field

**Cause:** Required field (label, source, id_column) not present  
**Detection:** Schema validation after parse  
**User Message:** `Missing required field '{field}' in {context}`  
**Recovery:** Add missing field

### FG-ONT-E003: Dangling Edge Reference

**Cause:** Edge references node label that doesn't exist  
**Detection:** Cross-reference validation  
**User Message:** `Edge '{edge_label}' references undefined node '{node_label}'`  
**Recovery:** Define missing node or fix edge reference

### FG-ONT-E004: Duplicate Label

**Cause:** Multiple nodes or edges with same label  
**Detection:** Label uniqueness check  
**User Message:** `Duplicate {node|edge} label '{label}'`  
**Recovery:** Rename to unique labels

### FG-ONT-E005: Invalid ID Transform

**Cause:** ID transform incompatible with column type  
**Detection:** Type checking against catalog schema  
**User Message:** `Cannot apply '{transform}' to column '{column}' of type '{type}'`  
**Recovery:** Choose compatible transform

### FG-ONT-W001: High Cardinality String ID

**Cause:** String ID column without hash transform on large table  
**Detection:** Statistics check (>1M rows)  
**User Message:** `Warning: String ID '{column}' on table with {rows} rows may cause hash collisions`  
**Recovery:** Add `id_transform = "hash_u64"` or use numeric ID

### FG-ONT-W002: Missing Partition Hint

**Cause:** Temporal edge without partition_column  
**Detection:** Schema analysis  
**User Message:** `Warning: Temporal edge '{label}' has no partition hint; full scans may occur`  
**Recovery:** Add `partition_column`

---

## 5. CSR Errors (CSR)

### FG-CSR-F001: Memory Corruption Detected

**Cause:** Internal invariant violation (likely bug or hardware)  
**Detection:** Checksum mismatch or impossible state  
**User Message:** `Fatal: CSR memory corruption detected in shard {shard_id}`  
**Recovery:** **FATAL** - Shutdown, alert ops, rebuild from source

```rust
#[error("FG-CSR-F001: Memory corruption in shard {shard_id}, checksum {expected} != {actual}")]
CsrCorruption { shard_id: u32, expected: u64, actual: u64 }
```

### FG-CSR-E001: Build Failed - Out of Memory

**Cause:** Insufficient RAM for CSR build  
**Detection:** Allocator returns OOM  
**User Message:** `CSR build failed: out of memory (requested {requested}, available {available})`  
**Recovery:**
1. Increase memory limit
2. Enable spill-to-disk (`allow_spill = true`)
3. Reduce shard size
4. Build incrementally by partition

### FG-CSR-E002: Build Failed - Invalid Edge Data

**Cause:** Edge references node ID outside valid range  
**Detection:** Range check during compact  
**User Message:** `Invalid edge: source {from} or target {to} exceeds node count {max}`  
**Recovery:** Check source data for invalid foreign keys

### FG-CSR-E003: Shard File Corrupted

**Cause:** Disk corruption or incomplete write  
**Detection:** Checksum on shard load  
**User Message:** `Shard file {path} corrupted`  
**Recovery:** Delete shard, rebuild from source

### FG-CSR-W001: Shard Imbalance

**Cause:** Uneven node distribution across shards  
**Detection:** Variance check on shard sizes  
**User Message:** `Warning: Shard size variance {variance}% exceeds threshold`  
**Recovery:** Consider rebalancing with different sharding strategy

### FG-CSR-W002: High Degree Node Detected

**Cause:** Single node with extremely high degree (>1M edges)  
**Detection:** Degree histogram during build  
**User Message:** `Warning: Node {id} has {degree} edges; may cause traversal hotspots`  
**Recovery:** Informational; consider sampling for very high degree nodes

---

## 6. Delta Layer Errors (DLT)

### FG-DLT-E001: Delta Overflow

**Cause:** Delta layer exceeds threshold without compaction  
**Detection:** Entry count > `delta_threshold`  
**User Message:** `Delta layer overflow: {count} entries exceed threshold {threshold}`  
**Recovery:**
1. Trigger manual compaction: `CALL graph.compact()`
2. Increase threshold
3. Enable auto-compaction

### FG-DLT-E002: Compaction Failed

**Cause:** Error during delta-to-base merge  
**Detection:** Merge operation exception  
**User Message:** `Delta compaction failed: {reason}`  
**Recovery:**
1. Retry compaction
2. If persistent, dump delta to file for recovery
3. Rebuild base from source + delta dump

### FG-DLT-W001: High Tombstone Ratio

**Cause:** Many deletions without compaction  
**Detection:** tombstones / total > 0.3  
**User Message:** `Warning: {ratio}% of delta entries are tombstones`  
**Recovery:** Run compaction to reclaim space

---

## 7. Traversal Errors (TRV)

### FG-TRV-E001: Start Node Not Found

**Cause:** Traversal start node doesn't exist  
**Detection:** Node lookup returns None  
**User Message:** `Start node '{node_id}' not found in graph`  
**Recovery:** Verify node ID, check if graph is materialized

### FG-TRV-E002: Invalid Traversal Spec

**Cause:** Malformed traversal parameters  
**Detection:** Spec validation  
**User Message:** `Invalid traversal: {reason}`  
**Recovery:** Fix parameters (e.g., max_depth > 0)

### FG-TRV-E003: Traversal Timeout

**Cause:** Traversal exceeded time limit  
**Detection:** Timer expiry  
**User Message:** `Traversal timed out after {duration}ms (visited {nodes} nodes)`  
**Recovery:**
1. Reduce max_depth
2. Add edge/node filters
3. Increase timeout
4. Use sampling for exploratory queries

### FG-TRV-E004: Cycle Limit Exceeded

**Cause:** Infinite loop detection triggered  
**Detection:** Visit count > threshold on single node  
**User Message:** `Cycle detected: node {id} visited {count} times`  
**Recovery:** Graph contains cycles; use appropriate algorithm (BFS handles naturally)

### FG-TRV-W001: Large Result Set

**Cause:** Traversal returned very large result  
**Detection:** Result count > warning threshold  
**User Message:** `Warning: Traversal returned {count} results; consider adding LIMIT`  
**Recovery:** Add LIMIT clause or tighter filters

### FG-TRV-W002: SIMD Fallback

**Cause:** SIMD not available or data not aligned  
**Detection:** Runtime feature detection  
**User Message:** `Warning: SIMD unavailable, using scalar fallback (expected {expected_speedup}x slower)`  
**Recovery:** Informational; ensure AVX-512/Neon available for production

---

## 8. FFI Errors (FFI)

### FG-FFI-F001: Use After Free

**Cause:** External code used Arrow array after FusionGraph freed it  
**Detection:** Address sanitizer or crash  
**User Message:** N/A (crash)  
**Recovery:** **FATAL** - Bug in external code; ensure proper ownership transfer

### FG-FFI-E001: Invalid Arrow Schema

**Cause:** Imported Arrow data has incompatible schema  
**Detection:** Schema validation on import  
**User Message:** `Arrow schema mismatch: expected {expected}, got {actual}`  
**Recovery:** Fix upstream data producer

### FG-FFI-E002: Null Pointer

**Cause:** External code passed null pointer  
**Detection:** Null check on entry  
**User Message:** `Null pointer passed to {function}`  
**Recovery:** Fix external code

### FG-FFI-W001: Alignment Warning

**Cause:** Arrow buffers not optimally aligned  
**Detection:** Alignment check  
**User Message:** `Warning: Buffer alignment {alignment} may reduce SIMD efficiency`  
**Recovery:** Ensure Arrow buffers are 64-byte aligned

---

## 9. Iceberg Errors (ICE)

### FG-ICE-E001: Catalog Connection Failed

**Cause:** Cannot connect to Iceberg catalog  
**Detection:** Connection timeout/refusal  
**User Message:** `Cannot connect to Iceberg catalog at {uri}: {reason}`  
**Recovery:**
1. Check network connectivity
2. Verify catalog URI
3. Check credentials

### FG-ICE-E002: Table Not Found

**Cause:** Ontology references non-existent table  
**Detection:** Catalog lookup  
**User Message:** `Table '{table}' not found in catalog '{catalog}'`  
**Recovery:** Verify table name, check catalog permissions

### FG-ICE-E003: Manifest Parse Error

**Cause:** Corrupted or incompatible manifest file  
**Detection:** Manifest parser exception  
**User Message:** `Failed to parse manifest {path}: {reason}`  
**Recovery:** Check Iceberg version compatibility; may need table repair

### FG-ICE-E004: Snapshot Expired

**Cause:** Requested snapshot no longer exists  
**Detection:** Snapshot lookup  
**User Message:** `Snapshot {snapshot_id} expired or not found`  
**Recovery:** Use current snapshot or extend retention

### FG-ICE-W001: Stale Snapshot

**Cause:** Using snapshot older than threshold  
**Detection:** Timestamp comparison  
**User Message:** `Warning: Using snapshot from {timestamp}, {age} hours old`  
**Recovery:** Call `graph.refresh()` for latest data

---

## 10. DataFusion Errors (DFN)

### FG-DFN-E001: Plan Generation Failed

**Cause:** Cannot generate physical plan for query  
**Detection:** Planner exception  
**User Message:** `Failed to generate execution plan: {reason}`  
**Recovery:** Check query syntax; may be unsupported operation

### FG-DFN-E002: Operator Execution Failed

**Cause:** Physical operator failed during execution  
**Detection:** Operator returns error  
**User Message:** `Execution failed in {operator}: {reason}`  
**Recovery:** Check input data; may be resource exhaustion

### FG-DFN-E003: Type Coercion Failed

**Cause:** Cannot coerce types for graph operation  
**Detection:** Type checker  
**User Message:** `Cannot coerce '{from_type}' to '{to_type}' for {operation}`  
**Recovery:** Explicit cast or fix schema

---

## 11. Memory Errors (MEM)

### FG-MEM-F001: Allocator Exhausted

**Cause:** System out of memory, allocator cannot proceed  
**Detection:** Allocator panic  
**User Message:** N/A (crash)  
**Recovery:** **FATAL** - Increase system memory; enable swap

### FG-MEM-E001: Memory Limit Exceeded

**Cause:** Operation would exceed configured limit  
**Detection:** Pre-allocation check  
**User Message:** `Operation requires {required} bytes, limit is {limit}`  
**Recovery:**
1. Increase `memory_limit`
2. Enable spill-to-disk
3. Process in smaller batches

### FG-MEM-E002: Spill Failed

**Cause:** Cannot spill to disk (full, permissions, etc.)  
**Detection:** Write failure  
**User Message:** `Failed to spill to {path}: {reason}`  
**Recovery:** Check disk space and permissions

### FG-MEM-W001: High Memory Pressure

**Cause:** Memory usage approaching limit  
**Detection:** Usage > 80% of limit  
**User Message:** `Warning: Memory usage at {percent}% of limit`  
**Recovery:** Informational; consider increasing limit

### FG-MEM-W002: Frequent GC

**Cause:** Epoch reclamation running frequently  
**Detection:** Reclamation rate > threshold  
**User Message:** `Warning: High memory churn detected`  
**Recovery:** May indicate inefficient access pattern; profile

---

## 12. SIMD Errors (SIM)

### FG-SIM-E001: Illegal Instruction

**Cause:** SIMD instruction not supported on CPU  
**Detection:** SIGILL handler  
**User Message:** `SIMD instruction {instr} not supported`  
**Recovery:** Auto-fallback to scalar; check CPU feature detection

### FG-SIM-W001: Suboptimal SIMD Width

**Cause:** Using narrower SIMD than CPU supports  
**Detection:** Feature detection mismatch  
**User Message:** `Warning: Using {actual} instead of available {optimal}`  
**Recovery:** Check build flags; may need recompile for target

---

## 13. Security Errors (SEC)

### FG-SEC-E001: Unauthorized Table Access

**Cause:** User lacks permission to read table  
**Detection:** Catalog ACL check  
**User Message:** `Access denied to table '{table}'`  
**Recovery:** Request permissions from data owner

### FG-SEC-E002: Credential Expired

**Cause:** Auth token expired during long operation  
**Detection:** Storage layer auth failure  
**User Message:** `Credentials expired; please re-authenticate`  
**Recovery:** Refresh credentials; consider longer token TTL

### FG-SEC-E003: Audit Log Write Failed

**Cause:** Cannot write to audit/access history  
**Detection:** Audit write failure  
**User Message:** `Error: Audit logging failed; operation blocked`  
**Recovery:** Fix audit infrastructure before proceeding

---

## 14. Error Handling Patterns

### 14.1 Rust Error Propagation

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphError {
    // Ontology errors
    #[error("FG-ONT-E001: Parse error at line {line}: {message}")]
    OntologyParse { line: usize, message: String },
    
    #[error("FG-ONT-E003: Edge '{edge}' references undefined node '{node}'")]
    DanglingEdge { edge: String, node: String },
    
    // CSR errors
    #[error("FG-CSR-E001: Out of memory (requested {requested}, available {available})")]
    OutOfMemory { requested: usize, available: usize },

    #[error("FG-CSR-F001: Corruption detected in shard {shard_id}")]
    CsrCorruption { shard_id: u32, expected: u64, actual: u64 },

    #[error("FG-TRV-E001: Node {id} not found")]
    NodeNotFound { id: NodeId },

    #[error("FG-MEM-E001: Memory limit exceeded ({requested} > {limit})")]
    MemoryLimitExceeded { requested: usize, limit: usize },

    #[error("FG-ICE-E001: Iceberg connection failed: {message}")]
    IcebergConnectionFailed { message: String },

    #[error("FG-AUT-E001: Credentials expired")]
    CredentialExpired,

    #[error("FG-SYS-E001: Circuit breaker open")]
    CircuitOpen,
    
    // ... etc
    
    // Wrapped errors from dependencies
    #[error("FG-ICE-E003: Iceberg error: {0}")]
    Iceberg(#[from] iceberg::Error),
    
    #[error("FG-DFN-E001: DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    
    #[error("FG-FFI-E001: Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
}

impl GraphError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::OntologyParse { .. } => "FG-ONT-E001",
            Self::DanglingEdge { .. } => "FG-ONT-E003",
            Self::OutOfMemory { .. } => "FG-CSR-E001",
            Self::CsrCorruption { .. } => "FG-CSR-F001",
            Self::NodeNotFound { .. } => "FG-TRV-E001",
            Self::MemoryLimitExceeded { .. } => "FG-MEM-E001",
            Self::IcebergConnectionFailed { .. } => "FG-ICE-E001",
            Self::CredentialExpired => "FG-AUT-E001",
            Self::CircuitOpen => "FG-SYS-E001",
            // ...
        }
    }
    
    pub fn severity(&self) -> Severity {
        match self {
            Self::CsrCorruption { .. } => Severity::Fatal,
            Self::OutOfMemory { .. } => Severity::Error,
            // ...
        }
    }
    
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            Self::IcebergConnectionFailed { .. } |
            Self::CredentialExpired |
            Self::MemoryLimitExceeded { .. }
        )
    }
}
```

### 14.2 Graceful Degradation

```rust
impl CsrGraph {
    pub fn neighbors_safe(&self, node: NodeId) -> Result<NeighborIter<'_>, GraphError> {
        // Check node exists
        if node.0 >= self.node_count() as u64 {
            return Err(GraphError::NodeNotFound { id: node });
        }
        
        // Check for corruption
        let shard = self.shard_for(node);
        if !shard.verify_checksum() {
            // Log fatal, but try to continue with other shards
            tracing::error!(error_code = "FG-CSR-F001", node = ?node, "Shard corrupted");
            return Err(GraphError::CsrCorruption { 
                shard_id: shard.id,
                expected: shard.expected_checksum,
                actual: shard.actual_checksum(),
            });
        }
        
        Ok(self.neighbors_unchecked(node))
    }
}
```

### 14.3 Circuit Breaker

```rust
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure: AtomicU64,
    state: AtomicU8, // 0=Closed, 1=Open, 2=HalfOpen
}

impl CircuitBreaker {
    const FAILURE_THRESHOLD: u32 = 5;
    const RESET_TIMEOUT_MS: u64 = 30_000;
    
    pub fn call<T, E>(&self, f: impl FnOnce() -> Result<T, E>) -> Result<T, GraphError> {
        match self.state.load(Ordering::Acquire) {
            0 => { // Closed - normal operation
                match f() {
                    Ok(v) => {
                        self.failure_count.store(0, Ordering::Release);
                        Ok(v)
                    }
                    Err(_) => {
                        if self.failure_count.fetch_add(1, Ordering::AcqRel) >= Self::FAILURE_THRESHOLD {
                            self.state.store(1, Ordering::Release);
                            self.last_failure.store(now_ms(), Ordering::Release);
                        }
                        Err(GraphError::CircuitOpen)
                    }
                }
            }
            1 => { // Open - fail fast
                if now_ms() - self.last_failure.load(Ordering::Acquire) > Self::RESET_TIMEOUT_MS {
                    self.state.store(2, Ordering::Release);
                    self.call(f) // Try half-open
                } else {
                    Err(GraphError::CircuitOpen)
                }
            }
            2 => { // Half-open - test
                match f() {
                    Ok(v) => {
                        self.state.store(0, Ordering::Release);
                        self.failure_count.store(0, Ordering::Release);
                        Ok(v)
                    }
                    Err(_) => {
                        self.state.store(1, Ordering::Release);
                        self.last_failure.store(now_ms(), Ordering::Release);
                        Err(GraphError::CircuitOpen)
                    }
                }
            }
            _ => unreachable!()
        }
    }
}
```

---

## 15. Logging & Alerting

### 15.1 Structured Logging

```rust
use tracing::{error, warn, info, instrument};

#[instrument(skip(graph), fields(error_code))]
pub fn traverse(graph: &CsrGraph, start: NodeId, max_depth: u32) -> Result<TraversalResult, GraphError> {
    if !graph.contains(start) {
        tracing::Span::current().record("error_code", "FG-TRV-E001");
        error!(node = ?start, "Start node not found");
        return Err(GraphError::StartNodeNotFound { node_id: start });
    }
    
    // ...
}
```

### 15.2 Alert Rules

```yaml
# alertmanager/rules/fusiongraph.yml
groups:
  - name: fusiongraph
    rules:
      - alert: FusionGraphFatalError
        expr: increase(fusiongraph_errors_total{severity="fatal"}[5m]) > 0
        labels:
          severity: critical
        annotations:
          summary: "FusionGraph fatal error detected"
          description: "Error code {{ $labels.error_code }}"

      - alert: FusionGraphHighErrorRate
        expr: rate(fusiongraph_errors_total{severity="error"}[5m]) > 10
        labels:
          severity: warning
        annotations:
          summary: "FusionGraph error rate elevated"

      - alert: FusionGraphMemoryPressure
        expr: fusiongraph_memory_bytes / fusiongraph_memory_limit_bytes > 0.9
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "FusionGraph memory usage above 90%"
```

---

## 16. Recovery Procedures

### 16.1 CSR Corruption Recovery

```bash
#!/bin/bash
# recovery/rebuild_csr.sh

echo "=== FusionGraph CSR Recovery ==="

# 1. Stop queries
fusiongraph-cli pause --wait

# 2. Backup corrupted state
mkdir -p /backup/fusiongraph/$(date +%Y%m%d)
cp -r /var/lib/fusiongraph/shards /backup/fusiongraph/$(date +%Y%m%d)/

# 3. Clear corrupted shards
rm -rf /var/lib/fusiongraph/shards/*

# 4. Rebuild from source
fusiongraph-cli materialize --force

# 5. Verify
fusiongraph-cli verify --checksums

# 6. Resume
fusiongraph-cli resume
```

### 16.2 Delta Layer Recovery

```bash
#!/bin/bash
# recovery/compact_delta.sh

# Force compaction if delta overflow
fusiongraph-cli compact --force

# If compaction fails, dump and rebuild
if [ $? -ne 0 ]; then
    echo "Compaction failed, dumping delta..."
    fusiongraph-cli delta dump /tmp/delta_backup.arrow
    fusiongraph-cli delta clear
    fusiongraph-cli delta restore /tmp/delta_backup.arrow
fi
```

---

## 17. Error Metrics

```rust
use prometheus::{IntCounterVec, register_int_counter_vec};

lazy_static! {
    static ref ERRORS: IntCounterVec = register_int_counter_vec!(
        "fusiongraph_errors_total",
        "Total errors by code and severity",
        &["error_code", "severity", "subsystem"]
    ).unwrap();
}

impl GraphError {
    pub fn record(&self) {
        ERRORS.with_label_values(&[
            self.code(),
            self.severity().as_str(),
            self.subsystem(),
        ]).inc();
    }
}
```
