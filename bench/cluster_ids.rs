//! Prints, for each line of a log, the id of the template logdelta assigns it to.
//!
//! ```console
//! $ cargo run --release --example cluster_ids -- app.log > ids.txt
//! ```
//!
//! The lines go through the same steps as `logdelta templates`: masking and tokenizing
//! ([`logdelta::mask::tokenize_line`]), then the miner. The output is what a benchmark
//! of log parsers scores: which lines a parser puts together. `studies/loghub` uses it
//! to compute grouping accuracy against Loghub's hand-labelled templates.

use std::io::{self, BufWriter, Write};

use logdelta::drain::{Drain, DEFAULT_SIMILARITY_THRESHOLD};
use logdelta::io::read_lines;
use logdelta::mask::tokenize_line;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: cluster_ids <log file> [similarity threshold]");
        std::process::exit(2);
    };
    let threshold = match args.next() {
        Some(raw) => raw.parse().unwrap_or_else(|_| {
            eprintln!("the similarity threshold must be a number, got {raw}");
            std::process::exit(2);
        }),
        None => DEFAULT_SIMILARITY_THRESHOLD,
    };

    let mut drain = Drain::new(threshold);
    let mut ids = Vec::new();
    for (index, line) in read_lines(&path)?.enumerate() {
        let line = line?;
        ids.push(drain.add_tokens(tokenize_line(&line, &[]), index + 1, &line));
    }
    let mut out = BufWriter::new(io::stdout().lock());
    for id in ids {
        writeln!(out, "{id}")?;
    }
    out.flush()
}
