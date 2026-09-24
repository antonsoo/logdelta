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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Direction, Finding, FindingKind};

    #[test]
    fn diff_json_round_trips_through_serde_json_value() {
        let result = DiffResult {
            baseline_totals: vec![10, 12],
            target_total: 11,
            findings: vec![Finding {
                kind: FindingKind::New,
                direction: Direction::Up,
                template: "boom <NUM>".to_string(),
                score: 42.5,
                baseline_counts: vec![0, 0],
                target_count: 1,
                first_target_line_no: Some(7),
                first_target_raw: Some("boom 42".to_string()),
                context: None,
            }],
        };
        let mut buf = Vec::new();
        write_diff(&mut buf, &result).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["target_total"], 11);
        assert_eq!(v["findings"][0]["kind"], "new");
        assert_eq!(v["findings"][0]["direction"], "up");
        assert_eq!(v["findings"][0]["template"], "boom <NUM>");
        // context is None and should be omitted entirely, not emitted as `null`.
        assert!(v["findings"][0].get("context").is_none());
    }
}
