//! Diff logs by meaning, not by bytes.
//!
//! `logdelta` masks the parts of each log line that vary from run to run (timestamps,
//! ids, durations, addresses), mines what is left into templates with the Drain
//! algorithm, and compares how often each template appears in one or more baseline
//! runs against a target run. The result says which templates are new in the target,
//! which are gone, which changed in frequency, and which variable took a value the
//! baselines never showed, and it groups the findings that are one event in the log (a
//! traceback, the steps a failed job skipped) into [`blocks`].
//!
//! The `logdelta` binary is the usual way in (`cargo install logdelta`); this library
//! is what it runs on, and what the browser demo compiles to WebAssembly. Build it
//! with `default-features = false` to leave out the command-line dependencies.
//!
//! # Example
//!
//! [`analysis::diff_lines`] works on any source of lines, here two logs held in memory:
//!
//! ```
//! use logdelta::analysis::{diff_lines, DiffOptions, FindingKind};
//!
//! fn lines(log: &str) -> impl Iterator<Item = std::io::Result<String>> + '_ {
//!     log.lines().map(|line| Ok(line.to_string()))
//! }
//!
//! let good = "\
//! 2024-01-01T00:00:00Z start
//! 2024-01-01T00:00:01Z ok 12ms
//! 2024-01-01T00:00:02Z ok 7ms";
//! let bad = "\
//! 2024-01-01T00:00:00Z start
//! 2024-01-01T00:00:01Z ok 9ms
//! 2024-01-01T00:00:02Z FATAL: out of memory";
//!
//! let diff = diff_lines(vec![lines(good)], lines(bad), &[], &DiffOptions::default())?;
//!
//! // The timestamps and the durations differ on every line, but only one template is new.
//! let new: Vec<_> = diff.findings.iter().filter(|f| f.kind == FindingKind::New).collect();
//! assert_eq!(new.len(), 1);
//! assert!(new[0].first_target_raw.as_deref().unwrap().ends_with("FATAL: out of memory"));
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! [`analysis::diff_runs`] and [`analysis::mine_run`] are the same operations over file
//! paths (plain or gzipped), and [`output`] renders a result for a terminal, as
//! Markdown or as JSON.

pub mod analysis;
pub mod blocks;
pub mod context;
pub mod drain;
pub mod field_rates;
pub mod fields;
pub mod io;
pub mod mask;
pub mod output;
pub mod scoring;
pub mod values;
