# dbengine

Single-node teaching database in Rust: page storage, a generic buffer pool
(LRU / Clock / LFU), a page-based B+Tree, a small SQL subset, and an eviction-policy
benchmark harness.

There is **no WAL, no crash recovery, and no transactions**. Dirty pages are written
on eviction or `flush`. A crash can lose data.

The first `INT` column of a table is the B+Tree index key.

## Build and test

```bash
cargo test
cargo build --release
```

## SQL CLI

```bash
cargo run --bin dbengine -- --file /tmp/demo.db --eval "CREATE TABLE t (id INT, name TEXT)"
cargo run --bin dbengine -- --file /tmp/demo.db --eval "INSERT INTO t VALUES (1, 'ada')"
cargo run --bin dbengine -- --file /tmp/demo.db --eval "SELECT * FROM t WHERE id = 1"
```

Supported statements:

- `CREATE TABLE name (col INT|TEXT, ...)` — at least one `INT` column
- `INSERT INTO name VALUES (...)`
- `SELECT * FROM name` and `SELECT * FROM name WHERE col OP literal`
  with `=`, `<>`, `<`, `>`, `<=`, `>=` (single condition)

`--policy lru|clock|lfu` selects the buffer pool eviction policy (static dispatch).
`--pool-size N` sets the number of cached pages (default 64).

## Benchmarks (research)

The `bench` binary talks to the buffer pool **directly** (no SQL parser) so policy
comparisons are not dominated by parsing.

```bash
cargo run --release --bin bench -- --out results.csv --mode both
```

Defaults: `--ops 4000 --pages 200 --keys 800 --seed 1`. `--mode` is `page`, `index`, or `both`.

Each mode varies **policy × pool size (32, 128, 512) × pattern (sequential, random, skewed)**.
Skewed = 80% of accesses on the hottest 20% of pages or keys.

- `page` — pin/unpin of allocated pages (no SQL).
- `index` — build a heap + B+Tree, then point-lookup keys and fetch the row.

Metrics: cache hit rate, mean latency, p95 latency.

See [docs/research.md](docs/research.md) for how to interpret results.

## Layout

| Module | Role |
|---|---|
| `storage/` | pages, file I/O, buffer pool, slotted heap |
| `eviction/` | `EvictionPolicy` trait + LRU, Clock, LFU |
| `index/` | B+Tree nodes (one node = one page) |
| `catalog/` | schema on header page 0 |
| `sql/` | tokenizer + recursive-descent parser |
| `exec/` | planner/executor (`Database<P>`) |
| `workload.rs` | access-pattern generator + page microbench |

B+Tree delete removes a key from a leaf and **does not** merge/redistribute (leaves may underflow).
