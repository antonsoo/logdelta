//! Real CLI gates for changes in already-seen values. The ledger is the denominator oracle.
use assert_cmd::prelude::*;
use serde_json::{json, Value};
use std::process::Command;

fn requests(route: &str, total: usize, errors: usize) -> String {
    (0..total)
        .map(|i| {
            format!(
                "{}\n",
                json!({"route": route, "status": if i < errors {503} else {200}})
            )
        })
        .collect()
}

fn run(baselines: &[&str], target: &str, extra: &[&str]) -> (i32, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("logdelta").unwrap();
    cmd.arg("diff");
    for (i, text) in baselines.iter().enumerate() {
        let path = dir.path().join(format!("baseline-{i}.jsonl"));
        std::fs::write(&path, text).unwrap();
        cmd.arg(path);
    }
    let path = dir.path().join("target.jsonl");
    std::fs::write(&path, target).unwrap();
    let output = cmd
        .arg("--target")
        .arg(path)
        .args(["--watch-field", "/status"])
        .args(extra)
        .output()
        .unwrap();
    let text = if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    (
        output.status.code().unwrap(),
        String::from_utf8(text).unwrap(),
    )
}

fn report(baselines: &[&str], target: &str, extra: &[&str]) -> (i32, Value) {
    let (exit, text) = run(baselines, target, &[&["--json"][..], extra].concat());
    (exit, serde_json::from_str(&text).unwrap())
}

const RATE_ARGS: &[&str] = &["--watch-by", "/route", "--watch-rate-change", "5"];

#[test]
fn already_seen_failure_rate_is_one_group_finding_with_exact_denominators() {
    let a = requests("/checkout", 1000, 10);
    let b = requests("/checkout", 1000, 12);
    let target = requests("/checkout", 1000, 200);
    assert_eq!(report(&[&a, &b], &target, &["--watch-by", "/route"]).0, 0);
    let (exit, report) = report(&[&a, &b], &target, RATE_ARGS);
    assert_eq!(exit, 1);
    assert_eq!(report["findings"], json!([]));
    assert_eq!(report["value_findings"], json!([]));
    let field = &report["watched_fields"][0];
    assert!(field["values"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["is_new"] == false));
    let rates = &field["rate_comparison"];
    assert_eq!(rates["complete"], true);
    assert_eq!(rates["min_change_pp"], 5.0);
    let groups = rates["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["baseline_totals"], json!([1000, 1000]));
    assert_eq!(groups[0]["target_total"], 1000);
    let changes = groups[0]["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    let errors = changes
        .iter()
        .find(|change| {
            field["values"][change["value_index"].as_u64().unwrap() as usize]["value_json"] == "503"
        })
        .unwrap();
    assert_eq!(errors["baseline_rates"], json!([0.01, 0.012]));
    assert_eq!(errors["target_rate"], 0.2);
    assert!((errors["range_distance_pp"].as_f64().unwrap() - 18.8).abs() < 1e-10);
    let (_, human) = run(&[&a, &b], &target, RATE_ARGS);
    assert!(human.contains("1 finding"), "{human}");
    assert!(human.contains("CHANGED RATES"));
    assert!(human.contains("1.0000% / 1.2000% -> target 20.0000%"));
}

#[test]
fn changed_route_mix_does_not_change_within_route_rates() {
    let baseline = requests("/checkout", 1000, 10) + &requests("/maintenance", 1000, 900);
    let target = requests("/checkout", 2000, 20) + &requests("/maintenance", 100, 90);
    let (_, pooled) = report(&[&baseline], &target, &["--watch-rate-change", "5"]);
    assert!(
        !pooled["watched_fields"][0]["rate_comparison"]["groups"][0]["changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (exit, grouped) = report(&[&baseline], &target, RATE_ARGS);
    assert_eq!(exit, 0);
    for group in grouped["watched_fields"][0]["rate_comparison"]["groups"]
        .as_array()
        .unwrap()
    {
        assert_eq!(group["status"], "compared");
        assert_eq!(group["changes"], json!([]));
    }
}

#[test]
fn a_target_inside_observed_baseline_variation_does_not_trip_a_pooled_score() {
    let a = requests("/checkout", 1000, 10);
    let b = requests("/checkout", 10000, 3000);
    let target = requests("/checkout", 10000, 2000);
    assert!(logdelta::scoring::score_template(&[10, 3000], &[1000, 10000], 2000, 10000) > 10.83);
    let (_, report) = report(&[&a, &b], &target, RATE_ARGS);
    assert_eq!(
        report["watched_fields"][0]["rate_comparison"]["groups"][0]["changes"],
        json!([])
    );
}

#[test]
fn ordinary_small_count_fluctuations_stay_below_the_score_cutoff() {
    let baseline = requests("/checkout", 1000, 10);
    let target = requests("/checkout", 5, 1);
    let (_, report) = report(&[&baseline], &target, RATE_ARGS);
    assert_eq!(
        report["watched_fields"][0]["rate_comparison"]["groups"][0]["changes"],
        json!([])
    );
}

#[test]
fn the_effect_cutoff_is_inclusive_despite_floating_point_roundoff() {
    let baseline = requests("/checkout", 10000, 1000);
    let target = requests("/checkout", 10000, 1500);
    let (exit, result) = report(&[&baseline], &target, RATE_ARGS);
    assert_eq!(exit, 1);
    assert_eq!(
        result["watched_fields"][0]["rate_comparison"]["groups"][0]["changes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (_, stricter) = report(&[&baseline], &target, &["--watch-rate-change", "5.01"]);
    assert_eq!(
        stricter["watched_fields"][0]["rate_comparison"]["groups"][0]["changes"],
        json!([])
    );
}

#[test]
fn absent_groups_have_unknown_rates_and_cannot_pass_a_gate() {
    let common = requests("/checkout", 1000, 10);
    let extra = requests("/archive", 100, 20);
    for (baseline, target, status) in [
        (
            common.clone() + &extra,
            common.clone(),
            "no_target_observations",
        ),
        (
            common.clone(),
            common.clone() + &extra,
            "no_baseline_observations",
        ),
    ] {
        let (exit, report) = report(&[&baseline], &target, RATE_ARGS);
        assert_eq!(exit, 2);
        let field = &report["watched_fields"][0];
        assert_eq!(field["complete"], true); // Exact novelty evidence is still complete.
        assert_eq!(field["rate_comparison"]["complete"], false);
        let unavailable = &field["rate_comparison"]["groups"][0];
        assert_eq!(unavailable["status"], status);
        assert_eq!(unavailable["changes"], json!([]));
        let (_, human) = run(&[&baseline], &target, RATE_ARGS);
        assert!(human.contains("INCOMPLETE RATES"));
        assert!(!human.contains("No significant differences found"));
    }
}

#[test]
fn incomplete_or_truncated_ledgers_never_supply_partial_rate_denominators() {
    let baseline = requests("/checkout", 1000, 10);
    for extra in [
        "{not-json}\n".to_string(),
        (0..260)
            .map(|i| format!("{{\"route\":\"/{i}\",\"status\":200}}\n"))
            .collect(),
    ] {
        let target = requests("/checkout", 1000, 200) + &extra;
        let (exit, report) = report(&[&baseline], &target, RATE_ARGS);
        assert_eq!(exit, 2);
        let field = &report["watched_fields"][0];
        assert_eq!(field["complete"], false);
        assert_eq!(field["rate_comparison"]["complete"], false);
        assert_eq!(field["rate_comparison"]["groups"], json!([]));
    }
}

#[test]
fn zero_group_baselines_are_unobserved_not_zero_rates() {
    let other = requests("/maintenance", 100, 50);
    let a = other.clone();
    let b = other.clone() + &requests("/checkout", 1000, 10);
    let target = other + &requests("/checkout", 1000, 200);
    let (exit, report) = report(&[&a, &b], &target, RATE_ARGS);
    assert_eq!(exit, 1);
    let group = &report["watched_fields"][0]["rate_comparison"]["groups"][0];
    assert_eq!(group["baseline_totals"], json!([0, 1000]));
    assert_eq!(group["changes"][1]["baseline_rates"], json!([null, 0.01]));
}

#[test]
fn rate_evidence_and_requested_context_survive_json_and_markdown() {
    let route = "/checkout|<script>bad</script>";
    let baseline = requests(route, 1000, 10);
    let target = requests(route, 1000, 200);
    let (_, report) = report(&[&baseline], &target, &[RATE_ARGS, &["-C", "1"]].concat());
    let field = &report["watched_fields"][0];
    assert_eq!(field["values"][1]["context"]["after"][0][0], 2);
    let (_, markdown) = run(
        &[&baseline],
        &target,
        &[RATE_ARGS, &["--markdown"]].concat(),
    );
    assert!(markdown.contains("CHANGED RATES"));
    assert!(markdown.contains("&#124;&lt;script&gt;bad&lt;/script&gt;"));
    assert!(!markdown.contains("<script>"));
    assert!(markdown.contains("baseline 1000; target 1000"));
    assert!(markdown.contains("20.0000%"));
}

#[test]
fn a_known_value_falling_to_zero_keeps_its_baseline_source() {
    let baseline = requests("/checkout", 1000, 200);
    let target = requests("/checkout", 1000, 0);
    let (exit, result) = report(&[&baseline], &target, RATE_ARGS);
    assert_eq!(exit, 1);
    let field = &result["watched_fields"][0];
    let change = &field["rate_comparison"]["groups"][0]["changes"][1];
    assert_eq!(change["target_rate"], 0.0);
    let value = &field["values"][change["value_index"].as_u64().unwrap() as usize];
    assert_eq!(value["value_json"], "503");
    assert!(value.get("first_target").is_none());
    assert_eq!(value["first_baseline"]["line_no"], 1);
    let (_, markdown) = run(
        &[&baseline],
        &target,
        &[RATE_ARGS, &["--markdown"]].concat(),
    );
    assert!(
        markdown.contains("<code>503</code> at <code>baseline 1:1</code>"),
        "{markdown}"
    );
    let (_, human) = run(&[&baseline], &target, RATE_ARGS);
    assert!(human.contains("First example baseline 1:1"));
}

#[test]
fn invalid_rate_thresholds_fail_before_reading_inputs() {
    use logdelta::analysis::{diff_runs, DiffOptions};
    for threshold in [0.0, -1.0, 100.01, f64::NAN, f64::INFINITY] {
        let options = DiffOptions {
            watch_fields: vec!["/status".into()],
            watch_rate_change: Some(threshold),
            ..DiffOptions::default()
        };
        let error = diff_runs(
            &["/nonexistent-baseline"],
            "/nonexistent-target",
            &[],
            &options,
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("watch-rate-change"));
    }
    let output = Command::cargo_bin("logdelta")
        .unwrap()
        .args([
            "diff",
            "missing",
            "also-missing",
            "--watch-rate-change",
            "5",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("--watch-field"));
}
