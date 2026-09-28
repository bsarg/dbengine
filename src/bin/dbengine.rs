use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use dbengine::{ClockPolicy, Database, LfuPolicy, LruPolicy};

fn usage() -> ! {
    eprintln!(
        "Usage: dbengine --file <path.db> [--pool-size N] [--eval SQL] [--policy lru|clock|lfu]"
    );
    std::process::exit(2);
}

struct Args {
    file: PathBuf,
    pool_size: usize,
    eval: Option<String>,
    policy: String,
}

fn parse_args() -> Args {
    let mut file = None;
    let mut pool_size = 64usize;
    let mut eval = None;
    let mut policy = "lru".to_string();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--file" => file = it.next().map(PathBuf::from),
            "--pool-size" => {
                pool_size = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--eval" => eval = it.next(),
            "--policy" => policy = it.next().unwrap_or_else(|| usage()),
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown arg {other}");
                usage();
            }
        }
    }
    let file = file.unwrap_or_else(|| usage());
    Args {
        file,
        pool_size,
        eval,
        policy,
    }
}

fn run_with<P: dbengine::EvictionPolicy>(args: &Args) -> dbengine::Result<()> {
    let mut db = Database::<P>::open(&args.file, args.pool_size)?;
    if let Some(sql) = &args.eval {
        let res = db.execute(sql)?;
        println!("{}", res.format());
        db.flush()?;
        return Ok(());
    }
    println!("dbengine REPL  file={}  policy={}", args.file.display(), args.policy);
    println!("Enter SQL (CREATE/INSERT/SELECT). Empty line or .quit to exit.");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() || t == ".quit" || t == ".exit" {
            break;
        }
        match db.execute(t) {
            Ok(r) => writeln!(stdout, "{}", r.format())?,
            Err(e) => writeln!(stdout, "error: {e}")?,
        }
        stdout.flush()?;
    }
    db.flush()?;
    Ok(())
}

fn main() {
    let args = parse_args();
    let result = match args.policy.to_ascii_lowercase().as_str() {
        "lru" => run_with::<LruPolicy>(&args),
        "clock" => run_with::<ClockPolicy>(&args),
        "lfu" => run_with::<LfuPolicy>(&args),
        other => {
            eprintln!("unknown policy {other}");
            usage();
        }
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
