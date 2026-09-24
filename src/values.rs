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
            if is_placeholder(tok) {
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
            if is_placeholder(tok) {
                continue;
            }
            self.target
                .entry((cluster_id, pos))
                .or_default()
                .record(tok, line_no, raw);
        }
    }

    /// If the target introduced a value at `(cluster_id, pos)` that never appeared in any
    /// baseline, and the position is low-cardinality in both (no side overflowed its
    /// `MAX_TRACKED_VALUES` cap), returns the most frequent such new value. Ties broken
    /// alphabetically for determinism.
    pub fn new_value_at(&self, cluster_id: usize, pos: usize) -> Option<NewValue> {
        let key = (cluster_id, pos);

        let mut baseline_set: HashSet<&str> = HashSet::new();
        for b in &self.baselines {
            if let Some(pv) = b.get(&key) {
                if pv.overflowed {
                    return None;
                }
                baseline_set.extend(pv.values.keys().map(String::as_str));
            }
        }
        if baseline_set.len() > MAX_TRACKED_VALUES {
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_a_value_never_seen_in_any_baseline() {
        let mut t = ValueTracker::with_baselines(1);
        t.record_baseline(0, 0, 1, "raw1", &["PASSED".to_string()]);
        t.record_baseline(0, 0, 2, "raw2", &["PASSED".to_string()]);
        t.record_target(0, 3, "raw3", &["FAILED".to_string()]);

        let found = t.new_value_at(0, 0).expect("should flag FAILED as new");
        assert_eq!(found.value, "FAILED");
        assert_eq!(found.baseline_values, vec!["PASSED".to_string()]);
        assert_eq!(found.first_target_line_no, 3);
        assert_eq!(found.first_target_raw, "raw3");
    }

    #[test]
    fn does_not_flag_a_value_already_seen_in_baseline() {
        let mut t = ValueTracker::with_baselines(1);
        t.record_baseline(0, 0, 1, "r", &["PASSED".to_string()]);
        t.record_baseline(0, 0, 2, "r", &["FAILED".to_string()]);
        t.record_target(0, 3, "r", &["FAILED".to_string()]);
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn placeholder_tokens_are_never_tracked() {
        let mut t = ValueTracker::with_baselines(1);
        t.record_baseline(0, 0, 1, "r", &["<IP>".to_string()]);
        t.record_target(0, 2, "r", &["<IP>".to_string()]);
        // Nothing was ever recorded (both were placeholders), so there's nothing to report.
        assert!(t.new_value_at(0, 0).is_none());
    }

    #[test]
    fn high_cardinality_position_is_not_flagged() {
        let mut t = ValueTracker::with_baselines(1);
        for i in 0..(MAX_TRACKED_VALUES + 5) {
            t.record_baseline(0, 0, i, "r", &[format!("path{i}")]);
        }
        t.record_target(0, 999, "r", &["path-new".to_string()]);
        assert!(
            t.new_value_at(0, 0).is_none(),
            "an id-like / high-cardinality position must overflow and stop being reported"
        );
    }

    #[test]
    fn most_frequent_new_value_wins_ties_broken_alphabetically() {
        let mut t = ValueTracker::with_baselines(1);
        t.record_baseline(0, 0, 1, "r", &["PASSED".to_string()]);
        t.record_target(0, 2, "r", &["FLAKY".to_string()]);
        t.record_target(0, 3, "r", &["FLAKY".to_string()]);
        t.record_target(0, 4, "r", &["ERROR".to_string()]);
        let found = t.new_value_at(0, 0).unwrap();
        assert_eq!(found.value, "FLAKY"); // seen twice vs ERROR's once
    }

    #[test]
    fn multiple_baselines_union_their_seen_values() {
        let mut t = ValueTracker::with_baselines(2);
        t.record_baseline(0, 0, 1, "r", &["PASSED".to_string()]);
        t.record_baseline(1, 0, 1, "r", &["SKIPPED".to_string()]);
        t.record_target(0, 2, "r", &["SKIPPED".to_string()]);
        // SKIPPED was seen in baseline 1, so it's not "new" even though baseline 0 never saw it.
        assert!(t.new_value_at(0, 0).is_none());
    }
}
