//! High-level operations built on top of [`crate::mask`], [`crate::drain`] and
//! [`crate::scoring`]: mining a single run's templates, and diffing one or more baseline
//! runs against a target run.

use std::collections::HashMap;
use std::io;

use serde::Serialize;

use crate::blocks::{gone_blocks, Block, Extent, NewRuns, ProtoBlock};
use crate::context::ContextWindow;
use crate::drain::{is_wildcard, Cluster, Drain, DEFAULT_SIMILARITY_THRESHOLD};
use crate::fields::{FieldTracker, WatchedField};
use crate::io::read_lines;
use crate::mask::{tokenize_line, CustomMask};
use crate::scoring::{count_g_test, score_template, DEFAULT_SIGNIFICANCE};
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
    mine_lines(read_lines(path)?, custom, threshold)
}

/// [`mine_run`] over any source of lines (a file, stdin, or text already in memory).
pub fn mine_lines<I>(lines: I, custom: &[CustomMask], threshold: f64) -> io::Result<RunSummary>
where
    I: Iterator<Item = io::Result<String>>,
{
    let mut drain = Drain::new(threshold);
    let mut total = 0u64;
    for line in lines {
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
    /// For a GONE finding, the line the target no longer has: the template's first line in
    /// the first baseline, by 1-based number and as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_baseline_line_no: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_baseline_raw: Option<String>,
    /// Index into [`DiffResult::blocks`] when this finding is one line of a larger event (a
    /// traceback, a skipped step) that reports show as a whole. See [`crate::blocks`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<usize>,
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
    /// `baseline occurrences / distinct baseline values` at this position — how established
    /// the baseline side is. Findings are sorted by this, most established first, so the
    /// most-confident content flips (a status seen hundreds of times, now different) lead.
    pub established: f64,
    /// Index into [`DiffResult::blocks`] when the line is inside a NEW block: a line of a
    /// traceback that happens to fit a template the baselines have. Reports show the block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<usize>,
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
    /// One entry per template that differs. A finding with a `block` is part of that block.
    pub findings: Vec<Finding>,
    /// Findings that are one event in the log, grouped: see [`crate::blocks`]. Empty when
    /// [`DiffOptions::group`] is off.
    pub blocks: Vec<Block>,
    pub value_findings: Vec<ValueFinding>,
    /// Explicit JSON scalar comparisons before masking. Absent unless fields were watched.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub watched_fields: Vec<WatchedField>,
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
        for field in &mut self.watched_fields {
            for value in &mut field.values {
                if value.is_new == Some(true) {
                    if let Some(context) = value
                        .first_target
                        .as_ref()
                        .and_then(|at| ctx.get(&at.line_no))
                    {
                        value.attach_context(context);
                    }
                }
            }
        }
    }

    /// Every first-target-line-number a report shows context around, for a single
    /// context-collection pass. A finding inside a block is shown with the block's own
    /// lines around it, so it asks for none.
    pub fn wanted_line_numbers(&self) -> std::collections::BTreeSet<usize> {
        self.findings
            .iter()
            .filter(|f| f.block.is_none())
            .filter_map(|f| f.first_target_line_no)
            .chain(self.ungrouped_values().map(|v| v.first_target_line_no))
            .chain(
                self.watched_fields
                    .iter()
                    .flat_map(|f| f.new_values())
                    .filter_map(|v| v.first_target.as_ref().map(|at| at.line_no)),
            )
            .collect()
    }

    /// The findings a report lists on their own: those that are not part of a block.
    pub fn ungrouped_findings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.block.is_none())
    }

    /// The new values a report lists on their own: those not on a line inside a block.
    pub fn ungrouped_values(&self) -> impl Iterator<Item = &ValueFinding> {
        self.value_findings.iter().filter(|v| v.block.is_none())
    }

    /// How many things a report has to show: blocks, and the findings and new values outside
    /// any block. Equal to the number of findings when nothing was grouped.
    pub fn report_count(&self) -> usize {
        self.blocks.len()
            + self.ungrouped_findings().count()
            + self.ungrouped_values().count()
            + self.field_finding_count()
    }

    /// How many findings there are before grouping: one per template that differs, and one
    /// per new value.
    pub fn finding_count(&self) -> usize {
        self.findings.len() + self.value_findings.len() + self.field_finding_count()
    }

    pub fn field_finding_count(&self) -> usize {
        self.watched_fields
            .iter()
            .map(|f| f.new_values().count())
            .sum()
    }

    /// An incomplete field watch must not pass a CI gate, even with no findings.
    pub fn complete(&self) -> bool {
        self.watched_fields.iter().all(|field| field.complete)
    }
}

pub struct DiffOptions {
    pub threshold: f64,
    pub significance: f64,
    /// Group findings whose lines sit together into [`Block`]s (on by default).
    pub group: bool,
    /// JSON Pointers selecting scalar values to compare exactly, before masking.
    pub watch_fields: Vec<String>,
    /// JSON Pointers forming an exact group key for every watched field. Empty pools records.
    pub watch_by: Vec<String>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            threshold: DEFAULT_SIMILARITY_THRESHOLD,
            significance: DEFAULT_SIGNIFICANCE,
            group: true,
            watch_fields: Vec::new(),
            watch_by: Vec::new(),
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
    let baseline_lines = baselines
        .iter()
        .map(|path| read_lines(path))
        .collect::<io::Result<Vec<_>>>()?;
    diff_lines(baseline_lines, read_lines(target)?, custom, opts)
}

/// [`diff_runs`] over any sources of lines: files, stdin, or text already in memory (the
/// browser build diffs pasted logs this way).
pub fn diff_lines<B, T>(
    baselines: Vec<B>,
    target: T,
    custom: &[CustomMask],
    opts: &DiffOptions,
) -> io::Result<DiffResult>
where
    B: Iterator<Item = io::Result<String>>,
    T: Iterator<Item = io::Result<String>>,
{
    let n_baselines = baselines.len();
    let mut fields = FieldTracker::new(&opts.watch_fields, &opts.watch_by, n_baselines)?;
    let mut drain = Drain::new(opts.threshold);
    let mut tracker = ValueTracker::with_baselines(n_baselines);

    // Without a baseline every line is new and there is nothing to set a block apart from.
    let group = opts.group && n_baselines > 0;

    let mut baseline_counts: Vec<HashMap<usize, u64>> = Vec::with_capacity(n_baselines);
    let mut baseline_totals: Vec<u64> = Vec::with_capacity(n_baselines);
    // Each template's last line in the first baseline (its first is the cluster's own).
    let mut first_baseline_last: HashMap<usize, usize> = HashMap::new();
    for (baseline_idx, lines) in baselines.into_iter().enumerate() {
        let mut counts: HashMap<usize, u64> = HashMap::new();
        let mut total = 0u64;
        for line in lines {
            let line = line?;
            total += 1;
            fields.record(Some(baseline_idx), total as usize, &line);
            let tokens = tokenize_line(&line, custom);
            let cid = drain.add_token_slice(&tokens, total as usize, &line);
            tracker.record_baseline(baseline_idx, cid, total as usize, &line, &tokens);
            *counts.entry(cid).or_insert(0) += 1;
            if group && baseline_idx == 0 {
                first_baseline_last.insert(cid, total as usize);
            }
        }
        baseline_counts.push(counts);
        baseline_totals.push(total);
    }

    // A cluster the target creates has an id past every baseline's: its lines are new.
    let baseline_clusters = drain.clusters().len();
    let mut new_runs = NewRuns::default();
    let mut target_counts: HashMap<usize, u64> = HashMap::new();
    let mut target_first: HashMap<usize, (usize, String)> = HashMap::new();
    let mut target_total = 0u64;
    for line in target {
        let line = line?;
        target_total += 1;
        fields.record(None, target_total as usize, &line);
        let tokens = tokenize_line(&line, custom);
        let cid = drain.add_token_slice(&tokens, target_total as usize, &line);
        tracker.record_target(cid, target_total as usize, &line, &tokens);
        *target_counts.entry(cid).or_insert(0) += 1;
        let first_of_cluster = !target_first.contains_key(&cid);
        if first_of_cluster {
            target_first.insert(cid, (target_total as usize, line.clone()));
        }
        if group {
            let is_new = cid >= baseline_clusters;
            new_runs.line(target_total as usize, cid, is_new, first_of_cluster);
        }
    }

    let total_templates = drain.clusters().len();
    // Each finding with the id of its cluster, which is how a block names its members.
    let mut findings: Vec<(usize, Finding)> = Vec::new();
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
        let share_direction = if target_rate > baseline_rate {
            Direction::Up
        } else if target_rate < baseline_rate {
            Direction::Down
        } else {
            Direction::Flat
        };
        // Per run: the target's count against the mean of the baselines'.
        let count_direction = match (tc * n_baselines as u64).cmp(&baseline_sum) {
            std::cmp::Ordering::Greater => Direction::Up,
            std::cmp::Ordering::Less => Direction::Down,
            std::cmp::Ordering::Equal => Direction::Flat,
        };

        let kind = if tc > 0 && baseline_sum == 0 && n_baselines > 0 {
            Some(FindingKind::New)
        } else if tc == 0 && present_in_every_baseline {
            Some(FindingKind::Gone)
        } else if n_baselines > 0 {
            // Changed: its share of the log moved, and so did its count, the same way. The
            // share alone moves whenever the run is shorter or longer for other reasons (a
            // job that fails early prints its setup lines as often as ever, in a shorter
            // log); the count alone moves whenever the whole run is bigger with the same mix.
            if score >= opts.significance
                && count_g_test(&bc, tc) >= opts.significance
                && share_direction == count_direction
            {
                Some(FindingKind::Changed)
            } else {
                None
            }
        } else {
            // No baseline given at all: everything is trivially "new" to the report.
            Some(FindingKind::New)
        };

        let Some(kind) = kind else { continue };

        let direction = match kind {
            FindingKind::New => Direction::Up,
            FindingKind::Gone => Direction::Down,
            FindingKind::Changed => share_direction,
        };

        let (line_no, raw) = target_first
            .get(&id)
            .map(|(n, r)| (Some(*n), Some(r.clone())))
            .unwrap_or((None, None));

        // A gone template was in every baseline, so it was first seen in the first one and
        // the cluster's own first line is a line of that run.
        let (baseline_line_no, baseline_raw) = if kind == FindingKind::Gone {
            (
                Some(cluster.first_line_no),
                Some(cluster.first_line_raw.clone()),
            )
        } else {
            (None, None)
        };

        findings.push((
            id,
            Finding {
                kind,
                direction,
                template: cluster.template(),
                score,
                baseline_counts: bc,
                target_count: tc,
                first_target_line_no: line_no,
                first_target_raw: raw,
                first_baseline_line_no: baseline_line_no,
                first_baseline_raw: baseline_raw,
                block: None,
                context: None,
            },
        ));
    }

    let mut gone_values: Vec<Finding> = Vec::new();
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
                if !is_wildcard(tok) {
                    continue;
                }
                // GONE, for one value of the template rather than the template: the mined
                // template is `<*> health check ok` for every instance of a service, and
                // one instance that stops is not the template going away.
                for missing in tracker.gone_values_at(id, pos) {
                    let mut tokens = cluster.tokens.clone();
                    tokens[pos] = missing.value;
                    gone_values.push(Finding {
                        kind: FindingKind::Gone,
                        direction: Direction::Down,
                        template: tokens.join(" "),
                        score: score_template(
                            &missing.baseline_counts,
                            &baseline_totals,
                            0,
                            target_total,
                        ),
                        baseline_counts: missing.baseline_counts,
                        target_count: 0,
                        first_target_line_no: None,
                        first_target_raw: None,
                        first_baseline_line_no: Some(missing.first_baseline_line_no),
                        first_baseline_raw: Some(missing.first_baseline_raw),
                        block: None,
                        context: None,
                    });
                }
                if let Some(found) = tracker.new_value_at(id, pos) {
                    value_findings.push(ValueFinding {
                        template: cluster.template(),
                        // As the template reads when split at spaces: a bracketed field
                        // before this position is one token with several words in it.
                        position: cluster.tokens[..pos]
                            .iter()
                            .map(|t| t.split(' ').count())
                            .sum(),
                        new_value: found.value,
                        baseline_values: found.baseline_values,
                        first_target_line_no: found.first_target_line_no,
                        first_target_raw: found.first_target_raw,
                        established: found.established,
                        block: None,
                        context: None,
                    });
                }
            }
        }
    }

    // These have no cluster of their own; an id no cluster has keeps them out of the blocks.
    findings.extend(
        gone_values
            .into_iter()
            .enumerate()
            .map(|(n, finding)| (usize::MAX - n, finding)),
    );

    findings.sort_by(|(_, a), (_, b)| {
        kind_rank(a.kind).cmp(&kind_rank(b.kind)).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let (cluster_ids, mut findings): (Vec<usize>, Vec<Finding>) = findings.into_iter().unzip();

    let mut blocks = Vec::new();
    if group {
        let index_of: HashMap<usize, usize> = cluster_ids
            .iter()
            .enumerate()
            .map(|(index, cid)| (*cid, index))
            .collect();
        let gone = findings
            .iter()
            .zip(&cluster_ids)
            .filter(|(f, _)| f.kind == FindingKind::Gone)
            .filter_map(|(f, cid)| {
                Some(Extent {
                    cid: *cid,
                    first: f.first_baseline_line_no?,
                    last: *first_baseline_last.get(cid)?,
                    count: f.baseline_counts.first().copied()?,
                })
            })
            .collect();
        let found = [
            (FindingKind::New, new_runs.finish(&target_counts)),
            (FindingKind::Gone, gone_blocks(gone)),
        ];
        for (kind, protos) in found {
            for proto in protos {
                if let Some(block) = build_block(kind, &proto, &index_of, &findings) {
                    blocks.push(block);
                }
            }
        }
        blocks.sort_by(|a, b| {
            kind_rank(a.kind)
                .cmp(&kind_rank(b.kind))
                .then(
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.first_line_no.cmp(&b.first_line_no))
        });
        for (index, block) in blocks.iter().enumerate() {
            for &member in &block.findings {
                findings[member].block = Some(index);
            }
        }
    }
    // Most-established baseline value first: a status seen hundreds of times that just
    // changed is a stronger signal than one that barely cleared the repetition bar.
    value_findings.sort_by(|a, b| {
        b.established
            .partial_cmp(&a.established)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.template.cmp(&b.template))
            .then(a.position.cmp(&b.position))
    });

    for v in &mut value_findings {
        v.block = blocks.iter().position(|b| {
            b.kind == FindingKind::New
                && (b.first_line_no..=b.last_line_no).contains(&v.first_target_line_no)
        });
    }

    Ok(DiffResult {
        baseline_totals,
        target_total,
        total_templates,
        findings,
        blocks,
        value_findings,
        watched_fields: fields.finish(),
    })
}

/// A [`Block`] from the cluster ids of its members. `None` when fewer than two of them are
/// findings of the block's kind, which a block found for that kind never is.
fn build_block(
    kind: FindingKind,
    proto: &ProtoBlock,
    index_of: &HashMap<usize, usize>,
    findings: &[Finding],
) -> Option<Block> {
    let line_no = |f: &Finding| match kind {
        FindingKind::Gone => f.first_baseline_line_no,
        _ => f.first_target_line_no,
    };
    let mut members: Vec<usize> = proto
        .members
        .iter()
        .filter_map(|cid| index_of.get(cid).copied())
        .filter(|&index| findings[index].kind == kind)
        .collect();
    if members.len() < crate::blocks::MIN_BLOCK_TEMPLATES {
        return None;
    }
    members.sort_by_key(|&index| line_no(&findings[index]));
    let lines = |f: &Finding| match kind {
        FindingKind::Gone => f.baseline_counts.first().copied().unwrap_or(0),
        _ => f.target_count,
    };
    Some(Block {
        kind,
        first_line_no: proto.first,
        last_line_no: proto.last,
        line_count: members.iter().map(|&index| lines(&findings[index])).sum(),
        lines_elsewhere: proto.lines_elsewhere,
        score: members.iter().map(|&index| findings[index].score).sum(),
        findings: members,
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
    fn diffing_text_in_memory_matches_diffing_the_same_files() {
        const GOOD: [&str; 3] = [
            "2024-01-01T00:00:00Z start",
            "2024-01-01T00:00:01Z ok 12ms",
            "2024-01-01T00:00:02Z ok 7ms",
        ];
        const BAD: [&str; 3] = [
            "2024-01-01T00:00:00Z start",
            "2024-01-01T00:00:01Z ok 9ms",
            "2024-01-01T00:00:02Z FATAL: out of memory",
        ];
        let (good, bad) = (&GOOD, &BAD);
        let (gf, bf) = (write_tmp(good), write_tmp(bad));
        let opts = DiffOptions::default();
        let from_files = diff_runs(
            &[gf.path().to_str().unwrap()],
            bf.path().to_str().unwrap(),
            &[],
            &opts,
        )
        .unwrap();
        let lines = |ls: &'static [&'static str]| ls.iter().map(|l| Ok(l.to_string()));
        let in_memory = diff_lines(vec![lines(good)], lines(bad), &[], &opts).unwrap();
        assert_eq!(
            serde_json::to_value(&from_files).unwrap(),
            serde_json::to_value(&in_memory).unwrap()
        );
        assert_eq!(in_memory.findings.len(), from_files.findings.len());
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

    fn lines_of(parts: &[(&str, usize)]) -> Vec<String> {
        parts
            .iter()
            .flat_map(|(line, times)| std::iter::repeat_n(line.to_string(), *times))
            .collect()
    }

    fn changed(baselines: &[Vec<String>], target: &[String]) -> Vec<String> {
        let sources: Vec<_> = baselines
            .iter()
            .map(|b| b.iter().cloned().map(Ok))
            .collect();
        let result = diff_lines(
            sources,
            target.iter().cloned().map(Ok),
            &[],
            &DiffOptions::default(),
        )
        .unwrap();
        result
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::Changed)
            .map(|f| f.template.clone())
            .collect()
    }

    #[test]
    fn a_run_that_stops_early_does_not_change_the_lines_it_did_print() {
        // The job fetches 250 branches, then runs 1,000 steps. The target fails after 100
        // of them: the fetch lines are a much larger share of it and exactly as many.
        let good = lines_of(&[("fetched branch ok", 250), ("step finished fine", 1000)]);
        let bad = lines_of(&[("fetched branch ok", 250), ("step finished fine", 100)]);
        let found = changed(&[good.clone(), good.clone(), good], &bad);
        assert_eq!(found, vec!["step finished fine".to_string()]);
    }

    #[test]
    fn a_bigger_run_with_the_same_mix_changes_nothing() {
        let good = lines_of(&[("request served", 300), ("cache hit", 100), ("retry", 20)]);
        let bad = lines_of(&[("request served", 900), ("cache hit", 300), ("retry", 60)]);
        assert_eq!(changed(&[good.clone(), good], &bad), Vec::<String>::new());
    }

    #[test]
    fn a_line_printed_far_more_often_in_a_run_of_the_same_size_is_changed() {
        let good = lines_of(&[("request served", 400), ("retry", 3)]);
        let bad = lines_of(&[("request served", 363), ("retry", 40)]);
        assert_eq!(
            changed(&[good.clone(), good.clone(), good], &bad),
            vec!["retry".to_string()]
        );
    }

    #[test]
    fn detects_a_content_flip_frequency_scoring_alone_would_miss() {
        // Same line, same position - only the value at that position changes. A count-based
        // test alone can't see this; it's what ValueFinding (backed by `crate::values`)
        // exists for. The baseline repeats PASSED enough times to clear
        // `values::MIN_AVG_REPETITION`, the way a real test suite run would.
        let good = write_tmp(&["tests/test_math.py::test_divide PASSED [ 57%]"; 6]);
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
