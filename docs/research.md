# Eviction-policy comparison

This is the research component: **LRU vs Clock (second chance) vs LFU** on the
same page-based engine. Policies are selected by generics (`BufferPool<P>`), not
`dyn Trait`, and there is no garbage collector on the measurement path.

## How to reproduce

```bash
cargo test
cargo run --release --bin bench -- --out results.csv --mode both --ops 4000 --pages 200 --keys 800 --seed 1
```

A smaller run used for the sample table below:

```bash
cargo run --release --bin bench -- --out results.csv --mode both --ops 2000 --pages 120 --keys 400 --seed 1
```

Columns in `results.csv`:

`kind,policy,pool_size,pattern,ops,hits,misses,hit_rate,mean_ns,p95_ns`

`kind` is `page` or `index`.

Workloads (`src/workload.rs`):

- **sequential** — ids `0,1,2,...` wrapping.
- **random** — uniform ids.
- **skewed** — 80% of accesses hit the hottest 20% of ids.

Pool sizes: **32, 128, 512** pages (4 KiB each). The first 10% of ops are warmup and are not counted.

- **page**: allocate N pages, then pin + touch + unpin. This is the clean policy comparison.
- **index**: insert `keys` rows into a heap and a B+Tree, then point-lookup and fetch the row. Same pool, more pages touched per op (root, internals, leaf, heap page).

## Sample results (release, ops=2000, pages=120, keys=400, seed=1)

Hit rate after warmup. Mean latency is in nanoseconds for one measured op.

| kind | pattern | pool | LRU | Clock | LFU |
|---|---|---:|---:|---:|---:|
| page | sequential | 32 | 0.000 | 0.000 | 0.250 |
| page | random | 32 | 0.259 | 0.260 | 0.266 |
| page | skewed | 32 | 0.903 | 0.905 | 0.906 |
| page | sequential | 128 | 1.000 | 1.000 | 1.000 |
| page | random | 128 | 0.987 | 0.987 | 0.987 |
| page | skewed | 128 | 0.986 | 0.986 | 0.986 |
| index | sequential | 32 | 0.974 | 0.974 | 0.748 |
| index | random | 32 | 0.877 | 0.876 | 0.745 |
| index | skewed | 32 | 0.974 | 0.971 | 0.952 |
| index | any pattern | 128 or 512 | 1.000 | 1.000 | 1.000 |

Pool 512 matched pool 128 on this working set (120 pages, or a 400-key tree). Once the data fits, policy stops mattering.

Latency moved with misses. Page sequential, pool 32: mean about 1.1–2.1 µs. Same workload, pool 128 (100% hits): mean about 16–18 ns.

## What the numbers mean

- **Hit rate** = `hits / (hits + misses)` inside the buffer pool. A miss reads a page from disk and may write a dirty victim back.
- **Latency** is one measured op (`Instant`): a page pin, or an index lookup plus heap fetch. Compare policies on the same row. Nanoseconds are not a portable absolute.
- Run **`--release`**. Debug builds spend enough time on checks to hide policy differences.

## Reading this run

1. **Working set fits (pool 128 and 512).** All three policies hit about 100% on both kinds. Recency and frequency are irrelevant when nothing needs to be evicted.
2. **Page sequential, pool 32 (120 pages).** Reuse distance is 120 and capacity is 32, so LRU and Clock never see a page again before it is gone: **hit rate 0**. LFU kept about 25%. A wrapping scan does not touch every page the same number of times once warmup cuts the sequence, so a few pages have a higher count and stay resident. That is LFU helping on a scan, not a bug in LRU.
3. **Page random, pool 32.** Hit rate is about 26% for every policy, close to `32/120`. There is no locality to exploit. Clock tracks LRU, as expected for a second-chance approximation.
4. **Page skewed, pool 32.** Hot set is 20% of 120 = 24 pages, which fits in 32 frames. All three policies land near 90%. LFU is only slightly ahead (0.906 vs 0.903). The large LFU win shows up when the hot set does **not** fit, or when cold pages would otherwise pollute a recency cache. Here they mostly fit.
5. **Index lookups, pool 32.** LRU and Clock beat LFU (about 0.97 vs 0.75 sequential, 0.88 vs 0.75 random). A point lookup reuses the root and, on a sequential scan, the current leaf. That is recency. LFU has no aging, so pages that were hot while the tree was built can occupy frames and force out the leaf being scanned. On skewed lookups the gap shrinks (0.97 vs 0.95) because the hot keys really are frequent.
6. **p95 vs mean.** On the page microbench, p95 collapses from ~1–2 µs (many misses) to ~42 ns (hits). Misses dominate the tail.

## Caveats

- Single-threaded. No WAL. A crash can lose dirty pages.
- B+Tree nodes are decoded into an owned `Node` and encoded back (extra copies versus mutating the frame in place).
- The harness does not parse SQL, so parser time is not in the numbers. The SQL executor uses the same `BufferPool<P>`.
- Clock staying close to LRU is expected, not a failed implementation.
- LFU never decays. A key that was hot during index build can pin a frame for the rest of the run.
- Laptop latency is noisy. Prefer **hit rate** as the primary result; treat latency as support from a release build.
