//! Opt-in rate comparisons over the exact field ledger. Denominators contain only scalar
//! observations of this field in this group, never unrelated log lines or other routes.

use std::collections::BTreeMap;
use std::io;

use serde::Serialize;

use crate::fields::WatchedField;
use crate::scoring::score_template;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RateGroupStatus {
    Compared,
    NoBaselineObservations,
    NoTargetObservations,
}

#[derive(Serialize)]
pub struct FieldRateChange {
    /// Index into the enclosing WatchedField.values: exact value, counts and source evidence.
    pub value_index: usize,
    /// Fractions in [0, 1]; null when this group was not observed in that baseline run.
    pub baseline_rates: Vec<Option<f64>>,
    pub target_rate: f64,
    /// Signed percentage points outside the closest edge of the observed baseline range.
    pub range_distance_pp: f64,
    /// Existing smoothed G statistic, divided by the baseline variability penalty. Not a p-value.
    pub score: f64,
}

#[derive(Serialize)]
pub struct FieldRateGroup {
    pub group_values_json: Vec<String>,
    pub baseline_totals: Vec<u64>,
    pub target_total: u64,
    pub status: RateGroupStatus,
    /// Several values can move together; the group counts as one rate finding.
    pub changes: Vec<FieldRateChange>,
}

#[derive(Serialize)]
pub struct FieldRateComparison {
    pub min_change_pp: f64,
    pub significance: f64,
    pub complete: bool,
    pub groups: Vec<FieldRateGroup>,
}

impl FieldRateComparison {
    pub fn finding_count(&self) -> usize {
        self.groups
            .iter()
            .filter(|group| !group.changes.is_empty())
            .count()
    }
}

pub(crate) fn validate_options(
    pointers: &[String],
    min_change_pp: Option<f64>,
    significance: f64,
) -> io::Result<()> {
    if let Some(delta) = min_change_pp {
        let message = if pointers.is_empty() {
            Some("--watch-rate-change requires at least one --watch-field")
        } else if !delta.is_finite() || delta <= 0.0 || delta > 100.0 {
            Some("--watch-rate-change must be a finite number of percentage points in (0, 100]")
        } else if !significance.is_finite() || significance < 0.0 {
            Some("--significance must be finite and non-negative for rate comparisons")
        } else {
            None
        };
        if let Some(message) = message {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, message));
        }
    }
    Ok(())
}

pub(crate) fn compare(
    field: &WatchedField,
    min_change_pp: f64,
    significance: f64,
) -> FieldRateComparison {
    let mut comparison = FieldRateComparison {
        min_change_pp,
        significance,
        complete: field.complete,
        groups: Vec::new(),
    };
    // A truncated ledger cannot supply a trustworthy denominator. Keep its raw evidence,
    // but never compute apparently precise rates from the retained subset.
    if !field.complete {
        return comparison;
    }
    let mut members: BTreeMap<&[String], Vec<usize>> = BTreeMap::new();
    for (index, value) in field.values.iter().enumerate() {
        members
            .entry(&value.group_values_json)
            .or_default()
            .push(index);
    }
    for (key, indexes) in members {
        let mut group = FieldRateGroup {
            group_values_json: key.to_vec(),
            baseline_totals: vec![0; field.baselines.len()],
            target_total: 0,
            status: RateGroupStatus::Compared,
            changes: Vec::new(),
        };
        for &index in &indexes {
            let value = &field.values[index];
            for (total, count) in group.baseline_totals.iter_mut().zip(&value.baseline_counts) {
                *total += count;
            }
            group.target_total += value.target_count;
        }
        if group.baseline_totals.iter().all(|&n| n == 0) {
            group.status = RateGroupStatus::NoBaselineObservations;
        } else if group.target_total == 0 {
            group.status = RateGroupStatus::NoTargetObservations;
        }
        if group.status != RateGroupStatus::Compared {
            comparison.complete = false;
            comparison.groups.push(group);
            continue;
        }
        for index in indexes {
            let value = &field.values[index];
            // New values already have exact novelty findings. Rate findings describe changes
            // in outcomes with baseline observations, including a known value falling to zero.
            if value.baseline_counts.iter().all(|&n| n == 0) {
                continue;
            }
            let baseline_rates: Vec<Option<f64>> = value
                .baseline_counts
                .iter()
                .zip(&group.baseline_totals)
                .map(|(&count, &total)| (total > 0).then(|| count as f64 / total as f64))
                .collect();
            let low = baseline_rates
                .iter()
                .flatten()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let high = baseline_rates
                .iter()
                .flatten()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let target_rate = value.target_count as f64 / group.target_total as f64;
            let distance = if target_rate > high {
                target_rate - high
            } else if target_rate < low {
                target_rate - low
            } else {
                0.0
            };
            let range_distance_pp = distance * 100.0;
            // Compare outside every observed baseline rate, not just a pooled mean that can
            // hide run-to-run variation. The G score also suppresses small-count fluctuations.
            // The cutoff is inclusive; allow only round-off at percentage-point scale.
            if distance == 0.0 || range_distance_pp.abs() + 1e-12 < min_change_pp {
                continue;
            }
            let score = score_template(
                &value.baseline_counts,
                &group.baseline_totals,
                value.target_count,
                group.target_total,
            );
            if score >= significance {
                group.changes.push(FieldRateChange {
                    value_index: index,
                    baseline_rates,
                    target_rate,
                    range_distance_pp,
                    score,
                });
            }
        }
        comparison.groups.push(group);
    }
    comparison
}
