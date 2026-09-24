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
    let n_findings = result.findings.len() + result.value_findings.len();

    writeln!(out, "### logdelta diff")?;
    writeln!(out)?;
    writeln!(
        out,
        "Baseline: `{}` ({} lines) — Target: `{}` ({} lines) — {} templates, {} finding{}",
        baseline_paths.join("`, `"),
        result.baseline_totals.iter().sum::<u64>(),
        target_path,
        result.target_total,
        result.total_templates,
        n_findings,
        if n_findings == 1 { "" } else { "s" },
    )?;
    writeln!(out)?;

    if n_findings == 0 {
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

    if !result.value_findings.is_empty() {
        writeln!(out, "#### New value")?;
        writeln!(out)?;
        writeln!(
            out,
            "| Template | New value | Baseline value(s) | First seen |"
        )?;
        writeln!(out, "|---|---|---|---|")?;
        for v in &result.value_findings {
            let first_seen = format!(
                "`{target_path}:{}` `{}`",
                v.first_target_line_no,
                escape(&v.first_target_raw)
            );
            writeln!(
                out,
                "| `{}` | `{}` | `{}` | {} |",
                escape(&v.template),
                escape(&v.new_value),
                escape(&v.baseline_values.join(", ")),
                first_seen,
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_pipes_so_they_dont_break_the_table() {
        assert_eq!(escape("a | b"), "a \\| b");
    }

    #[test]
    fn flattens_embedded_newlines() {
        assert_eq!(escape("line one\nline two"), "line one line two");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(escape("nothing special here"), "nothing special here");
    }
}
