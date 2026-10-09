use std::io::{self, Write};

use super::{clip, printable, MAX_SHOWN_CHARS};
use crate::fields::{FieldCoverage, FieldOccurrence, FieldValue, WatchedField};

pub(super) fn counts(values: &[u64]) -> String {
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
        (c.group_missing, "missing group key"),
        (c.group_non_scalar, "non-scalar group key"),
        (c.group_ambiguous, "ambiguous group key"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    parts.join("; ")
}

fn group_label(field: &WatchedField, value: &FieldValue) -> String {
    field
        .group_by
        .iter()
        .zip(&value.group_values_json)
        .map(|(pointer, value)| format!("{pointer} = {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn location(at: &FieldOccurrence) -> String {
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
pub(super) fn terminal_line<W: Write>(out: &mut W, text: &str, width: usize) -> io::Result<()> {
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
        "\nWatched JSON fields (before masking; {})",
        if fields.iter().any(|f| !f.group_by.is_empty()) {
            "within exact groups"
        } else {
            "pooled across records"
        }
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
                "  Need a scalar in every run, with no unassessed records or group keys."
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
        if field.group_by.is_empty() {
            writeln!(out, "  Baseline | Target | Observation | Value (JSON)")?;
        } else {
            terminal_line(
                out,
                &format!("  Grouped by: {}", field.group_by.join(", ")),
                width,
            )?;
        }
        let mut previous_group: Option<&[String]> = None;
        for value in &field.values {
            if !field.group_by.is_empty()
                && previous_group != Some(value.group_values_json.as_slice())
            {
                terminal_line(
                    out,
                    &format!("  Group: {}", group_label(field, value)),
                    width,
                )?;
                writeln!(out, "  Baseline | Target | Observation | Value (JSON)")?;
                previous_group = Some(&value.group_values_json);
            }
            let marker = match value.is_new {
                Some(true) if value.group_seen_in_baseline == Some(false) => "NEW GROUP",
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
        if field
            .values
            .iter()
            .any(|v| v.group_seen_in_baseline == Some(false))
        {
            terminal_line(
                out,
                "  NEW GROUP: no baseline observation for this field and key.",
                width,
            )?;
        }
        super::field_rates::human(out, field, width)?;
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
pub(super) fn code(text: &str) -> String {
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
    writeln!(out, "\n#### Watched JSON fields\n\nCompared before masking, {}. Values retain JSON types and number spelling.\n",
        if fields.iter().any(|f| !f.group_by.is_empty()) { "within exact groups" } else { "pooled across records" })?;
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
            writeln!(out, "Need a scalar in every run and no invalid, ambiguous, non-scalar or untracked records or group keys. Novelty is unknown.\n")?;
        }
        writeln!(out, "| Run | Coverage |\n|---|---|")?;
        for (index, c) in field.baselines.iter().enumerate() {
            writeln!(out, "| Baseline {} | {} |", index + 1, coverage(c))?;
        }
        writeln!(out, "| Target | {} |\n", coverage(&field.target))?;
        let groups = field
            .group_by
            .iter()
            .map(|p| format!(" Group: {} |", code(p)))
            .collect::<String>();
        writeln!(out, "|{groups} Baseline counts | Target count | Value (JSON) | Observation |\n|{}---|---:|---|---|", "---|".repeat(field.group_by.len()))?;
        for value in &field.values {
            let marker = match value.is_new {
                Some(true) if value.group_seen_in_baseline == Some(false) => "NEW GROUP",
                Some(true) => "NEW FIELD VALUE",
                None => "Unknown",
                _ => "Seen in baseline",
            };
            let groups = value
                .group_values_json
                .iter()
                .map(|v| format!(" {} |", code(v)))
                .collect::<String>();
            writeln!(
                out,
                "|{groups} {} | {} | {} | {marker} |",
                counts(&value.baseline_counts),
                value.target_count,
                code(&value.value_json)
            )?;
        }
        writeln!(out)?;
        if field
            .values
            .iter()
            .any(|v| v.group_seen_in_baseline == Some(false))
        {
            writeln!(out, "NEW GROUP means no baseline observation for this field and key; it does not establish a changed outcome within an observed group.\n")?;
        }
        super::field_rates::markdown(out, field)?;
        for value in field.finding_values() {
            if let Some(at) = value
                .first_target
                .as_ref()
                .or(value.first_baseline.as_ref())
            {
                if !field.group_by.is_empty() {
                    writeln!(out, "Group {}: ", code(&group_label(field, value)))?;
                }
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
