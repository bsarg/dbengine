use crate::error::Result;
use crate::eviction::EvictionPolicy;
use crate::storage::buffer::BufferPool;
use crate::storage::page::PageId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Sequential,
    Random,
    Skewed,
}

/// Tiny deterministic LCG so benches need no extra crates.
#[derive(Clone, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed | 1,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.state
    }

    pub fn bounded(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % u64::from(n)) as u32
    }
}

pub fn generate_keys(pattern: Pattern, key_count: u32, ops: usize, seed: u64) -> Vec<i64> {
    generate_page_ids(pattern, key_count, ops, seed)
        .into_iter()
        .map(|id| i64::from(id.as_u32() - 1))
        .collect()
}

pub fn generate_page_ids(pattern: Pattern, page_count: u32, ops: usize, seed: u64) -> Vec<PageId> {
    assert!(page_count > 0);
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(ops);
    match pattern {
        Pattern::Sequential => {
            for i in 0..ops {
                out.push(PageId((i as u32 % page_count) + 1)); // skip header page 0
            }
        }
        Pattern::Random => {
            for _ in 0..ops {
                out.push(PageId(rng.bounded(page_count) + 1));
            }
        }
        Pattern::Skewed => {
            let hot = std::cmp::max(1, page_count / 5);
            for _ in 0..ops {
                let id = if rng.bounded(100) < 80 {
                    rng.bounded(hot) + 1
                } else {
                    rng.bounded(page_count) + 1
                };
                out.push(PageId(id));
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct BenchRow {
    pub kind: &'static str,
    pub policy: &'static str,
    pub pool_size: usize,
    pub pattern: &'static str,
    pub ops: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
    pub mean_ns: f64,
    pub p95_ns: u64,
}

impl BenchRow {
    pub fn csv_header() -> &'static str {
        "kind,policy,pool_size,pattern,ops,hits,misses,hit_rate,mean_ns,p95_ns"
    }

    pub fn to_csv(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{:.4},{:.1},{}",
            self.kind,
            self.policy,
            self.pool_size,
            self.pattern,
            self.ops,
            self.hits,
            self.misses,
            self.hit_rate,
            self.mean_ns,
            self.p95_ns
        )
    }
}

fn p95(samples: &mut [u64]) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let idx = ((samples.len() as f64) * 0.95).ceil() as usize;
    samples[idx.min(samples.len()) - 1]
}

/// Page-get microbenchmark: allocate `page_count` pages, then pin/unpin `accesses`.
pub fn run_page_bench<P: EvictionPolicy>(
    path: impl AsRef<std::path::Path>,
    pool_size: usize,
    page_count: u32,
    accesses: &[PageId],
    policy_name: &'static str,
    pattern_name: &'static str,
    warmup: usize,
) -> Result<BenchRow> {
    let mut pool = BufferPool::<P>::open(path, pool_size)?;
    let existing = pool.page_count().saturating_sub(1);
    for _ in existing..page_count {
        pool.allocate_page()?;
    }
    for id in accesses.iter().take(warmup) {
        if id.0 >= pool.page_count() {
            continue;
        }
        let f = pool.pin(*id)?;
        let _ = pool.data(f)[0];
        pool.unpin(f, false);
    }
    pool.reset_stats();
    let mut samples = Vec::with_capacity(accesses.len().saturating_sub(warmup));
    for id in accesses.iter().skip(warmup) {
        if id.0 >= pool.page_count() {
            continue;
        }
        let t0 = std::time::Instant::now();
        let f = pool.pin(*id)?;
        let _ = pool.data(f)[0];
        pool.unpin(f, false);
        samples.push(t0.elapsed().as_nanos() as u64);
    }
    let stats = pool.stats();
    let total = stats.hits + stats.misses;
    let hit_rate = if total == 0 {
        0.0
    } else {
        stats.hits as f64 / total as f64
    };
    let mean_ns = if samples.is_empty() {
        0.0
    } else {
        samples.iter().sum::<u64>() as f64 / samples.len() as f64
    };
    let p95_ns = p95(&mut samples);
    Ok(BenchRow {
        kind: "page",
        policy: policy_name,
        pool_size,
        pattern: pattern_name,
        ops: accesses.len().saturating_sub(warmup),
        hits: stats.hits,
        misses: stats.misses,
        hit_rate,
        mean_ns,
        p95_ns,
    })
}

/// Build a heap + B+Tree, then time point lookups (index get + heap fetch).
/// Measured stats start after `warmup` lookups so the build is not in the numbers.
pub fn run_index_bench<P: EvictionPolicy>(
    path: impl AsRef<std::path::Path>,
    pool_size: usize,
    key_count: u32,
    accesses: &[i64],
    policy_name: &'static str,
    pattern_name: &'static str,
    warmup: usize,
) -> Result<BenchRow> {
    use crate::index::BPlusTree;
    use crate::storage::heap::HeapFile;

    let mut pool = BufferPool::<P>::open(path, pool_size)?;
    let mut heap = HeapFile::create(&mut pool)?;
    let mut tree = BPlusTree::create(&mut pool)?;
    for k in 0..key_count {
        let rec = (k as i64).to_le_bytes();
        let rid = heap.insert(&mut pool, &rec)?;
        tree.insert(&mut pool, k as i64, rid)?;
    }
    for key in accesses.iter().take(warmup) {
        let rids = tree.get(&mut pool, *key)?;
        if let Some(rid) = rids.first().copied() {
            let _ = heap.get(&mut pool, rid)?;
        }
    }
    pool.reset_stats();
    let mut samples = Vec::with_capacity(accesses.len().saturating_sub(warmup));
    for key in accesses.iter().skip(warmup) {
        let t0 = std::time::Instant::now();
        let rids = tree.get(&mut pool, *key)?;
        if let Some(rid) = rids.first().copied() {
            let _ = heap.get(&mut pool, rid)?;
        }
        samples.push(t0.elapsed().as_nanos() as u64);
    }
    let stats = pool.stats();
    let total = stats.hits + stats.misses;
    let hit_rate = if total == 0 {
        0.0
    } else {
        stats.hits as f64 / total as f64
    };
    let mean_ns = if samples.is_empty() {
        0.0
    } else {
        samples.iter().sum::<u64>() as f64 / samples.len() as f64
    };
    let p95_ns = p95(&mut samples);
    Ok(BenchRow {
        kind: "index",
        policy: policy_name,
        pool_size,
        pattern: pattern_name,
        ops: accesses.len().saturating_sub(warmup),
        hits: stats.hits,
        misses: stats.misses,
        hit_rate,
        mean_ns,
        p95_ns,
    })
}

pub fn tiny_matrix_for_tests() -> Result<Vec<BenchRow>> {
    use crate::eviction::{ClockPolicy, LfuPolicy, LruPolicy};
    use crate::test_util::temp_db_path;

    let page_count = 12u32;
    let ops = 40usize;
    let seed = 42u64;
    let mut rows = Vec::new();
    for (pattern, pname) in [
        (Pattern::Sequential, "sequential"),
        (Pattern::Random, "random"),
        (Pattern::Skewed, "skewed"),
    ] {
        let accesses = generate_page_ids(pattern, page_count, ops, seed);
        let path = temp_db_path("tiny-matrix");
        rows.push(run_page_bench::<LruPolicy>(
            &path, 4, page_count, &accesses, "lru", pname, 4,
        )?);
        let _ = std::fs::remove_file(&path);
        let path = temp_db_path("tiny-matrix");
        rows.push(run_page_bench::<ClockPolicy>(
            &path, 4, page_count, &accesses, "clock", pname, 4,
        )?);
        let _ = std::fs::remove_file(&path);
        let path = temp_db_path("tiny-matrix");
        rows.push(run_page_bench::<LfuPolicy>(
            &path, 4, page_count, &accesses, "lfu", pname, 4,
        )?);
        let _ = std::fs::remove_file(&path);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_deterministic() {
        let a = generate_page_ids(Pattern::Random, 10, 20, 99);
        let b = generate_page_ids(Pattern::Random, 10, 20, 99);
        assert_eq!(a, b);
        let c = generate_page_ids(Pattern::Random, 10, 20, 100);
        assert_ne!(a, c);
    }

    #[test]
    fn tiny_matrix_runs() {
        let rows = tiny_matrix_for_tests().unwrap();
        assert_eq!(rows.len(), 9);
        assert!(rows.iter().all(|r| r.kind == "page"));
        assert!(rows.iter().any(|r| r.hits + r.misses > 0));
    }

    #[test]
    fn index_bench_runs() {
        use crate::eviction::LruPolicy;
        use crate::test_util::temp_db_path;

        let keys = generate_keys(Pattern::Skewed, 40, 30, 7);
        let path = temp_db_path("index-bench");
        let row = run_index_bench::<LruPolicy>(&path, 8, 40, &keys, "lru", "skewed", 5).unwrap();
        assert_eq!(row.kind, "index");
        assert_eq!(row.ops, 25);
        assert!(row.hits + row.misses > 0);
        let _ = std::fs::remove_file(&path);
    }
}
