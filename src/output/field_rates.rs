use std::io::{self, Write};

use crate::field_rates::{FieldRateGroup, RateGroupStatus};
use crate::fields::WatchedField;

use super::fields::{code, counts, location, terminal_line};

fn label(field: &WatchedField, group: &FieldRateGroup) -> String {
    if field.group_by.is_empty() {
        return "all matched records".into();
    }
    field
        .group_by
        .iter()
        .zip(&group.group_values_json)
        .map(|(pointer, value)| format!("{pointer} = {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn rates(values: &[Option<f64>]) -> String {
    values
        .iter()
        .map(|value| {
            value.map_or_else(
                || "unobserved".into(),
                |rate| format!("{:.4}%", rate * 100.0),
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

fn status(group: &FieldRateGroup) -> &'static str {
    match group.status {
        RateGroupStatus::NoBaselineObservations => "INCOMPLETE RATES: no baseline observations",
        RateGroupStatus::NoTargetObservations => "INCOMPLETE RATES: no target observations",
        RateGroupStatus::Compared if group.changes.is_empty() => {
            "No rate change passed both thresholds"
        }
        RateGroupStatus::Compared => "CHANGED RATES",
    }
}

pub(super) fn human<W: Write>(out: &mut W, field: &WatchedField, width: usize) -> io::Result<()> {
    let Some(comparison) = &field.rate_comparison else {
        return Ok(());
    };
    terminal_line(
        out,
        &format!(
            "  Rate check: >= {} pp outside every baseline rate; score >= {}.",
            comparison.min_change_pp, comparison.significance
        ),
        width,
    )?;
    if !field.complete {
        return terminal_line(
            out,
            "  INCOMPLETE RATES: field coverage cannot supply reliable denominators.",
            width,
        );
    }
    terminal_line(
        out,
        "  Rates are of matched field observations in this group; percentages rounded.",
        width,
    )?;
    for group in &comparison.groups {
        terminal_line(
            out,
            &format!("  {}: {}", status(group), label(field, group)),
            width,
        )?;
        terminal_line(
            out,
            &format!(
                "    Observations: baseline {} | target {}",
                counts(&group.baseline_totals),
                group.target_total
            ),
            width,
        )?;
        for change in &group.changes {
            let value = &field.values[change.value_index];
            terminal_line(
                out,
                &format!(
                    "    {}: baseline {} -> target {:.4}%",
                    value.value_json,
                    rates(&change.baseline_rates),
                    change.target_rate * 100.0
                ),
                width,
            )?;
            terminal_line(
                out,
                &format!(
                    "      {:+.4} pp beyond baseline range; score {:.2}",
                    change.range_distance_pp, change.score
                ),
                width,
            )?;
            if let Some(at) = value
                .first_target
                .as_ref()
                .or(value.first_baseline.as_ref())
            {
                terminal_line(
                    out,
                    &format!("      First example {}: {}", location(at), at.raw),
                    width,
                )?;
                if let Some(context) = &value.context {
                    for (no, text) in &context.before {
                        terminal_line(out, &format!("      {no:>6} | {text}"), width)?;
                    }
                    terminal_line(out, &format!("      {:>6} > {}", at.line_no, at.raw), width)?;
                    for (no, text) in &context.after {
                        terminal_line(out, &format!("      {no:>6} | {text}"), width)?;
                    }
                    if value.context_truncated {
                        terminal_line(
                            out,
                            "      Context clipped; open the original log for more.",
                            width,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn markdown<W: Write>(out: &mut W, field: &WatchedField) -> io::Result<()> {
    let Some(comparison) = &field.rate_comparison else {
        return Ok(());
    };
    writeln!(out, "Rate check: at least {} percentage points outside every observed baseline rate, and score at least {}. Percentages are rounded.\n",
        comparison.min_change_pp, comparison.significance)?;
    if !field.complete {
        return writeln!(
            out,
            "**INCOMPLETE RATES:** field coverage cannot supply reliable denominators.\n"
        );
    }
    for group in &comparison.groups {
        writeln!(
            out,
            "**{}**: {}\n",
            status(group),
            code(&label(field, group))
        )?;
        writeln!(
            out,
            "Group observations: baseline {}; target {}.\n",
            counts(&group.baseline_totals),
            group.target_total
        )?;
        if group.changes.is_empty() {
            continue;
        }
        writeln!(out, "| Value (JSON) | Baseline rates | Target rate | Beyond baseline range (pp) | Score |\n|---|---|---:|---:|---:|")?;
        for change in &group.changes {
            let value = &field.values[change.value_index];
            writeln!(
                out,
                "| {} | {} | {:.4}% | {:+.4} | {:.2} |",
                code(&value.value_json),
                rates(&change.baseline_rates),
                change.target_rate * 100.0,
                change.range_distance_pp,
                change.score
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}
