//! High-level operations built on top of [`crate::mask`], [`crate::drain`] and
//! [`crate::scoring`]: mining a single run's templates, and diffing one or more baseline
//! runs against a target run.

use std::collections::HashMap;
use std::io;

use serde::Serialize;

use crate::context::ContextWindow;
use crate::drain::{Cluster, Drain, DEFAULT_SIMILARITY_THRESHOLD};
use crate::io::read_lines;
use crate::mask::{mask_line, CustomMask};
use crate::scoring::{score_template, DEFAULT_SIGNIFICANCE};

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
        let masked = mask_line(&line, custom);
        drain.add_line(&masked, total as usize, &line);
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

#[derive(Serialize)]
pub struct DiffResult {
    pub baseline_totals: Vec<u64>,
    pub target_total: u64,
    pub findings: Vec<Finding>,
}

impl DiffResult {
    /// Attaches `-C` context windows (keyed by `first_target_line_no`) to each finding.
    pub fn attach_context(&mut self, ctx: &HashMap<usize, ContextWindow>) {
        for f in &mut self.findings {
            if let Some(no) = f.first_target_line_no {
                f.context = ctx.get(&no).cloned();
            }
        }
    }

    /// Every `first_target_line_no` across all findings, for a single context-collection pass.
    pub fn wanted_line_numbers(&self) -> std::collections::BTreeSet<usize> {
        self.findings
            .iter()
            .filter_map(|f| f.first_target_line_no)
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

    let mut baseline_counts: Vec<HashMap<usize, u64>> = Vec::with_capacity(baselines.len());
    let mut baseline_totals: Vec<u64> = Vec::with_capacity(baselines.len());
    for path in baselines {
        let mut counts: HashMap<usize, u64> = HashMap::new();
        let mut total = 0u64;
        for line in read_lines(path)? {
            let line = line?;
            total += 1;
            let masked = mask_line(&line, custom);
            let cid = drain.add_line(&masked, total as usize, &line);
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
        let masked = mask_line(&line, custom);
        let cid = drain.add_line(&masked, target_total as usize, &line);
        *target_counts.entry(cid).or_insert(0) += 1;
        target_first
            .entry(cid)
            .or_insert((target_total as usize, line.clone()));
    }

    let n_baselines = baselines.len();
    let mut findings = Vec::new();
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

    findings.sort_by(|a, b| {
        kind_rank(a.kind).cmp(&kind_rank(b.kind)).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    Ok(DiffResult {
        baseline_totals,
        target_total,
        findings,
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
