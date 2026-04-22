# **The Architecture of Graph-Based Query Execution and Multi-Modal Data Fusion in the Apache DataFusion Ecosystem**

The modern data landscape is increasingly defined by the "Deconstructed Database" paradigm, a shift from monolithic, tightly coupled systems to modular, interoperable components designed for specific high-performance workloads.1 At the heart of this transformation is Apache DataFusion, an extensible, multi-threaded, and vectorized query engine written in Rust that leverages Apache Arrow as its core in-memory format.3 While fundamentally an analytical engine for relational data, DataFusion’s internal design is rooted in graph theory, specifically through its representation of query plans as Directed Acyclic Graphs (DAGs).2 Furthermore, the extensibility of the framework has enabled a burgeoning ecosystem of graph-native analytics and multi-modal data fusion techniques that bridge the gap between structured relational records and complex, interconnected information networks.6

The architectural brilliance of DataFusion lies in its ability to abstract the "what" from the "how." Users interact with the system via declarative interfaces such as SQL or the DataFrame API, which the engine subsequently translates into a series of graph-based intermediate representations.8 This translation process is not merely a syntactic conversion but a sophisticated mapping of relational algebra onto a dataflow graph where nodes represent computational operators and edges represent the stream of Arrow-formatted data batches.2 This report explores the nuances of these graph-based representations, the implementation of graph algorithms within relational frameworks, and the emerging methodologies for multi-modal data integration that utilize graph structures to synthesize text, images, and sensor data.

## **Architectural Foundations: The Internal Query Graph**

In DataFusion, every query undergoes a transformation from a high-level logical abstraction to a low-level physical execution strategy. Both of these stages are represented as graphs, providing a structured way for the optimizer to reason about and transform the dataflow.5

### **Logical Plan DAGs and Relational Abstraction**

The LogicalPlan is the primary intermediate representation (IR) in DataFusion. It describes the query’s intent without committing to a specific execution algorithm or hardware configuration.5 A LogicalPlan is implemented as a recursive enum in Rust, forming a tree-like DAG where each node is a relational operator such as Projection, Filter, Join, or Aggregate.5 This structure ensures that data flows from the leaf nodes—typically TableScan operations—up to the root, which produces the final result set.9

| Operator Type | Logical Representation | Purpose in the Graph |
| :---- | :---- | :---- |
| TableScan | TableScan | Leaf node; identifies the data source and schema 8 |
| Projection | Projection | Selects or computes a subset of columns 1 |
| Filter | Filter | Evaluates boolean expressions to prune rows 4 |
| Join | Join | Combines two input streams based on predicates 8 |
| Aggregate | Aggregate | Performs group-by and reduction operations 9 |
| Extension | Extension | Custom operator provided by external crates 5 |

The logical plan serves as a canonical form. For example, a Common Table Expression (CTE) and a subquery are often normalized into the same logical representation, simplifying the downstream optimization passes.14 This standardization allows developers building custom query languages to target the LogicalPlan directly, bypassing SQL parsing entirely and leveraging DataFusion’s sophisticated optimizer and execution engine.5

### **Execution Plan DAGs and Physical Realization**

Once the logical plan is optimized, it is translated into an ExecutionPlan, often referred to as the physical plan.5 The physical plan is also a DAG, but it incorporates detailed information about the execution strategy. Each node in this graph implements a pull-based interface where it requests data from its children in the form of Arrow RecordBatches.5

The physical plan is sensitive to the underlying data’s partitioning and sortedness. For instance, a physical join node might decide between a HashJoin or a MergeJoin based on whether the input streams are already sorted.4 This graph-based execution flow is inherently multi-threaded; the engine analyzes the DAG to identify branches that can be executed in parallel across multiple CPU cores, maximizing the throughput of the vectorized execution engine.2

### **Plan Visualization and Introspection**

A critical feature for developers and database administrators is the ability to inspect these internal graphs. DataFusion provides the EXPLAIN command, which can output the plan in several formats.13

* **Indent/Tree Format**: A text-based hierarchical view of the operators.13  
* **Graphviz (DOT) Format**: A specialized format that can be rendered into a visual diagram using Graphviz software. This is particularly useful for complex queries with dozens of joins and subqueries, as it allows users to see the overall topology of the dataflow.9  
* **Verbose Mode**: Includes internal details about optimization rules applied, such as type coercion and expression simplification.12

The Python API mirrors these capabilities, offering methods like display\_graphviz() on LogicalPlan objects, which facilitates the integration of plan visualization into Jupyter notebooks or web-based monitoring tools.9

## **Graph-Based Optimization Frameworks**

Optimization in DataFusion is the process of rewriting the query DAG to be more efficient while preserving the original semantics. This is achieved through a modular system of OptimizerRule and PhysicalOptimizerRule implementations.12

### **Rule-Based and Cost-Based Rewriting**

The optimizer applies a sequence of rules to the DAG. Some of these are "always-beneficial" rules, such as Projection Pushdown (removing unused columns as early as possible) and Filter Pushdown (moving predicates closer to the data source to reduce the volume of rows processed).1 Other rules are engine-specific, such as constant folding, where an expression like 1 \+ 2 is replaced by 3 before the physical plan is even generated.2

DataFusion also incorporates Cost-Based Optimization (CBO). By analyzing statistics such as row counts, null counts, and min/max values for columns, the optimizer can make informed decisions about join orders and algorithm selection.2 The AnalysisContext API tracks these statistics as they flow through the expression tree, using interval arithmetic to estimate the selectivity of filters.12

### **Advanced Techniques: E-Graphs and Equality Saturation**

A significant advancement in the DataFusion ecosystem is the datafusion-tokomak crate, which introduces e-graph-based optimization.15 In a traditional optimizer, rules are applied in a fixed order, which can sometimes miss the optimal configuration if one rule prevents another from firing. E-graphs solve this by implementing "equality saturation." This technique stores many equivalent versions of the query graph simultaneously in a compact data structure.15

The optimizer can then explore the entire space of equivalent plans and select the one with the lowest estimated cost. This approach is particularly powerful for complex expression simplification and join reordering, as it avoids the "local optima" problem inherent in sequential rule application.15

| Optimization Technique | Description | Primary Benefit |
| :---- | :---- | :---- |
| Predicate Pushdown | Moves filters below joins/projections 1 | Reduces data volume early |
| Projection Pushdown | Eliminates unused columns 1 | Saves memory/IO bandwidth |
| Ordering Analysis | Tracks and utilizes input data order 12 | Eliminates redundant Sorts |
| Equality Saturation | Uses e-graphs to explore all equivalent plans 15 | Finds global optimal plan |

## **Native Graph Analytics in the Arrow Ecosystem**

Beyond using graphs for internal query representation, there is an increasing demand for performing native graph analytics—such as PageRank, shortest path finding, and community detection—directly on data stored in the Arrow format.7

### **The arrow-graph Crate**

The arrow-graph crate provides a high-performance, Arrow-native graph analytics engine. It represents graphs as RecordBatches, where edges are stored in a columnar format (e.g., source\_id, destination\_id, weight).7 This design allows for zero-copy graph operations and leverages SIMD-optimized implementations of graph algorithms.7

arrow-graph integrates seamlessly with DataFusion by exposing its algorithms as User-Defined Functions (UDFs). This allows users to mix relational SQL with graph-based analysis. For example, a user could select all transactions for a specific customer and then run a community detection algorithm to identify if that customer belongs to a known fraud ring.7

### **Performance and Scalability**

Initial benchmarks for arrow-graph indicate significant performance advantages over traditional Python-based graph libraries like NetworkX, with reported speedups of 10x to 100x on datasets with millions of edges.7 This performance is achieved through the use of Arrow’s compute kernels and efficient adjacency list indexing that remains memory-resident.7

The roadmap for arrow-graph includes support for streaming graph processing and cloud-native distributed processing, aligning with DataFusion’s broader goals of scalability.7 By using Arrow as the common memory format, arrow-graph can also serve as a pre-processing engine for Graph Neural Networks (GNNs), preparing data for frameworks like PyTorch Geometric or DGL with minimal overhead.7

## **Implementing Graph Algorithms on Relational Engines**

One of the most profound insights from the DataFusion community is the realization that complex graph algorithms can be reformulated into relational operations. This allows DataFusion to function as a graph processing engine without needing specialized graph-native operators for every use case.18

### **Weakly Connected Components (WCC) via DataFrames**

A prime example is the implementation of the Weakly Connected Components (WCC) algorithm, which is essential for identity resolution in data warehouses.18 In identity resolution, different identifiers (e.g., email, cookies, device IDs) are treated as nodes, and known associations between them are treated as edges. Finding WCCs allows the system to group all identifiers belonging to the same physical entity into a "Golden ID".18

The implementation uses a MapReduce-style paradigm within the DataFusion DataFrame API:

1. **Map Phase**: Represented by SELECT and JOIN operations that propagate labels (component IDs) across edges.18  
2. **Reduce Phase**: Represented by GROUP BY and min() aggregations that update each node’s label to the smallest label in its neighborhood.18  
3. **Iteration**: The process repeats until no further labels change.

This "big-star small-star" algorithm is particularly well-suited for DataFusion because it avoids global shared state and handles the data skew typical of real-world scale-free networks.18 However, a critical technical detail is the necessity of calling .cache() on intermediate DataFrames. Because DataFusion is lazy, an iterative algorithm would otherwise result in an exponentially growing query plan that would eventually exhaust memory or crash the optimizer.18

### **Comparative Performance**

When compared to Apache Spark, DataFusion-based implementations of graph algorithms have shown remarkable efficiency. For single-node workloads, DataFusion can be 4-5 times faster than Spark, with significantly lower memory overhead.18 This is attributed to the low-level optimizations provided by Rust and the cache-friendly nature of the Arrow columnar format.4

| Metric | Apache Spark | Apache DataFusion |
| :---- | :---- | :---- |
| Language/Runtime | Scala/JVM | Rust/Native |
| Execution Model | Row-oriented (default) | Vectorized Columnar 4 |
| WCC Performance | Baseline | 4-5x Faster 18 |
| Memory Management | GC-based | Manual/Resource Pool 4 |
| Scalability | Distributed-first | Single-node (Ballista for Dist) 3 |

## **Multi-Modal Data Fusion Techniques**

In the era of AI and big data, information is rarely confined to a single format. Multi-modal data fusion is the process of integrating disparate data types—such as text, images, audio, and IoT sensor streams—into a unified view.6 Graph-based fusion has emerged as a dominant technique because of its ability to capture both the internal structure of a modality and the complex relationships between different modalities.6

### **Structural Integration via Graphs**

Graph-based fusion models entities or observations as nodes and their relationships as edges. For instance, in social media analysis, a "post" node could be connected to an "image" node and a "text" node, as well as the "user" node who created it.6 This creates a heterogeneous network where cross-modal information can be propagated via message passing.6

The mathematical formulation of this process often involves Graph Neural Networks (GNNs). A node's representation ![][image1] is updated by aggregating features from its neighbors:

![][image2]  
where ![][image3] represents an attention score between node ![][image4] and node ![][image5], and ![][image6] is a learnable weight matrix.21 In multi-modal settings, these attention scores can be designed to prioritize information from more reliable modalities, such as using sensor data to verify a visual observation in a smart manufacturing environment.19

### **Fusion Strategies: Early, Late, and Intermediate**

The timing of integration defines the fusion strategy:

1. **Early Fusion (Feature-Level)**: Raw features from all modalities are concatenated or merged before being passed into a single model.19 This allows the model to learn low-level cross-modal interactions but is sensitive to differences in data resolution and sampling rates.19  
2. **Late Fusion (Decision-Level)**: Each modality is processed by a specialized model, and their final predictions are aggregated (e.g., via weighted voting or an ensemble model).19 This is more robust to noise but misses deep semantic correlations between modalities.23  
3. **Intermediate Fusion**: Latent representations (embeddings) are extracted for each modality and fused in a shared embedding space using techniques like cross-modal attention.6 This is widely used in state-of-the-art multi-modal large language models (MLLMs) to align visual and textual tokens.24

| Strategy | Integration Layer | Best For | Trade-offs |
| :---- | :---- | :---- | :---- |
| Early | Input | High-correlation data 19 | High computational complexity |
| Intermediate | Latent | Complex reasoning (e.g., AVs) 23 | Requires sophisticated alignment |
| Late | Output | Decoupled sensors/tasks 23 | Loses contextual richness |

### **Application: Real-Time Multi-Modal Analytics**

DataFusion’s streaming, multi-threaded execution engine is ideal for building multi-modal fusion pipelines. Tools like Apache Kafka or AWS Kinesis ingest high-throughput streams of sensor telemetry, video feeds, and text logs.19 DataFusion can then be used to synchronize and align these streams based on timestamps before applying fusion algorithms. In healthcare, this enables "contextual intelligence" by fusing real-time biometric sensor data with historical medical records and clinical notes to predict critical events like sepsis hours earlier than traditional monitoring.19

## **Interoperability and the Global Query Graph**

The modularity of DataFusion is not limited to its internal components; it extends to the entire big data ecosystem. The ability to share query graphs and data across different languages and hardware is a cornerstone of its design.1

### **Substrait: The Universal Language for Query Plans**

Substrait is an open-source project that provides a standardized, cross-language serialization format for relational query plans.14 By supporting Substrait, DataFusion can act as a "compute backend" for other systems. For example, a query plan generated in Apache Calcite (Java) can be serialized into a Substrait protobuf message and then executed by DataFusion (Rust).15

This decoupling of the API from the compute engine is analogous to how LLVM provides a common backend for diverse programming languages.1 It allows developers to build domain-specific query languages—such as those for life sciences (Exon) or geospatial analysis (SedonaDB)—while relying on DataFusion for optimized execution.10

### **Hardware Acceleration and Operator Fusion**

The graph-based nature of physical plans in DataFusion facilitates the offloading of specific operators to specialized hardware. Projects like Maximus explore hybrid execution models where portions of the query graph are executed on CPUs while others are offloaded to GPUs using the Arrow Acero engine.27

A key benefit of this approach is "operator fusion." Instead of materializing intermediate results between every step, the engine can "fuse" multiple logical operations (like a filter and a projection) into a single physical task that processes a batch of data in a single pass over memory.28 This reduces memory bandwidth pressure and maximizes the efficiency of the CPU’s instruction pipeline.29

## **The Industry Ecosystem: From Databases to Stream Processing**

The adoption of DataFusion across the industry highlights its versatility as both a standalone engine and a foundational library.

### **Zero-ETL Graph Engines: PuppyGraph**

PuppyGraph represents a new breed of data platform: a real-time, zero-ETL graph query engine. It allows data teams to query existing relational stores—such as MySQL, Snowflake, or S3-backed Iceberg tables—as a unified graph model.30 PuppyGraph bypasses the need for specialized graph databases and the associated ETL costs by pointing its engine directly at the source tables.30

Architecturally, PuppyGraph leverages massively parallel processing and vectorized evaluation similar to DataFusion to execute complex multi-hop queries.32 Its integration with Apache Polaris (a metadata catalog for Iceberg) ensures fine-grained governance and security while enabling 10-hop neighbor queries across half a billion edges in under 3 seconds.32

### **Specialized Analytical Systems**

Many modern databases are "built on" DataFusion rather than being forks of it. This allows them to focus on domain-specific features like time-series ingestion or vector search while benefiting from DataFusion’s high-quality query planner.5

* **InfluxDB 3.0**: A time-series database that uses the "FDAP" architecture (Flight, DataFusion, Arrow, and Parquet) to provide high-performance SQL querying over time-stamped metrics.33  
* **LanceDB**: An open-source vector database for AI that uses DataFusion to support SQL filters over multi-modal data embeddings.28  
* **Comet (Apple)**: An accelerator for Apache Spark that replaces Spark’s native execution with a DataFusion-powered runtime, achieving significant speedups for vector searches and large-scale ML data analysis.3  
* **Arroyo**: A distributed stream processing engine that replaced its original SQL engine with a DataFusion-based implementation, resulting in 3x higher throughput and 20x faster startup times.33

### **Distributed Scaling: Apache Ballista**

While core DataFusion is a single-node engine, the Ballista project extends its capabilities to distributed environments.2 Ballista acts as a scheduler and coordinator, breaking a query graph into "fragments" that can be distributed across a cluster of DataFusion nodes.3 This architecture competes with systems like Apache Spark but offers the memory safety and performance advantages of Rust and the zero-copy efficiency of the Arrow format.3

## **Technical Nuances: Memory Format and Streaming Mechanics**

The efficacy of graph processing in DataFusion is inextricably linked to the underlying Apache Arrow memory format and the engine's streaming mechanics.11

### **The Columnar Advantage**

Apache Arrow standardizes the in-memory representation of columnar data. Each column is stored as a contiguous array, which is highly efficient for vectorized operations. For graph data, this means that following an edge from source to destination involves simple array indexing rather than following expensive pointer chains typical of row-oriented graph databases.7

| Concept | Relational View | Graph View | Arrow Implementation |
| :---- | :---- | :---- | :---- |
| Table Slice | RecordBatch | Subgraph | Contiguous arrays 11 |
| Column | Attribute | Edge List | ArrayRef (Ref-counted) 11 |
| Row | Tuple | Vertex/Edge | Array index 11 |
| Missing Data | Null | No Edge | Validity Bitmap 11 |

### **Pull-Based Streaming and Backpressure**

DataFusion processes queries as pull-based pipelines. Each operator requests a batch of data from its children only when it has the capacity to process it. This streaming approach ensures that memory usage remains bounded, as the engine does not need to materialize the entire dataset at once.11 This is particularly important for graph algorithms, where "intermediate results" (like the set of visited nodes in a BFS) can potentially be much larger than the original graph.11

To prevent memory exhaustion during complex joins or aggregations, DataFusion supports spilling to disk. The RuntimeEnv and MemoryPool components monitor the engine's memory footprint and trigger spilling when thresholds are exceeded.35

## **Future Outlook: The Convergence of Graph and Relational Paradigms**

The distinction between "graph databases" and "relational databases" is blurring. As engines like DataFusion become more extensible and high-performance, the cost of switching between these paradigms is decreasing.

### **Emerging Trends**

* **Streaming Graph Analytics**: The combination of datafusion-streams and arrow-graph will enable real-time monitoring of complex networks, such as detecting cascading failures in a power grid or viral misinformation on social networks as it happens.15  
* **AI-Integrated Query Planning**: Future optimizers may use machine learning to predict the cost of complex graph traversals more accurately, leading to better plan selection in highly skewed datasets.24  
* **Global Data Federation**: Through Substrait and Arrow Flight, query graphs will increasingly span multiple clouds and data centers, allowing for federated graph analysis across diverse organizational boundaries without moving data.15

### **Conclusion**

Apache DataFusion stands at the vanguard of the modern analytical ecosystem, demonstrating that a well-architected, graph-based relational engine can tackle the most demanding challenges in data fusion and network analysis. Its commitment to the Apache Arrow standard ensures that it remains at the center of a growing web of interoperable tools, from high-performance databases to real-time AI pipelines. By abstracting the complexities of query optimization and parallel execution into a modular, extensible framework, DataFusion allows developers to focus on what truly matters: deriving deep, contextual insights from the vast and interconnected sea of modern data. Whether through the direct implementation of graph algorithms on DataFrames, the use of specialized graph crates, or the integration of multi-modal fusion pipelines, the graph-based nature of DataFusion continues to push the boundaries of what is possible in high-performance computing.

#### **Works cited**

1. Insights from paper: Apache Arrow DataFusion: a Fast, Embeddable, Modular Analytic Query Engine \- Hemant Gupta, accessed April 22, 2026, [https://hemantkgupta.medium.com/insights-from-paper-apache-arrow-datafusion-a-fast-embeddable-modular-analytic-query-engine-987ce6cf3b7d](https://hemantkgupta.medium.com/insights-from-paper-apache-arrow-datafusion-a-fast-embeddable-modular-analytic-query-engine-987ce6cf3b7d)  
2. Apache Data Fusion: Building Next-Generation Analytics from the Ground Up \- Medium, accessed April 22, 2026, [https://medium.com/data-reply-it-datatech/apache-data-fusion-building-next-generation-analytics-from-the-ground-up-560032a151d4](https://medium.com/data-reply-it-datatech/apache-data-fusion-building-next-generation-analytics-from-the-ground-up-560032a151d4)  
3. Apache DataFusion — Apache DataFusion documentation \- Apache Software Foundation, accessed April 22, 2026, [https://datafusion.apache.org/](https://datafusion.apache.org/)  
4. Apache DataFusion — Modern query engine for performance | by Amit Singh Rathore, accessed April 22, 2026, [https://blog.devgenius.io/apache-datafusion-modern-query-engine-for-performance-787c47679ee1](https://blog.devgenius.io/apache-datafusion-modern-query-engine-for-performance-787c47679ee1)  
5. datafusion \- Rust \- Docs.rs, accessed April 22, 2026, [https://docs.rs/datafusion/latest/datafusion/](https://docs.rs/datafusion/latest/datafusion/)  
6. Graph-Based Fusion Techniques \- Emergent Mind, accessed April 22, 2026, [https://www.emergentmind.com/topics/graph-based-fusion](https://www.emergentmind.com/topics/graph-based-fusion)  
7. arrow-graph \- crates.io: Rust Package Registry, accessed April 22, 2026, [https://crates.io/crates/arrow-graph/0.2.0](https://crates.io/crates/arrow-graph/0.2.0)  
8. Building Logical Plans — Apache DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/library-user-guide/building-logical-plans.html](https://datafusion.apache.org/library-user-guide/building-logical-plans.html)  
9. datafusion.plan — Apache Arrow DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/python/autoapi/datafusion/plan/index.html](https://datafusion.apache.org/python/autoapi/datafusion/plan/index.html)  
10. Introduction — Apache DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/user-guide/introduction.html](https://datafusion.apache.org/user-guide/introduction.html)  
11. Gentle Arrow Introduction — Apache DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/user-guide/arrow-introduction.html](https://datafusion.apache.org/user-guide/arrow-introduction.html)  
12. Query Optimizer — Apache DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/library-user-guide/query-optimizer.html](https://datafusion.apache.org/library-user-guide/query-optimizer.html)  
13. datafusion.dataframe — Apache Arrow DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/python/autoapi/datafusion/dataframe/index.html](https://datafusion.apache.org/python/autoapi/datafusion/dataframe/index.html)  
14. Powering Semantic SQL for AI Agents with Apache DataFusion \- Wren AI, accessed April 22, 2026, [https://www.getwren.ai/post/powering-semantic-sql-for-ai-agents-with-apache-datafusion](https://www.getwren.ai/post/powering-semantic-sql-for-ai-agents-with-apache-datafusion)  
15. Introducing Apache Arrow DataFusion Contrib, accessed April 22, 2026, [https://datafusion.apache.org/blog/2022/03/21/datafusion-contrib/](https://datafusion.apache.org/blog/2022/03/21/datafusion-contrib/)  
16. EXPLAIN — Apache DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/user-guide/sql/explain.html](https://datafusion.apache.org/user-guide/sql/explain.html)  
17. Top 10 Graph Database Use Cases (With Real-World Case Studies) \- Neo4j, accessed April 22, 2026, [https://neo4j.com/blog/graph-database/graph-database-use-cases/](https://neo4j.com/blog/graph-database/graph-database-use-cases/)  
18. Graphs, Algorithms, and My First Impression of DataFusion | Sem ..., accessed April 22, 2026, [https://semyonsinchenko.github.io/ssinchenko/post/datafusion-graphs-cc/](https://semyonsinchenko.github.io/ssinchenko/post/datafusion-graphs-cc/)  
19. Multi-Modal Data Fusion: Integrating Text, Image, Audio, and Sensor Data in Real-Time Analytics Pipelines, accessed April 22, 2026, [https://datahubanalytics.com/multi-modal-data-fusion-integrating-text-image-audio-and-sensor-data-in-real-time-analytics-pipelines/](https://datahubanalytics.com/multi-modal-data-fusion-integrating-text-image-audio-and-sensor-data-in-real-time-analytics-pipelines/)  
20. M3DUSA: A Modular Multi-Modal Deep fUSion Architecture for fake news detection on social media \- CNR-IRIS, accessed April 22, 2026, [https://iris.cnr.it/bitstream/20.500.14243/559903/1/M3DUSA.pdf](https://iris.cnr.it/bitstream/20.500.14243/559903/1/M3DUSA.pdf)  
21. Multimodal data multidimensional time series fusion method based on graph neural network \- SPIE Digital Library, accessed April 22, 2026, [https://www.spiedigitallibrary.org/conference-proceedings-of-spie/14061/140611A/Multimodal-data-multidimensional-time-series-fusion-method-based-on-graph/10.1117/12.3107820.full](https://www.spiedigitallibrary.org/conference-proceedings-of-spie/14061/140611A/Multimodal-data-multidimensional-time-series-fusion-method-based-on-graph/10.1117/12.3107820.full)  
22. Boosting Document Layout Analysis with Graphic Multi-modal Data Fusion and Spatial Geometric Transformation | OpenReview, accessed April 22, 2026, [https://openreview.net/forum?id=kmbU3EdLtS](https://openreview.net/forum?id=kmbU3EdLtS)  
23. Multimodal Models and Fusion \- A Complete Guide \- Medium, accessed April 22, 2026, [https://medium.com/@raj.pulapakura/multimodal-models-and-fusion-a-complete-guide-225ca91f6861](https://medium.com/@raj.pulapakura/multimodal-models-and-fusion-a-complete-guide-225ca91f6861)  
24. Multimodal Information Fusion for Chart Understanding: A Survey of MLLMs—Evolution, Limitations, and Cognitive Enhancement \- arXiv, accessed April 22, 2026, [https://arxiv.org/html/2602.10138v1](https://arxiv.org/html/2602.10138v1)  
25. ​​One query plan, three different engines. | by Omri Eliyahu Levy | Medium, accessed April 22, 2026, [https://medium.com/@omri-levy/one-query-plan-three-different-engines-e5dc74aeb52f](https://medium.com/@omri-levy/one-query-plan-three-different-engines-e5dc74aeb52f)  
26. Data Fusion \- Simon Späti, accessed April 22, 2026, [https://www.ssp.sh/brain/datafusion/](https://www.ssp.sh/brain/datafusion/)  
27. Maximus: A Modular Accelerated Query Engine for Data Analytics on Heterogeneous Systems \- ETH Zurich Research Collection, accessed April 22, 2026, [https://www.research-collection.ethz.ch/server/api/core/bitstreams/4c8447f2-afc3-4da7-b8d3-9ecb5f5eeaa7/content](https://www.research-collection.ethz.ch/server/api/core/bitstreams/4c8447f2-afc3-4da7-b8d3-9ecb5f5eeaa7/content)  
28. Columnar File Readers in Depth: APIs and Fusion \- LanceDB, accessed April 22, 2026, [https://www.lancedb.com/blog/columnar-file-readers-in-depth-apis-and-fusion](https://www.lancedb.com/blog/columnar-file-readers-in-depth-apis-and-fusion)  
29. Understanding the Performance of Native Execution in Big Data Engines: The Good, the Bad, and How to Fix It \- OpenProceedings.org, accessed April 22, 2026, [https://openproceedings.org/2026/conf/edbt/paper-163.pdf](https://openproceedings.org/2026/conf/edbt/paper-163.pdf)  
30. PuppyGraph | Query Your Relational Data As A Graph. No ETL., accessed April 22, 2026, [https://www.puppygraph.com/](https://www.puppygraph.com/)  
31. Connecting \- PuppyGraph Docs, accessed April 22, 2026, [https://docs.puppygraph.com/connecting/](https://docs.puppygraph.com/connecting/)  
32. Integrating Apache Polaris with PuppyGraph for Real-time Graph Analysis, accessed April 22, 2026, [https://polaris.apache.org/blog/2025/10/02/integrating-apache-polaris-with-puppygraph-for-real-time-graph-analysis/](https://polaris.apache.org/blog/2025/10/02/integrating-apache-polaris-with-puppygraph-for-real-time-graph-analysis/)  
33. 7 Projects Building on DataFusion | InfluxData, accessed April 22, 2026, [https://www.influxdata.com/blog/7-datafusion-projects-influxdb/](https://www.influxdata.com/blog/7-datafusion-projects-influxdb/)  
34. Apache Arrow DataFusion \- A Primer \- Work-Bench, accessed April 22, 2026, [https://www.work-bench.com/post/apache-arrow-datafusion-a-primer](https://www.work-bench.com/post/apache-arrow-datafusion-a-primer)  
35. Apache DataFusion SQL Query Engine \- GitHub, accessed April 22, 2026, [https://github.com/apache/datafusion](https://github.com/apache/datafusion)  
36. Apache Arrow DataFusion documentation, accessed April 22, 2026, [https://datafusion.apache.org/python/autoapi/datafusion/index.html](https://datafusion.apache.org/python/autoapi/datafusion/index.html)  
37. Apache Arrow projection \- Neo4j Graph Data Science, accessed April 22, 2026, [https://neo4j.com/docs/graph-data-science/current/management-ops/graph-creation/graph-project-apache-arrow/](https://neo4j.com/docs/graph-data-science/current/management-ops/graph-creation/graph-project-apache-arrow/)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABEAAAAYCAYAAAAcYhYyAAAAyUlEQVR4XmNgGAWjgHQwEYhTkfgdQFyDxMcLxIH4EpSdC8S/gPg/lH8WiHugbLwApgEEeKB8fSC2gLIjkORxAiMkdhkDqqEcSGwYOAnEfOiCyOATA6oh2EArugA6ABmwGF2QEBBggGhUZkCEhxaS/FUktjwQ7wPiuUhiYDCTAaKRE4jPQdmKUDlQ4K6AskHgNhALAfFfJDEwYGSAaARhVwaIi2D8OiR1MAAKVKJiCx8gFOgEgQMDxEu8DJCwJBv8AOLl6IKjABUAAOZLJemxiJROAAAAAElFTkSuQmCC>

[image2]: <data:image/png;base64,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>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABgAAAAYCAYAAADgdz34AAABDElEQVR4XmNgGAWjYBRgA65AvAGI89AlKAUmQPwfiB2g/CooHwamIbFJBroMEMOE0MRBYquh7L/IEqQCkEEv0AWB4B8DRM4ciKPR5LKAuARNDATWArE+soADA8QQd2RBKHjEAJFDDioYCANiTnRBIGhGFwAFATYDQOAaA0ROEl2CFNDAgNuCiwzY5Z4C8Rc0sQAgXgLE59HEwQBkiCqa2D0GSHjCLOiD0uegNLLFLEDsjUUcDkCpB5RKYOE9A0nuPlQsHknME4ivI/FhQA+IP6ELkgN+ALEMuiAQrAPicnRBcgAsGOpQRCHioOCiGLwG4gfoggw4wp9awJgBYjHVAcjVoEinqesN0QXoBgAZ6zlY7HclfgAAAABJRU5ErkJggg==>

[image4]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAcAAAAXCAYAAADHhFVIAAAAaElEQVR4XmNgGHigAMT30QVh4C0Q/0cXpAx0AnECuiAI/IDSIPsckSVmAjETlA2SdEWSY6iF0v0MeFwKkihEFwSBPAaELmEgNkGSA0u8g7IfI0uAwDMgPsQAsT8TTQ4MAoBYDF1w6AAA4oAS3/pLqloAAAAASUVORK5CYII=>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAkAAAAYCAYAAAAoG9cuAAAAg0lEQVR4XmNgGDqADYjvowuig5VA/B9dkPqAGYgXAXEAugQMSAPxIygb5J52JDk4QHYoiH0ViQ8HqkhskKI4JD4GcGYgwvtHGIhQBFIA8wBOAFIUjy6IDNwYiLDqJAMORX+A2BfKBilYgJBCAJCEARA/BOLnaHJwEAnEC4FYHF1igAAAaWoanLUd/TEAAAAASUVORK5CYII=>

[image6]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAYCAYAAAAVibZIAAAA+klEQVR4Xu2SMetBYRTGT7EaWJAPwIcgi8liN6AsJotPoP7FqnwEBhl9BqMyyGCRMlooEuF/jvfcOh3v9Vpkub964n1+PbfrXgAB3yCE2WMeIjt2GcxRuRU7YqtcWbgnJxY2vJGNBqalS485+A/fXfSqC8kYzDCp+grmzk7Tx8R0KemAGeZUf8ZM2YWVW6rzC3Uww5rohpgIZsAuLdxCfPclD2b4J7oZf7bZFfkcBfPTnaTADEd83ghXZdfk80U4JzSku0tguqLPsuthCpiScE5oeMDcVE//CHITi3NCQwrdjcZzcS1c0MjveZFb6/ITaEhv1ga5gIBf8A/vQUX4wN6bNQAAAABJRU5ErkJggg==>