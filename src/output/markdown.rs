//! `--markdown` rendering, meant for `$GITHUB_STEP_SUMMARY` or a PR comment: GitHub renders
//! this directly, no ANSI involved.

use std::io::{self, Write};

use crate::analysis::{DiffResult, Finding, FindingKind};
use crate::blocks::Block;
use crate::output::{
    block_lines, clip, head_and_tail, items_of_kind, printable, Item, MAX_SHOWN_CHARS,
};

/// A template, value or log line as one table cell: without escape sequences, clipped (a PR
/// comment holds 65,536 characters, and one minified line is longer than that), with `|`
/// and newlines neutralized.
fn escape(s: &str) -> String {
    clip(&printable(s), MAX_SHOWN_CHARS).replace('|', "\\|")
}

/// A fence no line of `lines` can close: one backtick longer than the longest run of
/// backticks any of them holds, and never shorter than the usual three.
fn fence(lines: &[String]) -> String {
    let mut longest = 0;
    for line in lines {
        let mut run = 0;
        for ch in line.chars() {
            run = if ch == '`' { run + 1 } else { 0 };
            longest = longest.max(run);
        }
    }
    "`".repeat((longest + 1).max(3))
}

/// A block as a heading line and a fenced excerpt: its lines, one per template, with their
/// numbers. A block longer than `block_lines_shown` keeps its start and its end.
fn write_block<W: Write>(
    out: &mut W,
    block: &Block,
    findings: &[Finding],
    path: &str,
    block_lines_shown: usize,
) -> io::Result<()> {
    let lines = block_lines(block, findings);
    let elsewhere = if block.lines_elsewhere > 0 {
        format!(" (+{} of these lines further on)", block.lines_elsewhere)
    } else {
        String::new()
    };
    writeln!(
        out,
        "**{} templates, {} line{}** at `{path}:{}-{}`{elsewhere}, score {:.1}",
        lines.len(),
        block.line_count,
        if block.line_count == 1 { "" } else { "s" },
        block.first_line_no,
        block.last_line_no,
        block.score,
    )?;
    writeln!(out)?;

    let (head, tail) = head_and_tail(lines.len(), block_lines_shown);
    let hidden = lines.len() - head - tail;
    let mut shown: Vec<String> = Vec::with_capacity(head + tail + 1);
    let row = |line: &crate::output::BlockLine| {
        let times = if line.count > 1 {
            format!("  ×{}", line.count)
        } else {
            String::new()
        };
        format!(
            "{:>6} | {}{times}",
            line.line_no,
            clip(&printable(line.raw), MAX_SHOWN_CHARS)
        )
    };
    shown.extend(lines[..head].iter().map(row));
    if hidden > 0 {
        shown.push(format!("       ⋯ {hidden} more"));
    }
    shown.extend(lines[lines.len() - tail..].iter().map(row));

    let fence = fence(&shown);
    writeln!(out, "{fence}text")?;
    for line in &shown {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "{fence}")?;
    writeln!(out)
}

pub fn write_diff<W: Write>(
    out: &mut W,
    result: &DiffResult,
    baseline_paths: &[&str],
    target_path: &str,
    block_lines_shown: usize,
) -> io::Result<()> {
    let n_findings = result.report_count();
    let n_ungrouped = result.finding_count();
    let first_baseline = baseline_paths.first().copied().unwrap_or("baseline");
    let ungrouped = if result.blocks.is_empty() {
        String::new()
    } else {
        format!(" ({n_ungrouped} before grouping)")
    };

    writeln!(out, "### logdelta diff")?;
    writeln!(out)?;
    writeln!(
        out,
        "Baseline: `{}` ({} lines) — Target: `{}` ({} lines) — {} templates, {} finding{}{ungrouped}",
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
        let items = items_of_kind(kind, &result.blocks, &result.findings);
        if items.is_empty() {
            continue;
        }
        writeln!(out, "#### {title}")?;
        writeln!(out)?;
        // Blocks first, each with its excerpt; then one table for the findings on their own.
        let mut alone: Vec<&Finding> = Vec::new();
        for item in items {
            match item {
                Item::Finding(f) => alone.push(f),
                Item::Block(block) => {
                    let path = match block.kind {
                        FindingKind::Gone => first_baseline,
                        _ => target_path,
                    };
                    write_block(out, block, &result.findings, path, block_lines_shown)?;
                }
            }
        }
        if alone.is_empty() {
            continue;
        }
        let where_title = match kind {
            FindingKind::Gone => "Last seen",
            _ => "First seen",
        };
        writeln!(
            out,
            "| Score | Baseline | Target | Template | {where_title} |"
        )?;
        writeln!(out, "|---:|---:|---:|---|---|")?;
        for f in alone {
            let baseline: Vec<String> = f.baseline_counts.iter().map(u64::to_string).collect();
            let first_seen = match (
                (f.first_target_line_no, &f.first_target_raw),
                (f.first_baseline_line_no, &f.first_baseline_raw),
            ) {
                ((Some(n), Some(raw)), _) => format!("`{target_path}:{n}` `{}`", escape(raw)),
                // Gone: the target has no such line, so point at the one the baseline had.
                (_, (Some(n), Some(raw))) => format!("`{first_baseline}:{n}` `{}`", escape(raw)),
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

    if result.ungrouped_values().next().is_some() {
        writeln!(out, "#### New value")?;
        writeln!(out)?;
        writeln!(
            out,
            "| Template | New value | Baseline value(s) | First seen |"
        )?;
        writeln!(out, "|---|---|---|---|")?;
        for v in result.ungrouped_values() {
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
    use super::{escape, fence};

    #[test]
    fn a_fence_is_longer_than_any_backtick_run_inside_it() {
        assert_eq!(fence(&["plain".to_string()]), "```");
        assert_eq!(fence(&["a ``` b".to_string()]), "````");
        assert_eq!(fence(&["`````".to_string(), "``".to_string()]), "``````");
    }

    #[test]
    fn escapes_pipes_so_they_dont_break_the_table() {
        assert_eq!(escape("a | b"), "a \\| b");
    }

    #[test]
    fn flattens_embedded_newlines() {
        assert_eq!(escape("line one\nline two"), "line one line two");
    }

    #[test]
    fn clips_a_cell_that_would_swamp_the_table() {
        let cell = escape(&"x".repeat(5000));
        assert!(cell.starts_with(&"x".repeat(400)));
        assert!(cell.ends_with("… (+4600 more characters)"));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(escape("nothing special here"), "nothing special here");
    }
}
