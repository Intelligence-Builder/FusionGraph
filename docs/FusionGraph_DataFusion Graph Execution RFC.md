
# ---

**FusionGraph RFC**

**To:** Apache DataFusion Contrib Repository (datafusion-contrib)

**Subject:** \[RFC\] FusionGraph: A Native Graph Execution Extension for DataFusion

**Status:** Draft / Proposal

## **1\. Abstract**

We propose **FusionGraph**, a new sub-project within datafusion-contrib dedicated to providing a native graph execution engine. FusionGraph extends DataFusion’s physical planning capabilities to support high-concurrency graph traversals and topological algorithms directly on Arrow-native data sources (Iceberg, Parquet, Flight) without data movement.

## **2\. Motivation**

While DataFusion is the industry standard for relational query execution in Rust, it currently treats relationship-heavy queries (e.g., multi-hop traversals, shortest paths) as standard joins. This relational approach is computationally latent ($O(\\log N)$) compared to specialized graph structures ($O(1)$). FusionGraph bridges this gap by introducing **Graph Physical Operators** that utilize CSR (Compressed Sparse Row) indexing within the DataFusion pipeline.

## **3\. Proposed Architecture**

### **A. The GraphTableProvider Trait**

We introduce a new trait that extends TableProvider. It allows users to define an "Ontology Map" via TOML/JSON, designating specific tables as Nodes or Edges.

* **Metadata Pruning:** The provider intercepts query filters and uses Iceberg manifest statistics to skip files that do not contain relevant edges.

### **B. Graph-Native Execution Plan**

FusionGraph adds new ExecutionPlan nodes:

* **GraphTraversalExec:** Performs SIMD-accelerated BFS/DFS.  
* **GraphJoinExec:** Implements multi-way joins (Leapfrog Triejoin style) for pattern matching.  
* **CSRBuilderExec:** A physical operator that consumes Arrow RecordBatches and materializes a transient CSR matrix in memory.

### **C. SIMD-Accelerated Hot Path**

The kernel uses **AVX-512** and **ARM Neon** intrinsics to evaluate 8–16 neighbors per clock cycle. This ensures that "Blast Radius" and "Centrality" queries achieve sub-millisecond latencies on 100M+ edge subgraphs.

## **4\. Design Patterns & Prior Art**

* **FDAP Stack:** Leverages the Flight-DataFusion-Arrow-Parquet pattern for high-speed cross-cloud execution.  
* **Arrow-Graph Synergy:** Builds upon the performance benchmarks of the arrow-graph crate, transitioning from a UDF-based model to a first-class execution operator.  
* **Epoch-Based Memory:** Adopts the "Wait-Free" memory management patterns from high-performance systems programming to handle real-time lakehouse updates.

## **5\. Implementation Roadmap**

1. **M1:** Basic GraphTableProvider for Iceberg and local Parquet.  
2. **M2:** Implementation of the CSRBuilderExec operator and basic BFS traversal.  
3. **M3:** SIMD optimization for GraphTraversalExec and integration with DataFusion’s logical optimizer for pattern rewriting.  
4. **M4:** Support for Substrait-based graph plan serialization.

## **6\. Community & Governance**

Following the **Apache Way**, FusionGraph will be open-source (Apache 2.0) and managed via meritocratic governance. Initial maintainers will focus on hardening the FFI (Foreign Function Interface) to allow for use within **Snowpark Container Services (SPCS)** and other cloud-native runtimes.

### ---
