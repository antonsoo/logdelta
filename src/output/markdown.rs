//! `--markdown` rendering, meant for `$GITHUB_STEP_SUMMARY` or a PR comment: GitHub renders
//! this directly, no ANSI involved.

use std::io::{self, Write};

use crate::analysis::{DiffResult, FindingKind};

fn escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn write_diff<W: Write>(
    out: &mut W,
    result: &DiffResult,
    baseline_paths: &[&str],
    target_path: &str,
) -> io::Result<()> {
    writeln!(out, "### logdelta diff")?;
    writeln!(out)?;
    writeln!(
        out,
        "Baseline: `{}` ({} lines) — Target: `{}` ({} lines)",
        baseline_paths.join("`, `"),
        result.baseline_totals.iter().sum::<u64>(),
        target_path,
        result.target_total,
    )?;
    writeln!(out)?;

    if result.findings.is_empty() {
        writeln!(out, "No significant differences found.")?;
        return Ok(());
    }

    for (kind, title) in [
        (FindingKind::New, "New"),
        (FindingKind::Changed, "Changed"),
        (FindingKind::Gone, "Gone"),
    ] {
        let items: Vec<_> = result.findings.iter().filter(|f| f.kind == kind).collect();
        if items.is_empty() {
            continue;
        }
        writeln!(out, "#### {title}")?;
        writeln!(out)?;
        writeln!(out, "| Score | Baseline | Target | Template | First seen |")?;
        writeln!(out, "|---:|---:|---:|---|---|")?;
        for f in items {
            let baseline: Vec<String> = f.baseline_counts.iter().map(u64::to_string).collect();
            let first_seen = match (f.first_target_line_no, &f.first_target_raw) {
                (Some(n), Some(raw)) => format!("`{target_path}:{n}` `{}`", escape(raw)),
                _ => String::from("—"),
            };
            writeln!(
                out,
                "| {:.1} | {} | {} | `{}` | {} |",
                f.score,
                baseline.join("/"),
                f.target_count,
                escape(&f.template),
                first_seen,
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}
