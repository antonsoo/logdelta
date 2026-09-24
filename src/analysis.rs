//! High-level operations built on top of [`crate::mask`], [`crate::drain`] and
//! [`crate::scoring`]: mining a single run's templates, and diffing one or more baseline
//! runs against a target run.

use std::collections::HashMap;
use std::io;

use serde::Serialize;

use crate::context::ContextWindow;
use crate::drain::{Cluster, Drain, DEFAULT_SIMILARITY_THRESHOLD};
use crate::io::read_lines;
use crate::mask::{tokenize_line, CustomMask};
use crate::scoring::{score_template, DEFAULT_SIGNIFICANCE};
use crate::values::ValueTracker;

/// The result of mining a single log source.
#[derive(Serialize)]
pub struct RunSummary {
    pub total_lines: u64,
    pub clusters: Vec<Cluster>,
}

/// Masks and mines every line of `path` on its own, independent [`Drain`] instance. Used by
/// `logdelta templates`.
pub fn mine_run(path: &str, custom: &[CustomMask], threshold: f64) -> io::Result<RunSummary> {
    let mut drain = Drain::new(threshold);
    let mut total = 0u64;
    for line in read_lines(path)? {
        let line = line?;
        total += 1;
        let tokens = tokenize_line(&line, custom);
        drain.add_tokens(tokens, total as usize, &line);
    }
    Ok(RunSummary {
        total_lines: total,
        clusters: drain.into_clusters(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingKind {
    New,
    Gone,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
    Flat,
}

#[derive(Serialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub direction: Direction,
    pub template: String,
    pub score: f64,
    pub baseline_counts: Vec<u64>,
    pub target_count: u64,
    /// 1-based line number of the first line in the target that matched this template.
    pub first_target_line_no: Option<usize>,
    pub first_target_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextWindow>,
}

/// A wildcard position within an otherwise-stable template whose target-run value never
/// appeared in any baseline — the "content flip" case frequency-based scoring alone can't
/// see (see [`crate::values`]). Example: a pytest line's status word going from `PASSED` in
/// every baseline to `FAILED` in the target, with the line's frequency and position
/// otherwise unchanged.
#[derive(Serialize)]
pub struct ValueFinding {
    pub template: String,
    /// Index into `template`'s whitespace-separated tokens of the flipped position.
    pub position: usize,
    pub new_value: String,
    /// Distinct values seen across all baselines at this position, sorted.
    pub baseline_values: Vec<String>,
    pub first_target_line_no: usize,
    pub first_target_raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextWindow>,
}

#[derive(Serialize)]
pub struct DiffResult {
    pub baseline_totals: Vec<u64>,
    pub target_total: u64,
    /// Total distinct templates found across baselines and target combined (not just the
    /// ones with a finding) — the "412 templates" in a summary like "3,012 → 3,104 lines ·
    /// 412 templates · 4 findings".
    pub total_templates: usize,
    pub findings: Vec<Finding>,
    pub value_findings: Vec<ValueFinding>,
}

impl DiffResult {
    /// Attaches `-C` context windows (keyed by first-target-line-number) to each finding.
    pub fn attach_context(&mut self, ctx: &HashMap<usize, ContextWindow>) {
        for f in &mut self.findings {
            if let Some(no) = f.first_target_line_no {
                f.context = ctx.get(&no).cloned();
            }
        }
        for v in &mut self.value_findings {
            v.context = ctx.get(&v.first_target_line_no).cloned();
        }
    }

    /// Every first-target-line-number across all findings, for a single context-collection
    /// pass.
    pub fn wanted_line_numbers(&self) -> std::collections::BTreeSet<usize> {
        self.findings
            .iter()
            .filter_map(|f| f.first_target_line_no)
            .chain(self.value_findings.iter().map(|v| v.first_target_line_no))
            .collect()
    }
}

pub struct DiffOptions {
    pub threshold: f64,
    pub significance: f64,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            threshold: DEFAULT_SIMILARITY_THRESHOLD,
            significance: DEFAULT_SIGNIFICANCE,
        }
    }
}

/// Mines `baselines` and `target` into one shared [`Drain`] instance (baselines first, in
/// order, then the target) so that cluster ids and templates are directly comparable across
/// runs, then scores every template's frequency shift.
pub fn diff_runs(
    baselines: &[&str],
    target: &str,
    custom: &[CustomMask],
    opts: &DiffOptions,
) -> io::Result<DiffResult> {
    let mut drain = Drain::new(opts.threshold);
    let mut tracker = ValueTracker::with_baselines(baselines.len());

    let mut baseline_counts: Vec<HashMap<usize, u64>> = Vec::with_capacity(baselines.len());
    let mut baseline_totals: Vec<u64> = Vec::with_capacity(baselines.len());
    for (baseline_idx, path) in baselines.iter().enumerate() {
        let mut counts: HashMap<usize, u64> = HashMap::new();
        let mut total = 0u64;
        for line in read_lines(path)? {
            let line = line?;
            total += 1;
            let tokens = tokenize_line(&line, custom);
            let cid = drain.add_tokens(tokens.clone(), total as usize, &line);
            tracker.record_baseline(baseline_idx, cid, total as usize, &line, &tokens);
            *counts.entry(cid).or_insert(0) += 1;
        }
        baseline_counts.push(counts);
        baseline_totals.push(total);
    }

    let mut target_counts: HashMap<usize, u64> = HashMap::new();
    let mut target_first: HashMap<usize, (usize, String)> = HashMap::new();
    let mut target_total = 0u64;
    for line in read_lines(target)? {
        let line = line?;
        target_total += 1;
        let tokens = tokenize_line(&line, custom);
        let cid = drain.add_tokens(tokens.clone(), target_total as usize, &line);
        tracker.record_target(cid, target_total as usize, &line, &tokens);
        *target_counts.entry(cid).or_insert(0) += 1;
        target_first
            .entry(cid)
            .or_insert((target_total as usize, line.clone()));
    }

    let n_baselines = baselines.len();
    let total_templates = drain.clusters().len();
    let mut findings = Vec::new();
    let mut value_findings = Vec::new();
    for cluster in drain.clusters() {
        let id = cluster.id;
        let bc: Vec<u64> = baseline_counts
            .iter()
            .map(|m| *m.get(&id).unwrap_or(&0))
            .collect();
        let tc = *target_counts.get(&id).unwrap_or(&0);
        let baseline_sum: u64 = bc.iter().sum();
        let present_in_every_baseline = n_baselines > 0 && bc.iter().all(|&c| c > 0);
        // Computed unconditionally (even for NEW/GONE, which don't need it to be
        // classified) because it's cheap and every finding reports its score either way;
        // simpler than threading an `Option` through and computing it twice.
        let score = score_template(&bc, &baseline_totals, tc, target_total);

        let kind = if tc > 0 && baseline_sum == 0 && n_baselines > 0 {
            Some(FindingKind::New)
        } else if tc == 0 && present_in_every_baseline {
            Some(FindingKind::Gone)
        } else if n_baselines > 0 {
            if score >= opts.significance {
                Some(FindingKind::Changed)
            } else {
                None
            }
        } else {
            // No baseline given at all: everything is trivially "new" to the report.
            Some(FindingKind::New)
        };

        let Some(kind) = kind else { continue };

        let baseline_rate = if baseline_totals.iter().sum::<u64>() > 0 {
            baseline_sum as f64 / baseline_totals.iter().sum::<u64>() as f64
        } else {
            0.0
        };
        let target_rate = if target_total > 0 {
            tc as f64 / target_total as f64
        } else {
            0.0
        };
        let direction = match kind {
            FindingKind::New => Direction::Up,
            FindingKind::Gone => Direction::Down,
            FindingKind::Changed => {
                if target_rate > baseline_rate {
                    Direction::Up
                } else if target_rate < baseline_rate {
                    Direction::Down
                } else {
                    Direction::Flat
                }
            }
        };

        let (line_no, raw) = target_first
            .get(&id)
            .map(|(n, r)| (Some(*n), Some(r.clone())))
            .unwrap_or((None, None));

        findings.push(Finding {
            kind,
            direction,
            template: cluster.template(),
            score,
            baseline_counts: bc,
            target_count: tc,
            first_target_line_no: line_no,
            first_target_raw: raw,
            context: None,
        });
    }

    // NEW VALUE: a wildcard position in a template that's otherwise present in both baseline
    // and target (so NEW/GONE, which are about the *template's* presence, already explain
    // those cases) whose target-run value never appeared in any baseline. Independent of the
    // frequency-based findings above: the whole point is to catch a content flip that a
    // count-based test can't see because the count didn't change.
    if n_baselines > 0 {
        for cluster in drain.clusters() {
            let id = cluster.id;
            let baseline_present = baseline_counts
                .iter()
                .any(|m| m.get(&id).copied().unwrap_or(0) > 0);
            let target_present = target_counts.get(&id).copied().unwrap_or(0) > 0;
            if !baseline_present || !target_present {
                continue;
            }
            for (pos, tok) in cluster.tokens.iter().enumerate() {
                if tok != "<*>" {
                    continue;
                }
                if let Some(found) = tracker.new_value_at(id, pos) {
                    value_findings.push(ValueFinding {
                        template: cluster.template(),
                        position: pos,
                        new_value: found.value,
                        baseline_values: found.baseline_values,
                        first_target_line_no: found.first_target_line_no,
                        first_target_raw: found.first_target_raw,
                        context: None,
                    });
                }
            }
        }
    }

    findings.sort_by(|a, b| {
        kind_rank(a.kind).cmp(&kind_rank(b.kind)).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    value_findings.sort_by(|a, b| {
        a.template
            .cmp(&b.template)
            .then(a.position.cmp(&b.position))
    });

    Ok(DiffResult {
        baseline_totals,
        target_total,
        total_templates,
        findings,
        value_findings,
    })
}

fn kind_rank(k: FindingKind) -> u8 {
    match k {
        FindingKind::New => 0,
        FindingKind::Gone => 1,
        FindingKind::Changed => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn detects_new_template() {
        let good = write_tmp(&["2024-01-01T00:00:00Z start", "2024-01-01T00:00:01Z ok"]);
        let bad = write_tmp(&[
            "2024-01-01T00:00:00Z start",
            "2024-01-01T00:00:01Z ok",
            "2024-01-01T00:00:02Z FATAL: out of memory",
        ]);
        let result = diff_runs(
            &[good.path().to_str().unwrap()],
            bad.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::New && f.template.contains("FATAL")));
    }

    #[test]
    fn detects_gone_template() {
        let good = write_tmp(&["<TS> start", "<TS> shutdown clean"]);
        let bad = write_tmp(&["<TS> start"]);
        let result = diff_runs(
            &[good.path().to_str().unwrap()],
            bad.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::Gone && f.template.contains("shutdown")));
    }

    #[test]
    fn multiple_baselines_suppress_flaky_template() {
        // "retrying" happens in some passing baselines already, at a noisy rate; the same
        // rate in target should NOT be flagged once multiple baselines establish it's normal.
        let b1 = write_tmp(&["a", "a", "a", "retry attempt"]);
        let b2 = write_tmp(&["a", "a", "a", "a"]);
        let b3 = write_tmp(&["a", "a", "retry attempt", "retry attempt"]);
        let target = write_tmp(&["a", "a", "a", "retry attempt"]);
        let result = diff_runs(
            &[
                b1.path().to_str().unwrap(),
                b2.path().to_str().unwrap(),
                b3.path().to_str().unwrap(),
            ],
            target.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!result.findings.iter().any(|f| f.template.contains("retry")));
    }

    #[test]
    fn detects_a_content_flip_frequency_scoring_alone_would_miss() {
        // Same line, same position, same frequency (1 occurrence each run) - only the value
        // at that position changes. A count-based test alone can't see this; it's what
        // ValueFinding (backed by `crate::values`) exists for.
        let good = write_tmp(&["tests/test_math.py::test_divide PASSED [ 57%]"]);
        let bad = write_tmp(&["tests/test_math.py::test_divide FAILED [ 57%]"]);
        let result = diff_runs(
            &[good.path().to_str().unwrap()],
            bad.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            result.value_findings.iter().any(|v| v.new_value == "FAILED"
                && v.baseline_values == vec!["PASSED".to_string()]
                && v.template.contains("test_divide")),
            "expected a NEW VALUE finding for the PASSED->FAILED flip, got: {:?}",
            result
                .value_findings
                .iter()
                .map(|v| (&v.template, &v.new_value))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_value_findings_without_a_baseline() {
        let target = write_tmp(&["status PASSED"]);
        let result = diff_runs(
            &[],
            target.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(result.value_findings.is_empty());
    }

    #[test]
    fn first_target_line_is_recorded() {
        let good = write_tmp(&["ok"]);
        let bad = write_tmp(&["ok", "ok", "boom now"]);
        let result = diff_runs(
            &[good.path().to_str().unwrap()],
            bad.path().to_str().unwrap(),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        let f = result
            .findings
            .iter()
            .find(|f| f.template.contains("boom"))
            .unwrap();
        assert_eq!(f.first_target_line_no, Some(3));
        assert_eq!(f.first_target_raw.as_deref(), Some("boom now"));
    }

    #[test]
    fn mine_run_counts_templates() {
        let f = write_tmp(&["user alice in", "user bob in", "user carol in", "bye"]);
        let summary = mine_run(
            f.path().to_str().unwrap(),
            &[],
            DEFAULT_SIMILARITY_THRESHOLD,
        )
        .unwrap();
        assert_eq!(summary.total_lines, 4);
        let top = summary.clusters.iter().max_by_key(|c| c.count).unwrap();
        assert_eq!(top.count, 3);
        assert_eq!(top.template(), "user <*> in");
    }
}
