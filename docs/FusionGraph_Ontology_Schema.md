# FusionGraph Ontology Schema Specification

**Version:** 1.0  
**Status:** Draft

## 1. Overview

The **Ontology Schema** defines how relational tables in a Data Lakehouse (Iceberg/Parquet) are projected into a graph topology. This schema is the contract between the user's data model and FusionGraph's `GraphTableProvider`.

## 2. Schema Format

FusionGraph accepts ontology definitions in **TOML** (primary) or **JSON** (for programmatic generation).

### 2.1 File Location

```
project/
├── fusiongraph.toml          # Root ontology (default)
├── ontologies/
│   ├── iam_graph.toml        # Named ontology
│   └── network_graph.toml
```

### 2.2 Root Structure

```toml
[ontology]
name = "iam_security_graph"
version = "1.0"
description = "IAM relationships for blast radius analysis"

# Global settings
[settings]
default_node_id_type = "u64"          # u32, u64, uuid, string
edge_direction = "directed"            # directed, undirected
allow_self_loops = false
allow_parallel_edges = true

# Node definitions
[[nodes]]
# ... (see Section 3)

# Edge definitions  
[[edges]]
# ... (see Section 4)

# Computed properties (optional)
[[properties]]
# ... (see Section 5)
```

## 3. Node Definitions

Nodes represent entities in the graph. Each node type maps to a table or view.

### 3.1 Basic Node

```toml
[[nodes]]
label = "User"                         # Graph label (required)
source = "iceberg.iam.users"           # Fully-qualified table (required)
id_column = "user_id"                  # Primary key column (required)

# Optional: columns to include as node properties
properties = ["email", "created_at", "department"]

# Optional: filter predicate (pushdown to storage)
filter = "status = 'active'"
```

### 3.2 Node with Composite Key

```toml
[[nodes]]
label = "Resource"
source = "iceberg.aws.resources"
id_column = ["account_id", "resource_arn"]   # Composite key
separator = "::"                              # Composite ID separator; yields "123456::arn:aws:s3:::bucket"
properties = ["resource_type", "region"]
```

### 3.3 Node with ID Transformation

```toml
[[nodes]]
label = "Account"
source = "iceberg.aws.accounts"
id_column = "account_id"
id_transform = "hash_u64"              # hash_u64, hash_u32, passthrough, uuid_to_u128
properties = ["account_name", "org_unit"]
```

### 3.4 Supported ID Transforms

| Transform | Input Type | Output Type | Notes |
|-----------|-----------|-------------|-------|
| `passthrough` | u32/u64 | Same | Default for numeric IDs |
| `hash_u64` | String | u64 | FNV-1a hash |
| `hash_u32` | String | u32 | FNV-1a hash (collision risk on >1M nodes) |
| `uuid_to_u128` | UUID | u128 | Preserves uniqueness |
| `extract_numeric` | String | u64 | Regex: `(\d+)` from string |

## 4. Edge Definitions

Edges represent relationships between nodes.

### 4.1 Basic Edge (Foreign Key Pattern)

```toml
[[edges]]
label = "BELONGS_TO"
source = "iceberg.iam.user_groups"     # Edge table

# Source and target node references
from_node = "User"                      # Must match a [[nodes]].label
from_column = "user_id"                 # Column in edge table

to_node = "Group"
to_column = "group_id"

# Optional edge properties
properties = ["joined_at", "role"]
```

### 4.2 Self-Referential Edge

```toml
[[edges]]
label = "REPORTS_TO"
source = "iceberg.hr.employees"

from_node = "Employee"
from_column = "employee_id"

to_node = "Employee"                    # Same node type
to_column = "manager_id"

# Handle nulls (employees without managers)
skip_null_targets = true
```

### 4.3 Implicit Edge (Same Table)

When edges are embedded in the node table:

```toml
[[edges]]
label = "PARENT_OF"
source = "iceberg.org.accounts"         # Same as Account node source

from_node = "Account"
from_column = "account_id"

to_node = "Account"
to_column = "parent_account_id"

implicit = true                          # Edge derived from node table
```

### 4.4 Weighted Edge

```toml
[[edges]]
label = "TRUSTS"
source = "iceberg.iam.trust_policies"

from_node = "Role"
from_column = "role_arn"

to_node = "Principal"
to_column = "principal_arn"

weight_column = "trust_score"            # For shortest-path algorithms
weight_default = 1.0                     # Default if column is NULL
```

### 4.5 Temporal Edge

```toml
[[edges]]
label = "ACCESSED"
source = "iceberg.logs.access_events"

from_node = "User"
from_column = "user_id"

to_node = "Resource"
to_column = "resource_arn"

# Temporal bounds (for time-windowed traversals)
valid_from_column = "event_time"
valid_to_column = null                   # null = edge is instantaneous event

# Partition pruning hint
partition_column = "event_date"
```

## 5. Computed Properties

Define derived properties computed during CSR build or traversal.

```toml
[[properties]]
name = "is_admin"
node = "User"
expression = "ARRAY_CONTAINS(roles, 'admin')"   # SQL expression
materialized = true                              # Compute at build time

[[properties]]
name = "edge_age_days"
edge = "ACCESSED"
expression = "DATEDIFF('day', valid_from, CURRENT_DATE())"
materialized = false                             # Compute at query time
```

## 6. Validation Rules

FusionGraph validates ontology schemas at load time:

| Rule | Error Code | Description |
|------|------------|-------------|
| `E001` | `DUPLICATE_LABEL` | Node or edge label already defined |
| `E002` | `MISSING_SOURCE` | Referenced table does not exist in catalog |
| `E003` | `MISSING_COLUMN` | Referenced column not in table schema |
| `E004` | `TYPE_MISMATCH` | ID column type incompatible with `id_transform` |
| `E005` | `DANGLING_EDGE` | Edge references undefined node label |
| `E006` | `CYCLE_IN_IMPLICIT` | Implicit edges create infinite recursion |
| `W001` | `HIGH_CARDINALITY` | String ID without hash transform (>1M rows) |
| `W002` | `NO_PARTITION_HINT` | Temporal edge without partition column |

## 7. JSON Equivalent

For programmatic generation:

```json
{
  "ontology": {
    "name": "iam_security_graph",
    "version": "1.0"
  },
  "settings": {
    "default_node_id_type": "u64",
    "edge_direction": "directed"
  },
  "nodes": [
    {
      "label": "User",
      "source": "iceberg.iam.users",
      "id_column": "user_id",
      "properties": ["email", "department"]
    }
  ],
  "edges": [
    {
      "label": "BELONGS_TO",
      "source": "iceberg.iam.user_groups",
      "from_node": "User",
      "from_column": "user_id",
      "to_node": "Group",
      "to_column": "group_id"
    }
  ]
}
```

## 8. Example: Complete IAM Ontology

```toml
[ontology]
name = "aws_iam_graph"
version = "1.0"
description = "AWS IAM blast radius analysis"

[settings]
default_node_id_type = "u64"
edge_direction = "directed"

# === NODES ===

[[nodes]]
label = "Account"
source = "iceberg.aws.accounts"
id_column = "account_id"
properties = ["account_name", "org_unit", "environment"]

[[nodes]]
label = "User"
source = "iceberg.iam.users"
id_column = "user_arn"
id_transform = "hash_u64"
properties = ["user_name", "created_at", "last_login"]
filter = "status = 'active'"

[[nodes]]
label = "Role"
source = "iceberg.iam.roles"
id_column = "role_arn"
id_transform = "hash_u64"
properties = ["role_name", "trust_policy_hash"]

[[nodes]]
label = "Policy"
source = "iceberg.iam.policies"
id_column = "policy_arn"
id_transform = "hash_u64"
properties = ["policy_name", "policy_document_hash"]

[[nodes]]
label = "Resource"
source = "iceberg.aws.resources"
id_column = "resource_arn"
id_transform = "hash_u64"
properties = ["resource_type", "region", "account_id"]

# === EDGES ===

[[edges]]
label = "IN_ACCOUNT"
source = "iceberg.iam.users"
from_node = "User"
from_column = "user_arn"
to_node = "Account"
to_column = "account_id"
implicit = true

[[edges]]
label = "HAS_POLICY"
source = "iceberg.iam.user_policies"
from_node = "User"
from_column = "user_arn"
to_node = "Policy"
to_column = "policy_arn"

[[edges]]
label = "CAN_ASSUME"
source = "iceberg.iam.assumable_roles"
from_node = "User"
from_column = "user_arn"
to_node = "Role"
to_column = "role_arn"
properties = ["assumption_type"]    # direct, via_group, via_federation

[[edges]]
label = "ROLE_HAS_POLICY"
source = "iceberg.iam.role_policies"
from_node = "Role"
from_column = "role_arn"
to_node = "Policy"
to_column = "policy_arn"

[[edges]]
label = "ALLOWS_ACTION"
source = "iceberg.iam.policy_permissions"
from_node = "Policy"
from_column = "policy_arn"
to_node = "Resource"
to_column = "resource_arn"
properties = ["actions", "effect", "conditions"]
weight_column = "permission_score"

[[edges]]
label = "TRUSTS"
source = "iceberg.iam.trust_relationships"
from_node = "Role"
from_column = "role_arn"
to_node = "User"
to_column = "trusted_principal_arn"
properties = ["trust_type"]

# === COMPUTED PROPERTIES ===

[[properties]]
name = "is_privileged"
node = "User"
expression = """
  EXISTS (
    SELECT 1 FROM iceberg.iam.user_policies up
    JOIN iceberg.iam.policies p ON up.policy_arn = p.policy_arn
    WHERE up.user_arn = User.user_arn
    AND p.policy_name LIKE '%Admin%'
  )
"""
materialized = true
```

## 9. Runtime Behavior

### 9.1 Schema Loading

```rust
// Rust API
let ontology = Ontology::from_file("fusiongraph.toml")?;
// `GraphTableProvider` is a trait; construct a concrete implementation here.
let provider = MyGraphTableProvider::new(catalog, ontology)?;
ctx.register_table("graph", Arc::new(provider))?;
```

### 9.2 Lazy Projection

Nodes and edges are **not** loaded at schema registration. The CSR is built lazily when:
1. A traversal query is executed
2. `CALL graph.materialize()` is invoked explicitly

### 9.3 Incremental Updates

When Iceberg tables receive new data:
1. FusionGraph detects new snapshots via manifest polling
2. Only new/modified Parquet files are scanned
3. Delta layer receives incremental edges
4. Background compaction merges into Base layer

---

## 10. Appendix: Reserved Labels

The following labels are reserved for internal use:

| Label | Purpose |
|-------|---------|
| `__ROOT__` | Virtual root for forest algorithms |
| `__UNKNOWN__` | Dangling edge target placeholder |
| `__DELETED__` | Tombstone marker in Delta layer |
