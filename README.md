# FusionGraph
Graph Kernel Integrated with Apache DataFusion and Arrow

FusionGraph is a "Zero-ETL" graph execution layer that treats the Data Lakehouse (Iceberg/Parquet) as a virtualized adjacency list. It eliminates the "Data Movement Tax" by fusing graph traversals into the physical execution plan of the analytical engine.
1. Architectural Layers
Storage (The Catalog Trait): Deep integration with Apache Iceberg and Snowflake Horizon. It uses a metadata-aware mapper to prune Parquet files at the manifest level before reading data.
Projection (Zero-Copy Bridge): Utilizes the Arrow C Data Interface to stream data from DataFusion's readers directly into the graph kernel without serialization.
Kernel (CSR Core): A high-performance Rust binary that maintains topology in Compressed Sparse Row (CSR) format.
Intelligence (ReflexArc): A semantic layer that orchestrates multi-hop traversals using SIMD (AVX-512) and triggers agentic actions based on topological findings.
2. Functional Pillars
LSM-Graph Mutability: A dual-layer model (Base + Delta) that allows for real-time updates without stalling the SIMD hot path.
Epoch-Based Reclamation: Lock-free memory swapping that ensures wait-free traversals during background chunk compaction.
Hot/Warm/Cold Tiering: * Hot: Active research context pinned in CSR-RAM.
Warm: Recently accessed subgraphs in local NVMe/Page Cache.
Cold: Raw Iceberg data in S3/Snowflake.
