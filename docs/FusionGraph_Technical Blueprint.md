This comprehensive technical blueprint expands the **FusionGraph** workstreams into granular tasks, each accompanied by a specific **Success Criteria / Definition of Done (DoD)**. This ensures that the development of the "Virtual Graph Operating System" meets the high-concurrency and memory-safety requirements of the architecture.

### ---

**Workstream 1: Core Kernel & Memory (Low-Level Systems)**

*Focus: Implementing the high-speed CSR topology and LSM-style mutability.*

#### **Task 1.1: The Micro-Sharded CSR Layout**

* **Engineering Detail:** Implement the CSRShard struct using Arc\<Vec\<u32\>\>. Develop the shard-indexing logic to map a global NodeID to a specific (ShardID, Offset) coordinate.  
* **Success Criteria / DoD:**  
  * System handles 100M+ edges on a single node with \< 5% pointer overhead.  
  * Unit tests prove that accessing a node's neighborhood is an $O(1)$ operation.  
  * Memory footprint remains strictly at $\\text{Total RAM} \+ 64\\text{MB}$ during simulated shard updates.

#### **Task 1.2: SIMD-Accelerated BFS Hot Path**

* **Engineering Detail:** Write assembly-level or std::arch intrinsics for **AVX-512** (ZMM registers). Implement the bitset-masking loop to evaluate 16 neighbors per cycle without branch mispredictions.  
* **Success Criteria / DoD:**  
  * Benchmarking shows a minimum $8\\times$ speedup for "Cold" traversals compared to non-SIMD Rust iterators.  
  * Zero branch mispredictions are recorded in the hot-path loop during a 3-hop traversal.  
  * Hardware detection logic correctly falls back to SSE4.2/Neon on non-AVX-512 systems.

#### **Task 1.3: Epoch-Based Reclamation (EBR)**

* **Engineering Detail:** Integrate crossbeam-epoch to guard shard pointers. Implement the pin() mechanism for all reader threads.  
* **Success Criteria / DoD:**  
  * Zero "Use-After-Free" or "Segment Fault" errors during 1,000 concurrent updates and 10,000 concurrent reads.  
  * Old memory chunks are reclaimed within 50ms of the last thread exiting the old epoch.

#### **Task 1.4: LSM Delta Map & "Zombie" Validation**

* **Engineering Detail:** Build a lock-free DashMap for real-time edge insertions. Implement the validation logic where the Delta layer checks for tombstones (deletions) against the Base layer.  
* **Success Criteria / DoD:**  
  * Write throughput for the Delta Layer exceeds 500k edges/sec.  
  * A traversal correctly ignores a "tombstoned" edge in the Base layer immediately after a delete is committed to the Delta layer.

### ---

**Workstream 2: DataFusion Integration (Compute Bridge)**

*Focus: Connecting the Rust kernel to the DataFusion physical execution plan.*

#### **Task 2.1: CSRBuilderExec Physical Operator**

* **Engineering Detail:** Create a custom ExecutionPlan that consumes RecordBatch streams. Implement the "In-Place Sort-and-Compact" logic to build contiguous CSR arrays.  
* **Success Criteria / DoD:**  
  * Operator successfully transforms a 1GB Arrow table into a CSR structure in \< 2 seconds.  
  * Operator correctly implements the async stream interface, allowing DataFusion to pipe data without blocking.

#### **Task 2.2: GraphTraversalExec Physical Operator**

* **Engineering Detail:** Develop the operator that translates DataFusion "Joined" columns into the Kernel’s internal NodeID format and executes the traversal.  
* **Success Criteria / DoD:**  
  * Operator passes end-to-end integration tests where a SQL query triggers a BFS in the kernel.  
  * Output is returned as a standard RecordBatch, making it indistinguishable from relational data to the rest of the plan.

#### **Task 2.3: Logical Optimizer Rule (Fusion)**

* **Engineering Detail:** Implement a QueryOptimizerRule that detects $N$-hop self-joins or recursive CTEs and collapses them into a GraphTraversalExec node.  
* **Success Criteria / DoD:**  
  * The EXPLAIN plan for a 3-hop query shows exactly one GraphTraversalExec node instead of multiple HashJoin nodes.  
  * Optimization overhead adds \< 10ms to the total query planning time.

### ---

**Workstream 3: The Lakehouse Provider (Zero-ETL Ingestion)**

*Focus: Metadata-aware projection from Iceberg and Parquet.*

#### **Task 3.1: Iceberg Manifest-Level Pruning**

* **Engineering Detail:** Build a parser to interpret Iceberg manifest lists. Implement logic to skip Parquet files based on min/max stats for NodeID or RelationshipType.  
* **Success Criteria / DoD:**  
  * In a 1TB table, the system skips 90%+ of files when querying for a specific AccountID or VPC\_ID.  
  * The parser correctly handles Iceberg V2 "Delete Files" without re-triggering a full table scan.

#### **Task 3.2: Arrow C Data Interface Handoff**

* **Engineering Detail:** Implement the FFI bridge using ArrowArray and ArrowSchema structs to pass raw pointers from DataFusion to the CSR buffer.  
* **Success Criteria / DoD:**  
  * Serialization time for a 10M edge batch is measured at 0.0ms (true zero-copy).  
  * Memory safety is maintained across the FFI boundary, verified by Valgrind or Miri.

### ---

**Workstream 4: Governance & Deployment (Snowflake/SPCS)**

*Focus: Security boundary management and Native App packaging.*

#### **Task 4.1: Snowflake Horizon Security Proxy**

* **Engineering Detail:** Configure the container to use Snowflake Scoped Credentials. Implement a hook that writes every traversal start/stop to the ACCESS\_HISTORY table.  
* **Success Criteria / DoD:**  
  * The kernel successfully reads an Iceberg table on S3 without the developer providing static AWS keys.  
  * A Snowflake Security Admin can view every "Graph Research" event in the native Snowflake audit logs.

#### **Task 4.2: OCI Multi-Stage Build & Portability**

* **Engineering Detail:** Create a Dockerfile that produces a \< 100MB OCI image. Optimize for x86\_64 (Snowflake) and aarch64 (Graviton/ARM).  
* **Success Criteria / DoD:**  
  * The exact same image successfully starts and passes health checks in both a local Docker environment and a Snowflake SPCS instance.  
  * Startup time for the container is \< 5 seconds.

#### **Task 4.3: Secure Proxy UDFs & Dynamic Table DDL**

* **Engineering Detail:** Write the Python/Java UDFs that act as the interface between SQL and the SPCS container. Develop the CREATE DYNAMIC TABLE template for enriched graph outputs.  
* **Success Criteria / DoD:**  
  * Users can run SELECT get\_blast\_radius(resource\_id) from a standard Snowflake worksheet.  
  * The Dynamic Table refreshes incrementally, only updating nodes affected by new Iceberg data.