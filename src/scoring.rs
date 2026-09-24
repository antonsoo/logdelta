//! Scores how surprising a template's frequency change is between baseline run(s) and a
//! target run, using a G-test (log-likelihood-ratio test) on a 2x2 contingency table, with
//! additive smoothing so zero counts don't blow up and with a variance-based penalty that
//! down-weights templates whose rate already jumps around across multiple passing baselines
//! (flaky noise) instead of treating every jump as equally suspicious.
//!
//! Reference for the log-likelihood-ratio statistic: T. Dunning, "Accurate Methods for the
//! Statistics of Surprise and Coincidence", Computational Linguistics 19(1), 1993, pp. 61-74.
//! The 2x2 table compared here is {this template, every other template} x {baseline run(s), target
//! run}; it is the same construction Dunning uses for comparing word frequencies between two
//! corpora, applied to log templates instead of words.

/// Additive (Laplace-style) smoothing constant added to every cell of the 2x2 table before
/// computing G, so a template with zero occurrences in one side never produces `ln(0)`.
pub const SMOOTHING_ALPHA: f64 = 0.5;

/// Default significance cutoff. `10.83` is the chi-square critical value for 1 degree of
/// freedom at p = 0.001; G is asymptotically chi-square distributed under the null
/// hypothesis of equal rates, so this is a "p < 0.001-ish" threshold in the same units.
pub const DEFAULT_SIGNIFICANCE: f64 = 10.83;

/// G-statistic for one template's occurrence count in a baseline sample of `baseline_total`
/// lines (with `baseline_count` matches) versus a target sample of `target_total` lines
/// (with `target_count` matches).
pub fn g_test(
    baseline_count: u64,
    baseline_total: u64,
    target_count: u64,
    target_total: u64,
) -> f64 {
    let a = baseline_count as f64;
    let b = (baseline_total.saturating_sub(baseline_count)) as f64;
    let c = target_count as f64;
    let d = (target_total.saturating_sub(target_count)) as f64;

    let o11 = a + SMOOTHING_ALPHA;
    let o12 = b + SMOOTHING_ALPHA;
    let o21 = c + SMOOTHING_ALPHA;
    let o22 = d + SMOOTHING_ALPHA;

    let row1 = o11 + o12;
    let row2 = o21 + o22;
    let col1 = o11 + o21;
    let col2 = o12 + o22;
    let n = row1 + row2;
    if n <= 0.0 {
        return 0.0;
    }

    let mut g = 0.0;
    for (o, row, col) in [
        (o11, row1, col1),
        (o12, row1, col2),
        (o21, row2, col1),
        (o22, row2, col2),
    ] {
        let e = row * col / n;
        if e > 0.0 && o > 0.0 {
            g += o * (o / e).ln();
        }
    }
    2.0 * g
}

/// Population standard deviation of `values` (0.0 for 0 or 1 samples).
fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    var.sqrt()
}

/// How much to divide a raw G-score by to account for a template's rate already varying
/// across multiple passing baseline runs. `rates` are per-baseline occurrence rates
/// (count / total lines) for runs where the template's total line count was > 0. Returns
/// `1.0` (no penalty) with fewer than 2 usable baselines.
pub fn flakiness_penalty(rates: &[f64]) -> f64 {
    if rates.len() < 2 {
        return 1.0;
    }
    let mean = rates.iter().sum::<f64>() / rates.len() as f64;
    if mean <= 0.0 {
        return 1.0;
    }
    let cv = stddev(rates) / mean; // coefficient of variation
    1.0 + 2.0 * cv
}

/// Convenience wrapper: computes the (possibly down-weighted) score for a template that
/// occurred `baseline_counts[i]` times in baseline run `i` (of `baseline_totals[i]` lines
/// total) and `target_count` times in a target run of `target_total` lines.
pub fn score_template(
    baseline_counts: &[u64],
    baseline_totals: &[u64],
    target_count: u64,
    target_total: u64,
) -> f64 {
    let combined_baseline_count: u64 = baseline_counts.iter().sum();
    let combined_baseline_total: u64 = baseline_totals.iter().sum();
    let raw = g_test(
        combined_baseline_count,
        combined_baseline_total,
        target_count,
        target_total,
    );
    let rates: Vec<f64> = baseline_counts
        .iter()
        .zip(baseline_totals.iter())
        .filter(|(_, &t)| t > 0)
        .map(|(&c, &t)| c as f64 / t as f64)
        .collect();
    raw / flakiness_penalty(&rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_rates_score_near_zero() {
        let g = g_test(500, 1000, 500, 1000);
        assert!(g < 0.5, "expected near-zero G for identical rates, got {g}");
    }

    #[test]
    fn brand_new_template_scores_high() {
        // present 0/1000 in baseline, 50/1000 in target
        let g = g_test(0, 1000, 50, 1000);
        assert!(g > DEFAULT_SIGNIFICANCE, "expected a high G, got {g}");
    }

    #[test]
    fn vanished_template_scores_high() {
        let g = g_test(50, 1000, 0, 1000);
        assert!(g > DEFAULT_SIGNIFICANCE, "expected a high G, got {g}");
    }

    #[test]
    fn rare_template_low_noise_stays_below_threshold() {
        // 1 in 1000 vs 2 in 1000: plausible noise, should not be flagged at the default cutoff
        let g = g_test(1, 1000, 2, 1000);
        assert!(
            g < DEFAULT_SIGNIFICANCE,
            "expected low G for tiny counts, got {g}"
        );
    }

    #[test]
    fn smoothing_avoids_nan_and_inf() {
        let g = g_test(0, 0, 0, 0);
        assert!(g.is_finite());
        let g2 = g_test(0, 5, 5, 5);
        assert!(g2.is_finite());
    }

    #[test]
    fn no_penalty_with_one_baseline() {
        assert_eq!(flakiness_penalty(&[0.1]), 1.0);
        assert_eq!(flakiness_penalty(&[]), 1.0);
    }

    #[test]
    fn flaky_template_penalized_more_than_stable_one() {
        let stable = flakiness_penalty(&[0.10, 0.11, 0.09, 0.10]);
        let flaky = flakiness_penalty(&[0.01, 0.30, 0.02, 0.25]);
        assert!(flaky > stable, "flaky={flaky} stable={stable}");
    }

    #[test]
    fn score_template_downweights_flaky_baselines() {
        let target_count = 40;
        let target_total = 1000;
        let stable_score = score_template(
            &[100, 105, 98, 102],
            &[1000, 1000, 1000, 1000],
            target_count,
            target_total,
        );
        let flaky_score = score_template(
            &[10, 200, 15, 180],
            &[1000, 1000, 1000, 1000],
            target_count,
            target_total,
        );
        assert!(flaky_score < stable_score);
    }
}
