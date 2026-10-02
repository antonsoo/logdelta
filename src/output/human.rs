//! Human-readable terminal output: aligned, colour-coded (when enabled) sections for NEW /
//! GONE / CHANGED / NEW VALUE findings, and a ranked table for `logdelta templates`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};

use crate::analysis::{DiffResult, Finding, FindingKind, RunSummary};
use crate::blocks::Block;
use crate::context::ContextWindow;
use crate::mask::is_placeholder;

use super::{
    block_lines, clip, head_and_tail, highlight_template, items_of_kind, printable, BlockLine,
    Item, MAX_SHOWN_CHARS,
};

const INDENT: &str = "        ";
/// Fallback content width when stdout isn't a TTY (piped into a file, `$GITHUB_STEP_SUMMARY`
/// redirection, etc. all report no terminal size).
const DEFAULT_WIDTH: usize = 120;

fn style(color: AnsiColor, bold: bool, use_color: bool) -> (String, String) {
    if !use_color {
        return (String::new(), String::new());
    }
    let mut st = Style::new().fg_color(Some(color.into()));
    if bold {
        st = st.bold();
    }
    (st.render().to_string(), anstyle::Reset.render().to_string())
}

fn kind_label(kind: FindingKind, use_color: bool) -> String {
    let (color, text) = match kind {
        FindingKind::New => (AnsiColor::Red, "NEW"),
        FindingKind::Gone => (AnsiColor::Cyan, "GONE"),
        FindingKind::Changed => (AnsiColor::Yellow, "CHANGED"),
    };
    let (on, off) = style(color, true, use_color);
    format!("{on}{text:<9}{off}")
}

fn value_label(use_color: bool) -> String {
    let (on, off) = style(AnsiColor::Magenta, true, use_color);
    format!("{on}{:<9}{off}", "NEW VALUE")
}

fn dim(s: &str, use_color: bool) -> String {
    if !use_color {
        return s.to_string();
    }
    let st = Style::new().dimmed();
    format!("{st}{s}{}", anstyle::Reset)
}

fn format_counts(f: &Finding) -> String {
    let baseline: Vec<String> = f.baseline_counts.iter().map(u64::to_string).collect();
    if baseline.is_empty() {
        format!("target={}", f.target_count)
    } else {
        format!("baseline={} target={}", baseline.join("/"), f.target_count)
    }
}

/// Terminal column count: the real width when stdout is a TTY, [`DEFAULT_WIDTH`] otherwise
/// (piping into a file or another program never reports a size).
#[cfg(feature = "cli")]
fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(DEFAULT_WIDTH)
}

#[cfg(not(feature = "cli"))]
fn terminal_width() -> usize {
    DEFAULT_WIDTH
}

/// Truncates `s` to at most `max_chars` *characters* (not bytes, so multi-byte UTF-8 isn't
/// split mid-codepoint), replacing anything cut with a single `…`.
fn truncate(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars || max_chars == 0 {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Formats `n` with `,` thousands separators (`3104` -> `"3,104"`), for the header summary
/// line — the whole point of that line is to be skimmable at a glance on a log with
/// thousands of lines.
fn fmt_thousands(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(bytes.len() + bytes.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn write_location_and_raw<W: Write>(
    out: &mut W,
    width: usize,
    target_path: &str,
    line_no: usize,
    raw: &str,
    use_color: bool,
) -> io::Result<()> {
    writeln!(
        out,
        "{INDENT}{}",
        dim(
            &truncate(&format!("{target_path}:{line_no}"), width),
            use_color
        )
    )?;
    writeln!(out, "{INDENT}{}", truncate(&printable(raw), width))
}

fn write_context<W: Write>(
    out: &mut W,
    ctx: &ContextWindow,
    width: usize,
    line_no: usize,
    raw: &str,
    use_color: bool,
) -> io::Result<()> {
    for (n, r) in &ctx.before {
        writeln!(
            out,
            "{INDENT}{}",
            dim(
                &truncate(&format!("{n:>6} | {}", printable(r)), width),
                use_color
            )
        )?;
    }
    writeln!(
        out,
        "{INDENT}{}",
        truncate(&format!("{line_no:>6} > {}", printable(raw)), width)
    )?;
    for (n, r) in &ctx.after {
        writeln!(
            out,
            "{INDENT}{}",
            dim(
                &truncate(&format!("{n:>6} | {}", printable(r)), width),
                use_color
            )
        )?;
    }
    Ok(())
}

/// Renders a template with the token at `position` replaced by the actual `new_value` it took
/// in the target (styled to stand out), instead of the `<*>` wildcard it normally shows as.
/// A long template is clipped around that value, which is the thing the finding is about.
fn render_value_template(
    template: &str,
    position: usize,
    new_value: &str,
    use_color: bool,
) -> String {
    let (on, off) = style(AnsiColor::Red, true, use_color);
    let new_value = printable(new_value);
    let new_value = clip(&new_value, MAX_SHOWN_CHARS);
    let template = printable(template);
    let mut parts: Vec<String> = Vec::new();
    let mut shown = 0;
    let mut hidden = 0;
    let mut gap_marked = false;
    for (i, tok) in template.split(' ').enumerate() {
        if i == position {
            parts.push(format!("{on}{new_value}{off}"));
            gap_marked = false;
            continue;
        }
        let len = tok.chars().count() + 1;
        if shown + len > MAX_SHOWN_CHARS {
            hidden += len;
            if !gap_marked {
                parts.push("…".to_string());
                gap_marked = true;
            }
            continue;
        }
        shown += len;
        parts.push(if is_placeholder(tok) {
            highlight_template(tok, use_color)
        } else {
            tok.to_string()
        });
    }
    let mut out = parts.join(" ");
    if hidden > 0 {
        out.push_str(&format!(" (+{hidden} more characters)"));
    }
    out
}

/// One line of a block: its number, the line, and how many lines of the run share its
/// template when that is more than this one.
fn write_block_line<W: Write>(
    out: &mut W,
    line: &BlockLine,
    width: usize,
    use_color: bool,
) -> io::Result<()> {
    let times = if line.count > 1 {
        format!("  ×{}", fmt_thousands(line.count))
    } else {
        String::new()
    };
    let room = width.saturating_sub(times.chars().count()).max(20);
    writeln!(
        out,
        "{INDENT}{}{}",
        truncate(
            &format!("{:>6} | {}", line.line_no, printable(line.raw)),
            room
        ),
        dim(&times, use_color),
    )
}

/// A block: what it is in one line, where it is, then its lines, one per template. A block
/// longer than `block_lines` shows its start and its end and says how much is between them.
fn write_block<W: Write>(
    out: &mut W,
    block: &Block,
    findings: &[Finding],
    path: &str,
    width: usize,
    block_lines_shown: usize,
    use_color: bool,
) -> io::Result<()> {
    let lines = block_lines(block, findings);
    writeln!(
        out,
        "{}  {} templates · {} line{}  score={:.1}",
        kind_label(block.kind, use_color),
        fmt_thousands(lines.len() as u64),
        fmt_thousands(block.line_count),
        plural(block.line_count),
        block.score,
    )?;
    let mut location = format!("{path}:{}-{}", block.first_line_no, block.last_line_no);
    if block.lines_elsewhere > 0 {
        location.push_str(&format!(
            " (+{} of these lines further on)",
            fmt_thousands(block.lines_elsewhere)
        ));
    }
    writeln!(
        out,
        "{INDENT}{}",
        dim(&truncate(&location, width), use_color)
    )?;

    let (head, tail) = head_and_tail(lines.len(), block_lines_shown);
    for line in &lines[..head] {
        write_block_line(out, line, width, use_color)?;
    }
    let hidden = lines.len() - head - tail;
    if hidden > 0 {
        let note = format!("       ⋯ {} more", fmt_thousands(hidden as u64));
        writeln!(out, "{INDENT}{}", dim(&note, use_color))?;
    }
    for line in &lines[lines.len() - tail..] {
        write_block_line(out, line, width, use_color)?;
    }
    Ok(())
}

pub fn write_diff<W: Write>(
    out: &mut W,
    result: &DiffResult,
    baseline_paths: &[&str],
    target_path: &str,
    block_lines_shown: usize,
    use_color: bool,
) -> io::Result<()> {
    let n_findings = result.report_count();
    let n_ungrouped = result.finding_count();
    let content_width = terminal_width().saturating_sub(INDENT.len()).max(20);
    // A GONE finding points at the run that still had the line: the first baseline.
    let first_baseline = baseline_paths.first().copied().unwrap_or("baseline");

    writeln!(
        out,
        "logdelta diff  {} → {}",
        baseline_paths.join(", "),
        target_path,
    )?;
    // Grouped, the report is shorter than the list of templates that differ; say by how
    // much, and how to get that list.
    let ungrouped = if result.blocks.is_empty() {
        String::new()
    } else {
        format!(" ({} with --flat)", fmt_thousands(n_ungrouped as u64))
    };
    writeln!(
        out,
        "{} → {} lines · {} template{} · {} finding{}{ungrouped}",
        fmt_thousands(result.baseline_totals.iter().sum::<u64>()),
        fmt_thousands(result.target_total),
        fmt_thousands(result.total_templates as u64),
        plural(result.total_templates as u64),
        fmt_thousands(n_findings as u64),
        plural(n_findings as u64),
    )?;

    if n_findings == 0 {
        writeln!(out, "\nNo significant differences found.")?;
        return Ok(());
    }

    for kind in [FindingKind::New, FindingKind::Changed, FindingKind::Gone] {
        let items = items_of_kind(kind, &result.blocks, &result.findings);
        if items.is_empty() {
            continue;
        }
        writeln!(out)?;
        for item in items {
            let f = match item {
                Item::Finding(f) => f,
                Item::Block(block) => {
                    let path = match block.kind {
                        FindingKind::Gone => first_baseline,
                        _ => target_path,
                    };
                    write_block(
                        out,
                        block,
                        &result.findings,
                        path,
                        content_width,
                        block_lines_shown,
                        use_color,
                    )?;
                    continue;
                }
            };
            writeln!(
                out,
                "{}  {}  score={:.1}  {}",
                kind_label(f.kind, use_color),
                format_counts(f),
                f.score,
                highlight_template(&clip(&printable(&f.template), MAX_SHOWN_CHARS), use_color),
            )?;
            if let (Some(no), Some(raw)) = (f.first_target_line_no, &f.first_target_raw) {
                write_location_and_raw(out, content_width, target_path, no, raw, use_color)?;
            } else if let (Some(no), Some(raw)) = (f.first_baseline_line_no, &f.first_baseline_raw)
            {
                // Gone: there is no target line, so show the line the baseline had.
                write_location_and_raw(out, content_width, first_baseline, no, raw, use_color)?;
            }
            if let (Some(ctx), Some(no), Some(raw)) =
                (&f.context, f.first_target_line_no, &f.first_target_raw)
            {
                write_context(out, ctx, content_width, no, raw, use_color)?;
            }
        }
    }

    if result.ungrouped_values().next().is_some() {
        writeln!(out)?;
        for v in result.ungrouped_values() {
            writeln!(
                out,
                "{}  {}  {}",
                value_label(use_color),
                render_value_template(&v.template, v.position, &v.new_value, use_color),
                dim(
                    &format!(
                        "(baseline: {})",
                        clip(&printable(&v.baseline_values.join(", ")), MAX_SHOWN_CHARS)
                    ),
                    use_color
                ),
            )?;
            write_location_and_raw(
                out,
                content_width,
                target_path,
                v.first_target_line_no,
                &v.first_target_raw,
                use_color,
            )?;
            if let Some(ctx) = &v.context {
                write_context(
                    out,
                    ctx,
                    content_width,
                    v.first_target_line_no,
                    &v.first_target_raw,
                    use_color,
                )?;
            }
        }
    }
    Ok(())
}

pub fn write_templates<W: Write>(
    out: &mut W,
    summary: &RunSummary,
    limit: usize,
    use_color: bool,
) -> io::Result<()> {
    let mut clusters: Vec<_> = summary.clusters.iter().collect();
    clusters.sort_by(|a, b| b.count.cmp(&a.count));

    writeln!(
        out,
        "logdelta templates  {} line{}, {} distinct template{}",
        fmt_thousands(summary.total_lines),
        plural(summary.total_lines),
        clusters.len(),
        plural(clusters.len() as u64),
    )?;
    writeln!(out)?;

    let width = clusters
        .iter()
        .take(limit)
        .map(|c| c.count.to_string().len())
        .max()
        .unwrap_or(1);

    for c in clusters.into_iter().take(limit) {
        writeln!(
            out,
            "{:>width$}  {}",
            c.count,
            highlight_template(&clip(&printable(&c.template()), MAX_SHOWN_CHARS), use_color),
            width = width
        )?;
        writeln!(
            out,
            "{:width$}  {} {}:{}",
            "",
            dim("e.g.", use_color),
            c.first_line_no,
            clip(&printable(&c.first_line_raw), MAX_SHOWN_CHARS),
            width = width
        )?;
    }
    Ok(())
}

fn plural(n: u64) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_thousands_groups_digits() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(42), "42");
        assert_eq!(fmt_thousands(1234), "1,234");
        assert_eq!(fmt_thousands(3_104_000), "3,104,000");
    }

    #[test]
    fn truncate_leaves_short_strings_alone() {
        assert_eq!(truncate("short", 20), "short");
        assert_eq!(truncate("exact", 5), "exact");
    }

    #[test]
    fn truncate_cuts_long_strings_with_an_ellipsis() {
        let out = truncate("this line is much too long to fit", 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
        assert_eq!(out, "this line…");
    }

    #[test]
    fn truncate_is_char_not_byte_aware() {
        // "café" has a 2-byte 'é'; truncating to 3 chars must not panic or split it.
        let out = truncate("café bar", 4);
        assert_eq!(out.chars().count(), 4);
    }

    #[test]
    fn render_value_template_keeps_the_value_when_the_template_is_long() {
        let filler = vec!["word"; 300].join(" ");
        let template = format!("{filler} <*> {filler}");
        let out = render_value_template(&template, 300, "TIMEOUT", false);
        assert!(out.contains(" … TIMEOUT … (+"), "{out}");
        assert!(out.ends_with(" more characters)"));
        assert!(out.chars().count() < 500);
    }

    #[test]
    fn render_value_template_swaps_in_the_actual_value() {
        let out = render_value_template("user <*> logged in", 1, "mallory", false);
        assert_eq!(out, "user mallory logged in");
    }
}
