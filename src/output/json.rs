//! `--json` rendering: a thin, stable wrapper around the already-`Serialize` analysis types,
//! so the JSON schema is a direct reflection of the data model (see `analysis.rs`), not a
//! separately hand-maintained shape.

use std::io::{self, Write};

use serde::Serialize;

use crate::analysis::{DiffResult, RunSummary};

pub fn write_diff<W: Write>(out: &mut W, result: &DiffResult) -> io::Result<()> {
    write_pretty(out, result)
}

pub fn write_templates<W: Write>(out: &mut W, summary: &RunSummary) -> io::Result<()> {
    write_pretty(out, summary)
}

fn write_pretty<W: Write, T: Serialize>(out: &mut W, value: &T) -> io::Result<()> {
    let s = serde_json::to_string_pretty(value).expect("analysis types always serialize");
    writeln!(out, "{s}")
}
