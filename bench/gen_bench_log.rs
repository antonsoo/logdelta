//! Generates a large, deterministic synthetic log for throughput benchmarking.
//!
//! Not part of the library or CLI; run directly with:
//!   cargo run --release --example gen_bench_log -- <line-count> <output-path> [seed]
//!
//! The output mixes a handful of templates (HTTP access lines, job completions, retries,
//! and a rare error) with masking-relevant variable fields (timestamps, UUIDs, IPs,
//! durations, byte sizes) so it exercises the same code paths as a real log, at a size real
//! logs rarely reach in a repo. A tiny xorshift PRNG keeps this dependency-free and
//! reproducible across machines and Rust versions given the same seed.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn hex(rng: &mut Rng, len: usize) -> String {
    (0..len)
        .map(|_| std::char::from_digit((rng.below(16)) as u32, 16).unwrap())
        .collect()
}

fn ip(rng: &mut Rng) -> String {
    format!(
        "10.{}.{}.{}",
        rng.below(255),
        rng.below(255),
        rng.below(255)
    )
}

fn timestamp(base_secs: u64, offset_ms: u64) -> String {
    let total_ms = base_secs * 1000 + offset_ms;
    let secs = total_ms / 1000;
    let ms = total_ms % 1000;
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    // Not calendar-accurate; only needs to look like a real ISO timestamp and be masked
    // correctly, which it is regardless of the actual date arithmetic.
    format!(
        "2024-01-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1 + (days % 28),
        h,
        m,
        s,
        ms
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gen_bench_log <line-count> <output-path> [seed]");
        std::process::exit(2);
    }
    let count: u64 = args[1].parse().expect("line-count must be a number");
    let path = &args[2];
    let seed: u64 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(42);

    let mut rng = Rng(seed | 1);
    let file = File::create(path).expect("create output file");
    let mut out = BufWriter::with_capacity(1 << 20, file);

    for i in 0..count {
        let t = timestamp(1_700_000_000, i * 7);
        let roll = rng.below(1000);
        let line = if roll < 550 {
            format!(
                "{t} INFO {}:{} - \"GET /api/v1/items/{} HTTP/1.1\" 200 {}",
                ip(&mut rng),
                8000 + rng.below(40),
                rng.below(100_000),
                64 + rng.below(4096)
            )
        } else if roll < 800 {
            format!(
                "{t} INFO job {} completed in {}ms",
                hex(&mut rng, 8),
                1 + rng.below(500)
            )
        } else if roll < 970 {
            format!(
                "{t} WARN retrying connection to {}:{} (attempt {})",
                ip(&mut rng),
                5432,
                1 + rng.below(3)
            )
        } else if roll < 998 {
            format!(
                "{t} INFO uploaded {}KiB to s3://bucket/{}/data.bin",
                1 + rng.below(2048),
                hex(&mut rng, 12)
            )
        } else {
            format!(
                "{t} ERROR job {} failed: connection reset by peer after {}ms",
                hex(&mut rng, 8),
                100 + rng.below(9000)
            )
        };
        out.write_all(line.as_bytes()).unwrap();
        out.write_all(b"\n").unwrap();
    }
    out.flush().unwrap();
}
