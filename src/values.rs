//! Tracks the distinct literal values seen at each (cluster, token-position) pair, per
//! baseline run and in the target, so `logdelta diff` can flag a value that never appeared in
//! any baseline at an otherwise low-cardinality position — the "content flip" case pure
//! frequency scoring can't see. The canonical example: a pytest line's status word flipping
//! from `PASSED` to `FAILED` while the line's overall frequency and position stay identical,
//! so nothing in [`crate::scoring`]'s count-based G-test ever moves.
//!
//! Only positions Drain ends up wildcarding are meaningful here (a literal position is, by
//! definition, identical across every line in the cluster), but this module doesn't need to
//! know which positions those are: it just needs a bounded amount of memory regardless of how
//! many lines it sees, which the `MAX_TRACKED_VALUES` cap on distinct values per position
//! already provides. That cap also *is* the "at most ~10 distinct values" / "not an id-like
//! token" requirement: an id-like value (a path, a free-running counter) blows straight
//! through it and the position is marked "too varied to say anything about" instead of
//! reported.

use std::collections::{HashMap, HashSet};

use crate::mask::is_placeholder;

const MAX_TRACKED_VALUES: usize = 10;
/// A position is only "categorical" enough to report if its known baseline values recur at
/// least this many times on average (`total occurrences / distinct values`). `PASSED` /
/// `FAILED` / `SKIPPED` clear this easily; a worker id or a test name — each appearing once
/// or twice per run — doesn't, which is exactly the point: those aren't statuses, they're
/// identifiers that happen to vary, and flagging one as "new" on a clean pass-vs-pass diff is
/// noise, not a finding.
const MIN_AVG_REPETITION: f64 = 5.0;

/// True if `value` looks like an identifier rather than a status word: contains a digit (a
/// worker id like `gw3`, a shard like `node-7`), or a path/namespace separator (`::`, `/`,
/// `.`, as in a test id or file path). Such values are never tracked at all, so a position
/// that's *entirely* identifiers (e.g. "which test ran") never accumulates anything to
/// report, without needing to special-case "this is an identifier position" — one value at a
/// time is enough.
fn looks_identifier_like(value: &str) -> bool {
    value.contains(|c: char| c.is_ascii_digit())
        || value.contains("::")
        || value.contains('/')
        || value.contains('.')
}

#[derive(Clone)]
struct ValueEntry {
    count: u64,
    first_line_no: usize,
    first_raw: String,
}

#[derive(Default)]
struct PositionValues {
    values: HashMap<String, ValueEntry>,
    /// Set once more than `MAX_TRACKED_VALUES` distinct values have been seen; from then on
    /// this position is treated as high-cardinality and ineligible for a NEW VALUE finding.
    overflowed: bool,
}

impl PositionValues {
    fn record(&mut self, value: &str, line_no: usize, raw: &str) {
        if self.overflowed {
            return;
        }
        if let Some(e) = self.values.get_mut(value) {
            e.count += 1;
            return;
        }
        if self.values.len() >= MAX_TRACKED_VALUES {
            self.overflowed = true;
            self.values.clear();
            return;
        }
        self.values.insert(
            value.to_string(),
            ValueEntry {
                count: 1,
                first_line_no: line_no,
                first_raw: raw.to_string(),
            },
        );
    }
}

/// What [`ValueTracker::new_value_at`] found.
pub struct NewValue {
    pub value: String,
    /// Distinct values seen across all baselines at this position, sorted.
    pub baseline_values: Vec<String>,
    pub first_target_line_no: usize,
    pub first_target_raw: String,
    /// `baseline occurrences / distinct baseline values` — how established the baseline side
    /// is. Always `>= MIN_AVG_REPETITION`; higher means more confidence this position is a
    /// real, recurring status rather than something that happened to squeak under the
    /// cardinality cap. Findings are ranked by this, most established first.
    pub established: f64,
}

/// Per-(cluster id, token position) value tracking, kept separately per baseline run and for
/// the target so a "new" value can be checked against the union of everything seen across
/// every baseline (and so a value that's already flaky — present in some baselines but not
/// others — doesn't get reported: it's already visible in `baseline_values` as "we've seen
/// this vary before").
pub struct ValueTracker {
    baselines: Vec<HashMap<(usize, usize), PositionValues>>,
    target: HashMap<(usize, usize), PositionValues>,
}

impl ValueTracker {
    pub fn with_baselines(n: usize) -> Self {
        ValueTracker {
            baselines: (0..n).map(|_| HashMap::new()).collect(),
            target: HashMap::new(),
        }
    }

    /// Records one baseline line's tokens. Placeholder tokens (`<TS>`, `<NUM>`, ...) are
    /// skipped: they're already canonicalized by masking, so tracking them as "values" would
    /// just be tracking the placeholder string itself, never anything a NEW VALUE finding
    /// could usefully report.
    pub fn record_baseline(
        &mut self,
        baseline_idx: usize,
        cluster_id: usize,
        line_no: usize,
        raw: &str,
        tokens: &[String],
    ) {
        let map = &mut self.baselines[baseline_idx];
        for (pos, tok) in tokens.iter().enumerate() {
            if is_placeholder(tok) || looks_identifier_like(tok) {
                continue;
            }
            map.entry((cluster_id, pos))
                .or_default()
                .record(tok, line_no, raw);
        }
    }

    pub fn record_target(
        &mut self,
        cluster_id: usize,
        line_no: usize,
        raw: &str,
        tokens: &[String],
    ) {
        for (pos, tok) in tokens.iter().enumerate() {
            if is_placeholder(tok) || looks_identifier_like(tok) {
                continue;
            }
            self.target
                .entry((cluster_id, pos))
                .or_default()
                .record(tok, line_no, raw);
        }
    }

    /// If the target introduced a value at `(cluster_id, pos)` that never appeared in any
    /// baseline, the position is low-cardinality in both (no side overflowed its
    /// `MAX_TRACKED_VALUES` cap), and the baseline values are established enough (recur at
    /// least `MIN_AVG_REPETITION` times on average) to trust as "this position is a status,
    /// not an identifier that happened to repeat a little," returns the most frequent such
    /// new value. Ties broken alphabetically for determinism.
    pub fn new_value_at(&self, cluster_id: usize, pos: usize) -> Option<NewValue> {
        let key = (cluster_id, pos);

        let mut baseline_set: HashSet<&str> = HashSet::new();
        let mut baseline_total: u64 = 0;
        for b in &self.baselines {
            if let Some(pv) = b.get(&key) {
                if pv.overflowed {
                    return None;
                }
                baseline_set.extend(pv.values.keys().map(String::as_str));
                baseline_total += pv.values.values().map(|e| e.count).sum::<u64>();
            }
        }
        if baseline_set.is_empty() || baseline_set.len() > MAX_TRACKED_VALUES {
            return None;
        }
        let established = baseline_total as f64 / baseline_set.len() as f64;
        if established < MIN_AVG_REPETITION {
            return None;
        }

        let target_pv = self.target.get(&key)?;
        if target_pv.overflowed {
            return None;
        }

        let mut candidates: Vec<(&str, &ValueEntry)> = target_pv
            .values
            .iter()
            .filter(|(v, _)| !baseline_set.contains(v.as_str()))
            .map(|(v, e)| (v.as_str(), e))
            .collect();
        candidates.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
        let (value, entry) = candidates.into_iter().next()?;

        let mut baseline_values: Vec<String> = baseline_set.into_iter().map(String::from).collect();
        baseline_values.sort();

        Some(NewValue {
            value: value.to_string(),
            baseline_values,
            first_target_line_no: entry.first_line_no,
            first_target_raw: entry.first_raw.clone(),
            established,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records `value` at (cluster 0, position 0) of baseline `baseline_idx`, `n` times, with
    /// made-up increasing line numbers, so tests can easily clear `MIN_AVG_REPETITION`.
    fn record_n(
        t: &mut ValueTracker,
        baseline_idx: usize,
        value: &str,
        n: usize,
        start_line: usize,
    ) {
        for i in 0..n {
            t.record_baseline(baseline_idx, 0, start_line + i, "r", &[value.to_string()]);
        }
    }

    #[test]
    fn flags_a_value_never_seen_in_any_baseline() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "PASSED", 8, 1);
        t.record_target(0, 100, "raw100", &["FAILED".to_string()]);

        let found = t.new_value_at(0, 0).expect("should flag FAILED as new");
        assert_eq!(found.value, "FAILED");
        assert_eq!(found.baseline_values, vec!["PASSED".to_string()]);
        assert_eq!(found.first_target_line_no, 100);
        assert_eq!(found.first_target_raw, "raw100");
    }

    #[test]
    fn does_not_flag_a_value_already_seen_in_baseline() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "PASSED", 6, 1);
        record_n(&mut t, 0, "FAILED", 6, 100);
        t.record_target(0, 200, "r", &["FAILED".to_string()]);
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn placeholder_tokens_are_never_tracked() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "<IP>", 8, 1);
        t.record_target(0, 100, "r", &["<IP>".to_string()]);
        // Nothing was ever recorded (both were placeholders), so there's nothing to report.
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn high_cardinality_position_is_not_flagged() {
        let mut t = ValueTracker::with_baselines(1);
        // letter-only labels (no digits/separators) so this actually exercises the
        // cardinality cap, not the identifier filter below.
        for i in 0..(MAX_TRACKED_VALUES + 5) {
            let label = format!("status{}", (b'a' + (i % 26) as u8) as char);
            t.record_baseline(0, 0, i, "r", &[label]);
        }
        t.record_target(0, 999, "r", &["status-new".to_string()]);
        assert!(
            t.new_value_at(0, 0).is_none(),
            "a high-cardinality position must overflow and stop being reported"
        );
    }

    #[test]
    fn identifier_like_values_are_never_tracked() {
        // A pytest-xdist worker id (contains a digit) and a test id (contains `::` and `.`)
        // should never make it into the tracker at all, so a position that's *entirely*
        // these never accumulates enough to report - the false positives from #2.
        let mut t = ValueTracker::with_baselines(1);
        for (i, worker) in ["gw0", "gw1", "gw2"].iter().cycle().take(9).enumerate() {
            t.record_baseline(0, 0, i, "r", &[worker.to_string()]);
        }
        t.record_target(0, 100, "r", &["gw3".to_string()]);
        assert!(
            t.new_value_at(0, 0).is_none(),
            "worker ids must not be tracked as values"
        );

        let mut t2 = ValueTracker::with_baselines(1);
        for i in 0..8 {
            t2.record_baseline(0, 0, i, "r", &["tests/test_a.py::test_x".to_string()]);
        }
        t2.record_target(0, 100, "r", &["tests/test_b.py::test_y".to_string()]);
        assert!(
            t2.new_value_at(0, 0).is_none(),
            "test ids must not be tracked as values"
        );
    }

    #[test]
    fn low_repetition_baseline_values_are_not_established_enough() {
        // Each of 3 distinct values seen exactly once (avg = 1.0, well under the 5.0
        // threshold) - not enough baseline history to call this a real "status" position.
        let mut t = ValueTracker::with_baselines(1);
        t.record_baseline(0, 0, 1, "r", &["alpha".to_string()]);
        t.record_baseline(0, 0, 2, "r", &["bravo".to_string()]);
        t.record_baseline(0, 0, 3, "r", &["charlie".to_string()]);
        t.record_target(0, 4, "r", &["delta".to_string()]);
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn established_status_position_is_flagged_once_repetition_clears_the_bar() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "PASSED", 5, 1); // exactly at the 5.0 average
        t.record_target(0, 100, "r", &["FAILED".to_string()]);
        assert!(t.new_value_at(0, 0).is_some());
    }

    #[test]
    fn most_frequent_new_value_wins_ties_broken_alphabetically() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "PASSED", 6, 1);
        t.record_target(0, 100, "r", &["FLAKY".to_string()]);
        t.record_target(0, 101, "r", &["FLAKY".to_string()]);
        t.record_target(0, 102, "r", &["ERROR".to_string()]);
        let found = t.new_value_at(0, 0).unwrap();
        assert_eq!(found.value, "FLAKY"); // seen twice vs ERROR's once
    }

    #[test]
    fn multiple_baselines_union_their_seen_values() {
        let mut t = ValueTracker::with_baselines(2);
        record_n(&mut t, 0, "PASSED", 5, 1);
        record_n(&mut t, 1, "SKIPPED", 5, 1);
        t.record_target(0, 100, "r", &["SKIPPED".to_string()]);
        // SKIPPED was seen in baseline 1, so it's not "new" even though baseline 0 never saw it.
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn established_reflects_baseline_repetition() {
        let mut t = ValueTracker::with_baselines(1);
        record_n(&mut t, 0, "PASSED", 20, 1);
        t.record_target(0, 100, "r", &["FAILED".to_string()]);
        let found = t.new_value_at(0, 0).unwrap();
        assert_eq!(found.established, 20.0); // 20 occurrences / 1 distinct value
    }
}
