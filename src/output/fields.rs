use std::io::{self, Write};

use super::{clip, printable, MAX_SHOWN_CHARS};
use crate::fields::{FieldCoverage, FieldOccurrence, WatchedField};

fn counts(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

fn coverage(c: &FieldCoverage) -> String {
    let mut parts = vec![format!("{} matched", c.matched)];
    for (n, label) in [
        (c.missing, "absent"),
        (c.non_json, "non-JSON"),
        (c.invalid_json, "invalid JSON"),
        (c.non_scalar, "non-scalar"),
        (c.ambiguous, "ambiguous"),
        (c.oversized_records, "oversized"),
        (c.untracked, "untracked"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    parts.join("; ")
}

fn location(at: &FieldOccurrence) -> String {
    let source = at
        .baseline_index
        .map_or_else(|| "target".into(), |n| format!("baseline {}", n + 1));
    format!(
        "{source}:{}{}",
        at.line_no,
        if at.truncated {
            " (excerpt clipped)"
        } else {
            ""
        }
    )
}

/// Clip presentation to the terminal; JSON retains the recorded value and excerpt.
fn terminal_line<W: Write>(out: &mut W, text: &str, width: usize) -> io::Result<()> {
    let text = printable(text);
    if text.chars().count() > width {
        writeln!(
            out,
            "{}...",
            text.chars()
                .take(width.saturating_sub(3))
                .collect::<String>()
        )
    } else {
        writeln!(out, "{text}")
    }
}

pub fn human<W: Write>(out: &mut W, fields: &[WatchedField], width: usize) -> io::Result<()> {
    if fields.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "\nWatched JSON fields (before masking; pooled across records)"
    )?;
    writeln!(
        out,
        "Long text is clipped; --json retains recorded evidence."
    )?;
    for field in fields {
        writeln!(out)?;
        terminal_line(
            out,
            &format!(
                "{}  {}",
                if field.complete {
                    "WATCHED"
                } else {
                    "INCOMPLETE"
                },
                field.pointer
            ),
            width,
        )?;
        if !field.complete {
            writeln!(
                out,
                "  Need a scalar in every run, with no unassessed records."
            )?;
        }
        for (index, c) in field.baselines.iter().enumerate() {
            terminal_line(
                out,
                &format!("  Baseline {}: {}", index + 1, coverage(c)),
                width,
            )?;
        }
        terminal_line(
            out,
            &format!("  Target:     {}", coverage(&field.target)),
            width,
        )?;
        writeln!(out, "  Baseline | Target | Observation | Value (JSON)")?;
        for value in &field.values {
            let marker = match value.is_new {
                Some(true) => "NEW FIELD",
                None => "UNKNOWN",
                _ => "seen",
            };
            terminal_line(
                out,
                &format!(
                    "  {} | {} | {marker} | {}",
                    counts(&value.baseline_counts),
                    value.target_count,
                    value.value_json
                ),
                width,
            )?;
            if value.is_new == Some(true) {
                if let Some(at) = &value.first_target {
                    terminal_line(out, &format!("    {}: {}", location(at), at.raw), width)?;
                    if let Some(ctx) = &value.context {
                        for (no, line) in &ctx.before {
                            terminal_line(out, &format!("    {no:>6} | {line}"), width)?;
                        }
                        terminal_line(out, &format!("    {:>6} > {}", at.line_no, at.raw), width)?;
                        for (no, line) in &ctx.after {
                            terminal_line(out, &format!("    {no:>6} | {line}"), width)?;
                        }
                        if value.context_truncated {
                            terminal_line(
                                out,
                                "    Context clipped; open the original log for more.",
                                width,
                            )?;
                        }
                    }
                }
            }
        }
        for c in field.baselines.iter().chain(std::iter::once(&field.target)) {
            if let Some(at) = &c.first_problem {
                terminal_line(
                    out,
                    &format!("  First problem at {}: {}", location(at), at.raw),
                    width,
                )?;
            }
        }
    }
    Ok(())
}

/// HTML code spans keep backticks, links, angle brackets and table pipes inert.
fn code(text: &str) -> String {
    let escaped = clip(&printable(text), MAX_SHOWN_CHARS)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('`', "&#96;");
    format!("<code>{escaped}</code>")
}

pub fn markdown<W: Write>(out: &mut W, fields: &[WatchedField]) -> io::Result<()> {
    if fields.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n#### Watched JSON fields\n\nCompared before masking, pooled across records. Values retain JSON types and number spelling.\n")?;
    for field in fields {
        writeln!(
            out,
            "**{}** {}\n",
            if field.complete {
                "Compared"
            } else {
                "INCOMPLETE"
            },
            code(&field.pointer)
        )?;
        if !field.complete {
            writeln!(out, "Need a scalar in every run and no invalid, ambiguous, non-scalar or untracked records. Novelty is unknown.\n")?;
        }
        writeln!(out, "| Run | Coverage |\n|---|---|")?;
        for (index, c) in field.baselines.iter().enumerate() {
            writeln!(out, "| Baseline {} | {} |", index + 1, coverage(c))?;
        }
        writeln!(out, "| Target | {} |\n", coverage(&field.target))?;
        writeln!(
            out,
            "| Baseline counts | Target count | Value (JSON) | Observation |\n|---|---:|---|---|"
        )?;
        for value in &field.values {
            let marker = match value.is_new {
                Some(true) => "NEW FIELD VALUE",
                None => "Unknown",
                _ => "Seen in baseline",
            };
            writeln!(
                out,
                "| {} | {} | {} | {marker} |",
                counts(&value.baseline_counts),
                value.target_count,
                code(&value.value_json)
            )?;
        }
        writeln!(out)?;
        for value in field.new_values() {
            if let Some(at) = &value.first_target {
                writeln!(
                    out,
                    "{} at {}: {}\n",
                    code(&value.value_json),
                    code(&location(at)),
                    code(&at.raw)
                )?;
            }
        }
        for c in field.baselines.iter().chain(std::iter::once(&field.target)) {
            if let Some(at) = &c.first_problem {
                writeln!(
                    out,
                    "First problem at {}: {}\n",
                    code(&location(at)),
                    code(&at.raw)
                )?;
            }
        }
    }
    Ok(())
}
