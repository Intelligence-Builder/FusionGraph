INSTALL duckpgq FROM community;
LOAD duckpgq;
.timer on
-- Graph load: Parquet -> edge/vertex tables -> property graph.
CREATE TABLE edges AS SELECT source, target FROM read_parquet('__PARQUET__');
CREATE TABLE nodes AS SELECT DISTINCT id FROM (SELECT source AS id FROM edges UNION SELECT target FROM edges);
CREATE PROPERTY GRAPH g
  VERTEX TABLES (nodes)
  EDGE TABLES (edges SOURCE KEY (source) REFERENCES nodes (id) DESTINATION KEY (target) REFERENCES nodes (id));
.print '-- parity check: SQL/PGQ vs chained joins (must match) --'
SELECT COUNT(DISTINCT b_id) AS pgq_3hop
FROM GRAPH_TABLE (g MATCH (a:nodes)-[e:edges]->{1,3}(b:nodes) WHERE a.id = 0 COLUMNS (b.id AS b_id));
SELECT COUNT(DISTINCT n) AS joins_3hop FROM (
  SELECT target AS n FROM edges WHERE source = 0
  UNION
  SELECT e2.target FROM edges e1 JOIN edges e2 ON e1.target = e2.source WHERE e1.source = 0
  UNION
  SELECT e3.target FROM edges e1
    JOIN edges e2 ON e1.target = e2.source
    JOIN edges e3 ON e2.target = e3.source WHERE e1.source = 0
);
.print '-- timed runs (second run of each is the warm number) --'
SELECT COUNT(DISTINCT b_id) AS pgq_2hop
FROM GRAPH_TABLE (g MATCH (a:nodes)-[e:edges]->{1,2}(b:nodes) WHERE a.id = 0 COLUMNS (b.id AS b_id));
SELECT COUNT(DISTINCT b_id) AS pgq_2hop_warm
FROM GRAPH_TABLE (g MATCH (a:nodes)-[e:edges]->{1,2}(b:nodes) WHERE a.id = 0 COLUMNS (b.id AS b_id));
SELECT COUNT(DISTINCT b_id) AS pgq_3hop
FROM GRAPH_TABLE (g MATCH (a:nodes)-[e:edges]->{1,3}(b:nodes) WHERE a.id = 0 COLUMNS (b.id AS b_id));
SELECT COUNT(DISTINCT b_id) AS pgq_3hop_warm
FROM GRAPH_TABLE (g MATCH (a:nodes)-[e:edges]->{1,3}(b:nodes) WHERE a.id = 0 COLUMNS (b.id AS b_id));
