//! `--markdown` rendering, meant for `$GITHUB_STEP_SUMMARY` or a PR comment: GitHub renders
//! this directly, no ANSI involved.

use std::io::{self, Write};

use crate::analysis::{DiffResult, Finding, FindingKind};
use crate::blocks::Block;
use crate::output::{
    block_excerpt, block_lines, clip, items_of_kind, printable, ExcerptRow, Item, MAX_SHOWN_CHARS,
};

/// A real Markdown code span: HTML <code> wrappers still parse links/emphasis,
/// and even entity-escaped email addresses can become GFM autolinks inside them.
pub(super) fn code(text: &str) -> String {
    literal(text, false)
}

/// GFM tables consume one backslash before a pipe, including inside code spans.
/// Paragraph spans must not add that backslash because no table consumes it.
pub(super) fn cell(text: &str) -> String {
    literal(text, true)
}

fn literal(text: &str, in_table: bool) -> String {
    let printable = printable(text);
    let text = clip(&printable, MAX_SHOWN_CHARS);
    if text.is_empty() {
        return "<code></code>".into();
    }
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let delimiter = "`".repeat(longest + 1);
    // Padding separates boundary backticks from the delimiter and preserves
    // leading/trailing spaces that Markdown would otherwise trim once.
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !text.chars().all(|c| c == ' '));
    let text = if in_table {
        text.replace('|', "\\|")
    } else {
        text.into_owned()
    };
    let padding = if pad { " " } else { "" };
    format!("{delimiter}{padding}{text}{padding}{delimiter}")
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
/// numbers. Long blocks prioritize diagnostic-looking lines and mark every gap.
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
        "**{} templates, {} line{}** at {}{elsewhere}, score {:.1}",
        lines.len(),
        block.line_count,
        if block.line_count == 1 { "" } else { "s" },
        code(&format!(
            "{path}:{}-{}",
            block.first_line_no, block.last_line_no
        )),
        block.score,
    )?;
    writeln!(out)?;

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
    let shown: Vec<String> = block_excerpt(&lines, block_lines_shown)
        .into_iter()
        .map(|selection| match selection {
            ExcerptRow::Line(index) => row(&lines[index]),
            ExcerptRow::Gap(hidden) => format!("       ⋯ {hidden} more templates"),
        })
        .collect();

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
        "Baseline: {} ({} lines) — Target: {} ({} lines) — {} templates, {} finding{}{ungrouped}",
        baseline_paths
            .iter()
            .map(|path| code(path))
            .collect::<Vec<_>>()
            .join(", "),
        result.baseline_totals.iter().sum::<u64>(),
        code(target_path),
        result.target_total,
        result.total_templates,
        n_findings,
        if n_findings == 1 { "" } else { "s" },
    )?;
    writeln!(out)?;

    super::fields::markdown(out, &result.watched_fields)?;
    if !result.complete() {
        writeln!(
            out,
            "\n**Field comparison incomplete. This report cannot establish a clean result.**\n"
        )?;
    }
    if n_findings == 0 && result.complete() {
        writeln!(out, "No significant differences found.")?;
        return Ok(());
    }
    if block_lines_shown > 0
        && result
            .blocks
            .iter()
            .any(|block| block_lines(block, &result.findings).len() > block_lines_shown)
    {
        writeln!(
            out,
            "Long blocks favor source diagnostics and error markers. \
             Use `--block-lines 0` to show every template; `--json` keeps full recorded text.\n"
        )?;
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
                ((Some(n), Some(raw)), _) => {
                    format!("{} {}", cell(&format!("{target_path}:{n}")), cell(raw))
                }
                // Gone: the target has no such line, so point at the one the baseline had.
                (_, (Some(n), Some(raw))) => {
                    format!("{} {}", cell(&format!("{first_baseline}:{n}")), cell(raw))
                }
                _ => String::from("—"),
            };
            writeln!(
                out,
                "| {:.1} | {} | {} | {} | {} |",
                f.score,
                baseline.join("/"),
                f.target_count,
                cell(&f.template),
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
                "{} {}",
                cell(&format!("{target_path}:{}", v.first_target_line_no)),
                cell(&v.first_target_raw)
            );
            writeln!(
                out,
                "| {} | {} | {} | {} |",
                cell(&v.template),
                cell(&v.new_value),
                cell(&v.baseline_values.join(", ")),
                first_seen,
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cell, code, fence};

    #[test]
    fn a_fence_is_longer_than_any_backtick_run_inside_it() {
        assert_eq!(fence(&["plain".to_string()]), "```");
        assert_eq!(fence(&["a ``` b".to_string()]), "````");
        assert_eq!(fence(&["`````".to_string(), "``".to_string()]), "``````");
    }

    #[test]
    fn escapes_pipes_so_they_dont_break_the_table() {
        assert_eq!(cell("a | b"), "`a \\| b`");
        assert_eq!(cell(r"a\|b"), r"`a\\|b`");
        assert_eq!(code(r"a\|b"), r"`a\|b`");
    }

    #[test]
    fn flattens_embedded_newlines() {
        assert_eq!(code("line one\nline two"), "`line one line two`");
    }

    #[test]
    fn clips_a_cell_that_would_swamp_the_table() {
        let cell = cell(&"x".repeat(5000));
        assert!(cell.starts_with(&format!("`{}", "x".repeat(400))));
        assert!(cell.ends_with("… (+4600 more characters)`"));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(code("nothing special here"), "`nothing special here`");
    }

    #[test]
    fn preserves_boundary_backticks_spaces_and_markup_as_code() {
        assert_eq!(
            code("`![PASS](https://example.invalid)`"),
            "`` `![PASS](https://example.invalid)` ``"
        );
        assert_eq!(code("a `` b ` c"), "```a `` b ` c```");
        assert_eq!(code("  padded  "), "`   padded   `");
        assert_eq!(code("   "), "`   `");
        assert_eq!(code(""), "<code></code>");
        assert_eq!(code("test@example.com"), "`test@example.com`");
    }
}
