//! Findings that are one event in the log, reported as one.
//!
//! A failing test prints a traceback: eighty lines, nearly every one a template no baseline
//! has. A job that stops early skips its last steps: two hundred templates, every one of them
//! gone. Listed a finding per template, the first is eighty findings and the second two
//! hundred, and the reader has to see for themselves that each is a single thing that
//! happened. A [`Block`] is that single thing: templates of one kind whose lines sit together.
//!
//! **What folds into a block, and what never does.** A template that also turns up on its
//! own somewhere else (a retry message printed five hundred times, one of them next to an
//! error) stays a finding of its own, with its count: a block must not hide how often a line
//! occurs away from it. So a template folds only when every one of its lines is part of
//! such a group. Every template is in at most one block, and a block has at least
//! [`MIN_BLOCK_TEMPLATES`], so grouping never makes a report longer.
//!
//! **NEW blocks** are found while the target is read, which is possible because a target
//! line is known to be new the moment it is seen (its template was created after the last
//! baseline). The reader follows runs of new lines, letting up to [`MAX_GAP`] known lines
//! through (the blank line inside a traceback, a progress line between two of its frames).
//! A run with two or more new templates is a group; the run where a template first appears
//! is the block it belongs to. The same stack trace logged fifty times is one block whose
//! lines are counted, not fifty.
//!
//! **GONE blocks** cannot be found the same way: that a baseline line is gone is known only
//! once the whole target has been read, and the baseline is not kept. What is kept, per
//! template, is its first and last line in the first baseline and how many lines it had
//! there. Templates whose first lines are next to each other are chained, and those that
//! begin and end inside the chain fold into it.
//!
//! Gone chains are then joined more loosely than new lines are. A job that fails skips its
//! remaining steps, and in the baseline each of those steps is its own lines mixed with
//! lines every step prints (the environment dump before a command, a group marker); those
//! are still in the target, printed by the steps that did run. Nobody reads a vanished step
//! line by line: what matters is which part of the run no longer happens. So two chains up
//! to [`MAX_GONE_GAP`] lines apart become one block when at least one line in
//! [`MIN_GONE_DENSITY`] between the start of the first and the end of the second is known
//! to be gone. The density test is what keeps a log with one gone line in every ten from
//! turning into a single block.
//!
//! Both keep a few numbers per template and nothing per line, so memory stays flat.

use std::collections::HashMap;

use serde::Serialize;

use crate::analysis::FindingKind;

/// How many lines that are not part of a run may sit between two that are.
pub const MAX_GAP: usize = 2;

/// How far apart two chains of gone lines may be and still be one block, provided
/// one line in [`MIN_GONE_DENSITY`] of the joined stretch is gone.
pub const MAX_GONE_GAP: usize = 16;

/// A joined stretch of gone chains holds at least one gone line in this many.
pub const MIN_GONE_DENSITY: u64 = 3;

/// The fewest templates a block holds. One template is a finding, not a block.
pub const MIN_BLOCK_TEMPLATES: usize = 2;

/// Templates of one kind that sit together in the log: a traceback, a skipped step.
#[derive(Debug, Clone, Serialize)]
pub struct Block {
    pub kind: FindingKind,
    /// The lines the block spans: in the target for a NEW block, in the first baseline for a
    /// GONE one (the target has no such lines to point at).
    pub first_line_no: usize,
    pub last_line_no: usize,
    /// Lines its templates account for in that run, wherever they are.
    pub line_count: u64,
    /// Of those, the lines outside `first_line_no..=last_line_no`: the later copies of a
    /// stack trace that is logged more than once.
    pub lines_elsewhere: u64,
    /// The sum of its findings' scores.
    pub score: f64,
    /// Indexes into [`crate::analysis::DiffResult::findings`], in line order.
    pub findings: Vec<usize>,
}

/// A block before its templates have been matched to findings: cluster ids, not indexes.
#[derive(Debug, PartialEq)]
pub(crate) struct ProtoBlock {
    pub first: usize,
    pub last: usize,
    pub members: Vec<usize>,
    pub lines_elsewhere: u64,
}

/// A run of new lines that introduced two or more templates.
struct Candidate {
    first: usize,
    last: usize,
    /// (cluster id, its lines in this run) for each template first seen here.
    introduced: Vec<(usize, u64)>,
}

/// Follows the target as it is read and finds the runs of new lines in it.
#[derive(Default)]
pub(crate) struct NewRuns {
    open: bool,
    first: usize,
    last: usize,
    gap: usize,
    /// Lines per new template in the open run.
    counts: HashMap<usize, u64>,
    /// Templates whose first line in the target is in the open run, in line order.
    introduced: Vec<usize>,
    /// Per template, its lines inside runs that held two or more new templates.
    grouped: HashMap<usize, u64>,
    candidates: Vec<Candidate>,
}

impl NewRuns {
    /// The next target line: its cluster, whether that cluster is new, and whether this is
    /// the cluster's first line in the target.
    pub fn line(&mut self, line_no: usize, cid: usize, is_new: bool, first_of_cluster: bool) {
        if !is_new {
            if self.open {
                self.gap += 1;
                if self.gap > MAX_GAP {
                    self.close();
                }
            }
            return;
        }
        if !self.open {
            self.open = true;
            self.first = line_no;
        }
        self.gap = 0;
        self.last = line_no;
        *self.counts.entry(cid).or_insert(0) += 1;
        if first_of_cluster {
            self.introduced.push(cid);
        }
    }

    fn close(&mut self) {
        if self.counts.len() >= MIN_BLOCK_TEMPLATES {
            for (&cid, &lines) in &self.counts {
                *self.grouped.entry(cid).or_insert(0) += lines;
            }
            if self.introduced.len() >= MIN_BLOCK_TEMPLATES {
                self.candidates.push(Candidate {
                    first: self.first,
                    last: self.last,
                    introduced: self
                        .introduced
                        .iter()
                        .map(|cid| (*cid, self.counts[cid]))
                        .collect(),
                });
            }
        }
        self.counts.clear();
        self.introduced.clear();
        self.open = false;
        self.gap = 0;
    }

    /// The blocks, once the target has been read. `target_counts` is every cluster's line
    /// count in the target: a template folds when all of those lines were in groups.
    pub fn finish(mut self, target_counts: &HashMap<usize, u64>) -> Vec<ProtoBlock> {
        if self.open {
            self.close();
        }
        let grouped = &self.grouped;
        let total = |cid: &usize| target_counts.get(cid).copied().unwrap_or(0);
        self.candidates
            .iter()
            .filter_map(|run| {
                let folded: Vec<&(usize, u64)> = run
                    .introduced
                    .iter()
                    .filter(|(cid, _)| grouped.get(cid).copied().unwrap_or(0) == total(cid))
                    .collect();
                (folded.len() >= MIN_BLOCK_TEMPLATES).then(|| ProtoBlock {
                    first: run.first,
                    last: run.last,
                    members: folded.iter().map(|(cid, _)| *cid).collect(),
                    lines_elsewhere: folded.iter().map(|(cid, here)| total(cid) - here).sum(),
                })
            })
            .collect()
    }
}

/// Where a gone template was in the first baseline.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Extent {
    pub cid: usize,
    pub first: usize,
    pub last: usize,
    pub count: u64,
}

impl Extent {
    /// The last line this template is known to cover without a break: its last line when its
    /// lines are packed (a thousand `PASSED` lines in a row), else only its first. A template
    /// seen at line 10 and at line 90,000 says nothing about the lines in between.
    fn packed(&self) -> bool {
        let span = (self.last - self.first + 1) as u64;
        self.count.saturating_mul(2) >= span
    }

    fn covered_to(&self) -> usize {
        if self.packed() {
            self.last
        } else {
            self.first
        }
    }

    /// The lines whose position is known: all of a packed template's, the first of any other.
    fn placed_lines(&self) -> u64 {
        if self.packed() {
            self.count
        } else {
            1
        }
    }
}

/// A stretch of the first baseline: `extents[from..to]`, sorted by first line.
struct Chain {
    from: usize,
    to: usize,
    first: usize,
    covered: usize,
    /// Gone lines known to be inside `first..=covered`.
    placed: u64,
}

/// Chains gone templates whose first lines are within [`MAX_GAP`] lines of what the chain
/// already covers, joins chains that are close and dense enough (see the module
/// documentation), and folds the templates that end inside their chain.
pub(crate) fn gone_blocks(mut extents: Vec<Extent>) -> Vec<ProtoBlock> {
    extents.sort_by_key(|e| (e.first, e.cid));
    let mut chains: Vec<Chain> = Vec::new();
    let mut start = 0;
    while start < extents.len() {
        let mut chain = Chain {
            from: start,
            to: start + 1,
            first: extents[start].first,
            covered: extents[start].covered_to(),
            placed: extents[start].placed_lines(),
        };
        while chain.to < extents.len() && extents[chain.to].first <= chain.covered + MAX_GAP + 1 {
            chain.covered = chain.covered.max(extents[chain.to].covered_to());
            chain.placed += extents[chain.to].placed_lines();
            chain.to += 1;
        }
        start = chain.to;
        chains.push(chain);
        // Joining can make the stretch dense enough to take in the chain before it too.
        while chains.len() >= 2 {
            let (a, b) = (&chains[chains.len() - 2], &chains[chains.len() - 1]);
            let gap = b.first.saturating_sub(a.covered + 1);
            let span = (a.covered.max(b.covered) - a.first + 1) as u64;
            if gap > MAX_GONE_GAP || (a.placed + b.placed) * MIN_GONE_DENSITY < span {
                break;
            }
            let b = chains.pop().expect("two chains");
            let a = chains.last_mut().expect("two chains");
            a.to = b.to;
            a.covered = a.covered.max(b.covered);
            a.placed += b.placed;
        }
    }

    chains
        .iter()
        .filter_map(|chain| {
            let members: Vec<usize> = extents[chain.from..chain.to]
                .iter()
                .filter(|e| e.last <= chain.covered)
                .map(|e| e.cid)
                .collect();
            (members.len() >= MIN_BLOCK_TEMPLATES).then_some(ProtoBlock {
                first: chain.first,
                last: chain.covered,
                members,
                lines_elsewhere: 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds a target described as one entry per line: `Some(cid)` for a new line of that
    /// template, `None` for a line the baselines have.
    fn blocks_of(lines: &[Option<usize>]) -> Vec<ProtoBlock> {
        let mut runs = NewRuns::default();
        let mut counts: HashMap<usize, u64> = HashMap::new();
        for (i, line) in lines.iter().enumerate() {
            match line {
                Some(cid) => {
                    let first = !counts.contains_key(cid);
                    *counts.entry(*cid).or_insert(0) += 1;
                    runs.line(i + 1, *cid, true, first);
                }
                // The cluster id of a known line is never looked at.
                None => runs.line(i + 1, usize::MAX, false, false),
            }
        }
        runs.finish(&counts)
    }

    fn block(first: usize, last: usize, members: &[usize], lines_elsewhere: u64) -> ProtoBlock {
        ProtoBlock {
            first,
            last,
            members: members.to_vec(),
            lines_elsewhere,
        }
    }

    #[test]
    fn consecutive_new_lines_are_one_block() {
        let lines = [None, Some(1), Some(2), Some(3), None, None, None, None];
        assert_eq!(blocks_of(&lines), vec![block(2, 4, &[1, 2, 3], 0)]);
    }

    #[test]
    fn a_run_reaches_across_a_couple_of_known_lines_and_no_further() {
        // A blank line and a progress line inside a traceback do not end it.
        let bridged = [Some(1), None, None, Some(2)];
        assert_eq!(blocks_of(&bridged), vec![block(1, 4, &[1, 2], 0)]);
        let apart = [Some(1), None, None, None, Some(2)];
        assert_eq!(blocks_of(&apart), vec![]);
    }

    #[test]
    fn one_new_line_is_not_a_block_however_often_it_repeats() {
        assert_eq!(blocks_of(&[Some(1), Some(1), Some(1), Some(1)]), vec![]);
    }

    #[test]
    fn a_repeated_template_inside_a_block_is_counted_once() {
        // `E   ...` on six lines of one traceback is one of its templates.
        let lines = [Some(1), Some(2), Some(2), Some(2), Some(3)];
        assert_eq!(blocks_of(&lines), vec![block(1, 5, &[1, 2, 3], 0)]);
    }

    #[test]
    fn the_same_stack_trace_logged_again_is_still_one_block() {
        let mut lines = vec![Some(1), Some(2), Some(3)];
        for _ in 0..4 {
            lines.extend([None, None, None, None, Some(1), Some(2), Some(3)]);
        }
        // Three lines where it is first logged, twelve more in the four later copies.
        assert_eq!(blocks_of(&lines), vec![block(1, 3, &[1, 2, 3], 12)]);
    }

    #[test]
    fn a_template_that_also_occurs_on_its_own_keeps_its_own_finding() {
        // Template 9 is a retry message: next to an error once, alone five times. Folded
        // into the block it would show as one line, and its count would be gone.
        let mut lines = vec![Some(1), Some(2), Some(9)];
        for _ in 0..5 {
            lines.extend([None, None, None, None, Some(9)]);
        }
        assert_eq!(blocks_of(&lines), vec![block(1, 3, &[1, 2], 0)]);
    }

    #[test]
    fn a_block_needs_two_templates_that_fold() {
        // Both templates of the only group also occur alone, so nothing folds and the group
        // is not reported: two findings with their counts say more than an empty block.
        let lines = [
            Some(1),
            Some(2),
            None,
            None,
            None,
            Some(1),
            None,
            None,
            None,
            Some(2),
        ];
        assert_eq!(blocks_of(&lines), vec![]);
    }

    #[test]
    fn a_second_failure_is_a_second_block_and_shares_the_furniture() {
        // Templates 1 and 2 are what every traceback has (a separator, a `def` line); 3 and
        // 4 belong to the first failure, 5 and 6 to the second.
        let lines = [
            Some(1),
            Some(3),
            Some(2),
            Some(4),
            None,
            None,
            None,
            Some(1),
            Some(5),
            Some(2),
            Some(6),
        ];
        assert_eq!(
            blocks_of(&lines),
            vec![block(1, 4, &[1, 3, 2, 4], 2), block(8, 11, &[5, 6], 0)]
        );
    }

    #[test]
    fn every_template_is_in_at_most_one_block() {
        // Pseudo-random targets over a few templates: whatever the arrangement, a template
        // is a member of one block at most and each block has at least two.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..500 {
            let mut lines = Vec::new();
            for _ in 0..60 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                lines.push(match state % 9 {
                    cid @ 0..=5 => Some(cid as usize),
                    _ => None,
                });
            }
            let blocks = blocks_of(&lines);
            let mut seen = std::collections::HashSet::new();
            for b in &blocks {
                assert!(b.members.len() >= MIN_BLOCK_TEMPLATES);
                assert!(b.first < b.last);
                for cid in &b.members {
                    assert!(seen.insert(*cid), "template {cid} is in two blocks");
                }
            }
        }
    }

    fn extent(cid: usize, first: usize, last: usize, count: u64) -> Extent {
        Extent {
            cid,
            first,
            last,
            count,
        }
    }

    #[test]
    fn gone_templates_that_start_next_to_each_other_are_one_block() {
        // A skipped step: each of its lines printed once, one after the other.
        let extents = vec![
            extent(4, 103, 103, 1),
            extent(2, 101, 101, 1),
            extent(3, 102, 102, 1),
            extent(9, 400, 400, 1),
        ];
        assert_eq!(gone_blocks(extents), vec![block(101, 103, &[2, 3, 4], 0)]);
    }

    #[test]
    fn a_packed_template_carries_the_block_to_its_last_line() {
        // "collected", two thousand PASSED lines in a row, then the summary line.
        let extents = vec![
            extent(1, 10, 10, 1),
            extent(2, 11, 2010, 2000),
            extent(3, 2011, 2011, 1),
        ];
        assert_eq!(gone_blocks(extents), vec![block(10, 2011, &[1, 2, 3], 0)]);
    }

    #[test]
    fn a_scattered_template_neither_extends_a_block_nor_folds_into_one() {
        // Template 7 is a heartbeat: first printed between two lines of a step that is gone,
        // then every two hundred lines to the end of the run.
        let extents = vec![
            extent(1, 10, 10, 1),
            extent(7, 11, 90_000, 450),
            extent(2, 12, 12, 1),
            extent(3, 5000, 5000, 1),
        ];
        assert_eq!(gone_blocks(extents), vec![block(10, 12, &[1, 2], 0)]);
    }

    #[test]
    fn skipped_steps_with_shared_lines_between_them_are_one_block() {
        // Three steps of six lines each; between them the eleven lines every step prints,
        // which the target still has. 18 gone lines in a stretch of 40.
        let mut extents = Vec::new();
        for (step, start) in [100, 117, 134].into_iter().enumerate() {
            for line in 0..6 {
                extents.push(extent(step * 10 + line, start + line, start + line, 1));
            }
        }
        let blocks = gone_blocks(extents);
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].first, blocks[0].last), (100, 139));
        assert_eq!(blocks[0].members.len(), 18);
    }

    #[test]
    fn chains_join_only_while_a_third_of_the_stretch_is_gone() {
        // Pairs of gone lines twelve lines apart: each pair is a block, and joined they
        // would be four gone lines in sixteen.
        let extents = vec![
            extent(1, 10, 10, 1),
            extent(2, 11, 11, 1),
            extent(3, 24, 24, 1),
            extent(4, 25, 25, 1),
        ];
        assert_eq!(
            gone_blocks(extents),
            vec![block(10, 11, &[1, 2], 0), block(24, 25, &[3, 4], 0)]
        );
    }

    #[test]
    fn chains_further_apart_than_the_gap_stay_apart_however_dense() {
        // A hundred gone lines, twenty kept ones, a hundred gone lines.
        let extents = vec![
            extent(1, 1, 50, 50),
            extent(2, 51, 100, 50),
            extent(3, 121, 170, 50),
            extent(4, 171, 220, 50),
        ];
        assert_eq!(
            gone_blocks(extents),
            vec![block(1, 100, &[1, 2], 0), block(121, 220, &[3, 4], 0)]
        );
    }

    #[test]
    fn gone_lines_scattered_through_a_run_do_not_become_one_block() {
        // One gone line in every ten, for a thousand lines.
        let extents: Vec<Extent> = (0..100)
            .map(|i| extent(i, 5 + i * 10, 5 + i * 10, 1))
            .collect();
        assert_eq!(gone_blocks(extents), vec![]);
    }

    #[test]
    fn two_gone_templates_far_apart_are_two_findings() {
        let extents = vec![extent(1, 10, 10, 1), extent(2, 500, 500, 1)];
        assert_eq!(gone_blocks(extents), vec![]);
    }
}
