# ---

**FusionGraph: Technical Specification (V4.0)**

## **1\. Executive Summary**

**FusionGraph** is an open-source, high-performance graph execution kernel built in Rust. It is designed to function as a "Zero-ETL" extension for Apache DataFusion, allowing users to project Apache Iceberg and Parquet data lakes into a navigable graph in-memory. By treating the data cloud as a **Virtual Graph Operating System**, FusionGraph eliminates the "data movement tax" and enables sub-millisecond relationship traversals directly within governed data perimeters like **Snowflake Horizon**.

## **2\. Design Pillars**

* **Zero-ETL Projection:** No static loading into a specialized database; the graph is a transient state projected from existing tables.  
* **Arrow-Native:** Utilizes Apache Arrow as the shared memory backplane for zero-copy handoffs.  
* **Topological Speed:** Achieves $O(1)$ neighbor lookups using **CSR-in-RAM** structures.  
* **Agentic Readiness:** Built to provide stateful memory and "Action" orchestration for LLM-based research agents.

## **3\. System Architecture**

### **3.1. The Storage & Integration Layer (The Catalog Trait)**

FusionGraph leverages **Metadata-Aware Schema Mapping**. Instead of scanning entire tables, it reads Iceberg manifest files and Snowflake INFORMATION\_SCHEMA to identify node-labels and edge-types.

* **Selective Projection:** Only the specific Parquet columns required for the graph topology are streamed into the kernel.  
* **Pushdown Optimization:** Filters are pushed down to the storage layer, ensuring the kernel only "sees" the relevant subgraph.

### **3.2. The Compute Layer (Rust CSR Kernel)**

The core execution engine is a **Chunked, Bitset-Masked LSM-Graph**.

* **Compressed Sparse Row (CSR):** Topology is stored in contiguous memory arrays (Nodes, Edges, and Properties) to maximize L1/L2 cache hits.  
* **Micro-Sharding (V3 Update):** The Base Layer is partitioned into **64MB shards** to prevent "Compaction Walls," reducing memory overhead during merges from $2\\times$ RAM to $\\text{Total RAM} \+ 64\\text{MB}$.  
* **SIMD Pipeline (AVX-512):** The kernel uses **Single Instruction, Multiple Data** instructions to evaluate 16 node-IDs per clock cycle during breadth-first searches (BFS).

### **3.3. Memory & Concurrency Management**

FusionGraph implements **Wait-Free** execution patterns to handle high-concurrency investigative research.

* **Epoch-Based Reclamation:** Uses crossbeam-epoch to swap memory chunks safely. Old memory is reclaimed only after all active threads move to a new epoch, eliminating "Use-After-Free" bugs.  
* **Dual-Layer Mutability:**  
  * **Base Layer:** Immutable, SIMD-optimized CSR shards.  
  * **Delta Layer:** A lock-free DashMap for real-time updates and "Zombie Edge" tombstones.  
  * **Dirty Bitset:** A dense bitset flags "Dirty" nodes, signaling the engine to switch from the high-speed SIMD path to the Delta Map logic.

## **4\. Functional Specifications**

| ID | Feature | Description |
| :---- | :---- | :---- |
| **FR1** | **Zero-Copy FFI** | Use the Arrow C Data Interface to ingest RecordBatches from DataFusion without serialization. |
| **FR2** | **Multi-Hop Traversal** | Native support for N-hop pathfinding, Shortest Path, and "Blast Radius" scoring. |
| **FR3** | **LSM Mutability** | Implement the Base \+ Delta model to allow real-time graph updates without pausing readers. |
| **FR4** | **Substrait Support** | Support Substrait-based graph plan serialization for cross-platform interoperability. |
| **FR5** | **Agentic Handoff** | Provide a semantic interface for LLM agents to trigger "Actions" (e.g., revoking IAM roles) based on topological findings. |

## **5\. Deployment Model (Snowflake Native App)**

For enterprise deployment, FusionGraph is packaged as a **Snowflake Native App** running on **Snowpark Container Services (SPCS)**.

* **Security:** Data remains behind the consumer’s firewall, governed by **Snowflake Horizon** policies.  
* **Output:** The kernel flattens complex graph outputs into **Graph-Enriched Dynamic Iceberg Tables**, making insights available to standard BI tools (Tableau, Looker) via SQL.

## **6\. Strategic Roadmap**

* **Phase 1 (The Bridge):** Build the GraphTableProvider for Iceberg and local Parquet.  
* **Phase 2 (The Core):** Implement the CSRBuilderExec operator and basic BFS/Dijkstra in the Rust binary.  
* **Phase 3 (Optimization):** Integrate **AVX-512 SIMD** hot-paths and datafusion-tokomak for global plan optimization.  
* **Phase 4 (Ecosystem):** Formalize as an official datafusion-contrib sub-project.

---

