use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use dbengine::eviction::{ClockPolicy, LfuPolicy, LruPolicy};
use dbengine::workload::{
    generate_keys, generate_page_ids, run_index_bench, run_page_bench, BenchRow, Pattern,
};

fn usage() -> ! {
    eprintln!(
        "Usage: bench [--out results.csv] [--ops N] [--pages N] [--keys N] [--seed N] [--mode page|index|both]"
    );
    std::process::exit(2);
}

fn main() {
    let mut out = PathBuf::from("results.csv");
    let mut ops = 4000usize;
    let mut pages = 200u32;
    let mut keys = 800u32;
    let mut seed = 1u64;
    let mut mode = "both".to_string();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--ops" => {
                ops = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--pages" => {
                pages = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--keys" => {
                keys = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--seed" => {
                seed = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--mode" => mode = it.next().unwrap_or_else(|| usage()),
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown arg {other}");
                usage();
            }
        }
    }
    if !matches!(mode.as_str(), "page" | "index" | "both") {
        eprintln!("--mode must be page, index, or both");
        usage();
    }

    let pool_sizes = [32usize, 128, 512];
    let patterns = [
        (Pattern::Sequential, "sequential"),
        (Pattern::Random, "random"),
        (Pattern::Skewed, "skewed"),
    ];
    let warmup = ops / 10;
    let mut rows = Vec::new();

    for pool_size in pool_sizes {
        for (pat, pname) in patterns {
            let tmp_base = format!("dbengine-bench-{pname}-{pool_size}");
            if mode == "page" || mode == "both" {
                let accesses = generate_page_ids(pat, pages, ops, seed);
                push_page(&mut rows, &tmp_base, pool_size, pages, &accesses, pname, warmup);
            }
            if mode == "index" || mode == "both" {
                let accesses = generate_keys(pat, keys, ops, seed);
                push_index(&mut rows, &tmp_base, pool_size, keys, &accesses, pname, warmup);
            }
        }
    }

    let mut f = File::create(&out).expect("create csv");
    writeln!(f, "{}", BenchRow::csv_header()).unwrap();
    for r in &rows {
        writeln!(f, "{}", r.to_csv()).unwrap();
        println!("{}", r.to_csv());
    }
    eprintln!("wrote {} ({} rows)", out.display(), rows.len());
}

fn push_page(
    rows: &mut Vec<BenchRow>,
    tmp_base: &str,
    pool_size: usize,
    pages: u32,
    accesses: &[dbengine::PageId],
    pname: &'static str,
    warmup: usize,
) {
    run_one(rows, tmp_base, "lru", |path| {
        run_page_bench::<LruPolicy>(path, pool_size, pages, accesses, "lru", pname, warmup)
    });
    run_one(rows, tmp_base, "clock", |path| {
        run_page_bench::<ClockPolicy>(path, pool_size, pages, accesses, "clock", pname, warmup)
    });
    run_one(rows, tmp_base, "lfu", |path| {
        run_page_bench::<LfuPolicy>(path, pool_size, pages, accesses, "lfu", pname, warmup)
    });
}

fn push_index(
    rows: &mut Vec<BenchRow>,
    tmp_base: &str,
    pool_size: usize,
    keys: u32,
    accesses: &[i64],
    pname: &'static str,
    warmup: usize,
) {
    run_one(rows, &format!("{tmp_base}-index"), "lru", |path| {
        run_index_bench::<LruPolicy>(path, pool_size, keys, accesses, "lru", pname, warmup)
    });
    run_one(rows, &format!("{tmp_base}-index"), "clock", |path| {
        run_index_bench::<ClockPolicy>(path, pool_size, keys, accesses, "clock", pname, warmup)
    });
    run_one(rows, &format!("{tmp_base}-index"), "lfu", |path| {
        run_index_bench::<LfuPolicy>(path, pool_size, keys, accesses, "lfu", pname, warmup)
    });
}

fn run_one(
    rows: &mut Vec<BenchRow>,
    tmp_base: &str,
    policy: &str,
    f: impl FnOnce(&std::path::Path) -> dbengine::Result<BenchRow>,
) {
    let tmp = std::env::temp_dir().join(format!("{tmp_base}-{policy}.db"));
    let _ = std::fs::remove_file(&tmp);
    match f(&tmp) {
        Ok(r) => rows.push(r),
        Err(e) => {
            eprintln!("{policy}: {e}");
            std::process::exit(1);
        }
    }
    let _ = std::fs::remove_file(&tmp);
}
