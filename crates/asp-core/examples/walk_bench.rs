//! Spike: how fast can we scan a big tree for changes (mtime+size index walk)?
//!
//! This is the prototype of asp's change-detection fast path. Run with:
//!   cargo run --release --example walk_bench -- <tree> [--iters N]

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("usage: walk_bench <tree> [--iters N]"));
    let iters: u32 = match (args.next().as_deref(), args.next()) {
        (Some("--iters"), Some(n)) => n.parse().unwrap_or(3),
        _ => 3,
    };

    for i in 0..iters {
        let t0 = Instant::now();
        let mut files = 0u64;
        let mut bytes = 0u64;
        let mut newest = std::time::SystemTime::UNIX_EPOCH;
        for entry in walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".git" && e.file_name() != ".asp")
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                files += 1;
                if let Ok(md) = entry.metadata() {
                    bytes += md.len();
                    if let Ok(m) = md.modified() {
                        if m > newest {
                            newest = m;
                        }
                    }
                }
            }
        }
        println!(
            "iter {}: {} files, {:.2} GiB, scanned in {:.1} ms",
            i,
            files,
            bytes as f64 / (1u64 << 30) as f64,
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}
