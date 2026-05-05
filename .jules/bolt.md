## 2025-03-08 - [Batch Database Inserts]
**Learning:** During repository ingestion (`crates/dk-engine/src/repo.rs`), the application parses files and inserts extracted symbols, types, and call edges into the database one by one inside a loop. This creates an N+1 query performance bottleneck that limits indexing throughput, especially for large files.
**Action:** Use `sqlx::QueryBuilder` with `push_values` to implement batch inserts/upserts when processing multiple records extracted from a single file's AST.
