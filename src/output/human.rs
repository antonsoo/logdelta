//! Human-readable terminal output: aligned, colour-coded (when enabled) sections for NEW /
//! GONE / CHANGED findings, and a ranked table for `logdelta templates`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};

use crate::analysis::{DiffResult, Finding, FindingKind, RunSummary};

use super::highlight_template;

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
    format!("{on}{text:<7}{off}")
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

pub fn write_diff<W: Write>(
    out: &mut W,
    result: &DiffResult,
    baseline_paths: &[&str],
    target_path: &str,
    use_color: bool,
) -> io::Result<()> {
    writeln!(
        out,
        "logdelta diff  baseline: {} ({} line{})   target: {} ({} line{})",
        baseline_paths.join(", "),
        result.baseline_totals.iter().sum::<u64>(),
        plural(result.baseline_totals.iter().sum::<u64>()),
        target_path,
        result.target_total,
        plural(result.target_total),
    )?;

    if result.findings.is_empty() {
        writeln!(out, "\nNo significant differences found.")?;
        return Ok(());
    }

    for kind in [FindingKind::New, FindingKind::Changed, FindingKind::Gone] {
        let items: Vec<&Finding> = result.findings.iter().filter(|f| f.kind == kind).collect();
        if items.is_empty() {
            continue;
        }
        writeln!(out)?;
        for f in items {
            writeln!(
                out,
                "{}  {}  score={:.1}  {}",
                kind_label(f.kind, use_color),
                format_counts(f),
                f.score,
                highlight_template(&f.template, use_color),
            )?;
            if let (Some(no), Some(raw)) = (f.first_target_line_no, &f.first_target_raw) {
                writeln!(
                    out,
                    "        {} {}:{}: {}",
                    dim("first seen at", use_color),
                    target_path,
                    no,
                    raw
                )?;
            }
            if let Some(ctx) = &f.context {
                for (n, raw) in &ctx.before {
                    writeln!(
                        out,
                        "        {}",
                        dim(&format!("{n:>6} | {raw}"), use_color)
                    )?;
                }
                if let (Some(no), Some(raw)) = (f.first_target_line_no, &f.first_target_raw) {
                    writeln!(out, "        {no:>6} > {raw}")?;
                }
                for (n, raw) in &ctx.after {
                    writeln!(
                        out,
                        "        {}",
                        dim(&format!("{n:>6} | {raw}"), use_color)
                    )?;
                }
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
        summary.total_lines,
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
            highlight_template(&c.template(), use_color),
            width = width
        )?;
        writeln!(
            out,
            "{:width$}  {} {}:{}",
            "",
            dim("e.g.", use_color),
            c.first_line_no,
            c.first_line_raw,
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
