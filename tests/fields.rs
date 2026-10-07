//! Run the actual native command, so a missed outcome also checks the CI exit contract.
use assert_cmd::prelude::*;
use serde_json::{json, Value};
use std::process::Command;

fn diff(baselines: &[&str], target: &str, pointers: &[&str], extra: &[&str]) -> (i32, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("logdelta").unwrap();
    cmd.arg("diff");
    for (i, text) in baselines.iter().enumerate() {
        let path = dir.path().join(format!("baseline-{i}.log"));
        std::fs::write(&path, text).unwrap();
        cmd.arg(path);
    }
    let path = dir.path().join("target.log");
    std::fs::write(&path, target).unwrap();
    cmd.arg("--target").arg(path);
    for pointer in pointers {
        cmd.arg("--watch-field").arg(pointer);
    }
    let out = cmd.args(extra).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(if out.stdout.is_empty() {
            out.stderr
        } else {
            out.stdout
        })
        .unwrap(),
    )
}

fn report(baselines: &[&str], target: &str, pointers: &[&str]) -> (i32, Value) {
    let (code, text) = diff(baselines, target, pointers, &["--json"]);
    (code, serde_json::from_str(&text).unwrap())
}

#[test]
fn unchanged_templates_do_not_hide_watched_status_and_exit_code() {
    let good = include_str!("../examples/http-good.log");
    let other = include_str!("../examples/http-good-2.log");
    let bad = include_str!("../examples/http-failed.log");
    let (code, default) = report(&[good, other], bad, &[]);
    assert_eq!(code, 0);
    assert_eq!(default["findings"], json!([]));
    assert_eq!(default["value_findings"], json!([]));
    assert!(default.get("watched_fields").is_none());
    let (code, watched) = report(&[good, other], bad, &["/http/status", "/exit_code"]);
    assert_eq!(code, 1);
    let field = &watched["watched_fields"][0];
    assert_eq!(field["complete"], true);
    assert_eq!(field["values"][0]["value_json"], "200");
    assert_eq!(field["values"][0]["baseline_counts"], json!([20, 20]));
    assert_eq!(field["values"][0]["target_count"], 18);
    assert_eq!(field["values"][1]["value_json"], "503");
    assert_eq!(field["values"][1]["target_count"], 2);
    assert_eq!(field["values"][1]["is_new"], true);
    assert_eq!(field["values"][1]["first_target"]["line_no"], 8);
    assert!(field["values"][1]["first_target"]["raw"]
        .as_str()
        .unwrap()
        .contains("\"status\":503"));
    assert_eq!(watched["watched_fields"][1]["values"][1]["value_json"], "1");
    assert_eq!(report(&[good], other, &["/http/status", "/exit_code"]).0, 0);
}

#[test]
fn types_large_numbers_number_spelling_and_multiple_new_values_survive() {
    let (code, r) = report(
        &["{\"v\":9007199254740992}\n{\"v\":null}\n"],
        "{\"v\":9007199254740993}\n{\"v\":\"9007199254740992\"}\n{\"v\":false}\n{\"v\":1e400}\n",
        &["/v"],
    );
    assert_eq!(code, 1);
    let field = &r["watched_fields"][0];
    assert_eq!(field["complete"], true);
    let new: Vec<_> = field["values"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["is_new"] == true)
        .map(|v| v["value_json"].as_str().unwrap())
        .collect();
    assert_eq!(
        new,
        vec!["\"9007199254740992\"", "1e400", "9007199254740993", "false"]
    );
    assert_eq!(report(&["{\"v\":200}"], "{\"v\":200.0}", &["/v"]).0, 1);
    assert_eq!(
        report(&["{\"v\":\"a\"}"], "{\"v\":\"\\u0061\"}", &["/v"]).0,
        0
    );
}

#[test]
fn a_value_known_in_any_baseline_is_not_new_and_counts_are_per_run() {
    let (_, r) = report(
        &["{\"v\":200}\n{\"v\":200}", "{\"v\":503}"],
        "{\"v\":503}",
        &["/v"],
    );
    let values = r["watched_fields"][0]["values"].as_array().unwrap();
    assert!(values.iter().all(|v| v["is_new"] == false));
    assert_eq!(values[1]["baseline_counts"], json!([0, 1]));
    assert_eq!(values[1]["first_baseline"]["baseline_index"], 1);
}

#[test]
fn nested_array_and_escaped_keys_follow_json_pointer_without_unicode_normalization() {
    let baseline = r#"{"a/b":{"~1":[{"":200}]},"é":"yes","é":"no"}"#;
    let target = r#"{"a/b":{"~1":[{"":500}]},"é":"yes","é":"new"}"#;
    let (code, r) = report(&[baseline], target, &["/a~1b/~01/0/", "/é", "/é"]);
    assert_eq!(code, 1);
    assert_eq!(r["watched_fields"][0]["values"][1]["is_new"], true);
    assert!(r["watched_fields"][1]["values"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["is_new"] == false));
    assert_eq!(report(&[baseline], target, &["/a~1b/~01/00/"]).0, 2);
}

#[test]
fn ambiguous_malformed_and_non_scalar_values_are_incomplete_even_among_valid_records() {
    for (problem, key) in [
        (r#"{"v":500,"v":200}"#, "ambiguous"),
        (r#"{"v":500,"\u0076":200}"#, "ambiguous"),
        (r#"{"v":[]}"#, "non_scalar"),
        (r#"{"v":{}}"#, "non_scalar"),
        (r#"{"v":500"#, "invalid_json"),
    ] {
        let target = format!("{{\"v\":200}}\n{problem}\n");
        let (code, r) = report(&[r#"{"v":200}"#], &target, &["/v"]);
        assert_eq!(code, 2, "{problem}");
        let field = &r["watched_fields"][0];
        assert_eq!(field["complete"], false);
        assert_eq!(field["target"][key], 1);
        assert_eq!(field["target"]["first_problem"]["line_no"], 2);
        assert!(field["values"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["is_new"].is_null()));
    }
    assert_eq!(
        report(
            &[r#"{"a":{"v":200},"a":{"v":500}}"#],
            r#"{"a":{"v":200}}"#,
            &["/a/v"]
        )
        .0,
        2
    );
}

#[test]
fn every_source_needs_an_observation_and_missing_is_not_null() {
    for baselines in [
        vec![""],
        vec!["plain log"],
        vec!["{\"other\":200}"],
        vec!["{\"v\":200}", ""],
    ] {
        assert_eq!(report(&baselines, "{\"v\":500}", &["/v"]).0, 2);
    }
    assert_eq!(report(&["{\"v\":200}"], "", &["/v"]).0, 2);
    let (code, r) = report(
        &["plain\n{}\n{\"v\":null}"],
        "plain\n{}\n{\"v\":null}",
        &["/v"],
    );
    assert_eq!(code, 0);
    assert_eq!(r["watched_fields"][0]["baselines"][0]["matched"], 1);
    assert_eq!(r["watched_fields"][0]["baselines"][0]["missing"], 1);
    assert_eq!(r["watched_fields"][0]["baselines"][0]["non_json"], 1);
}

#[test]
fn cardinality_and_size_limits_retain_evidence_and_fail_the_gate() {
    let many = (0..65)
        .map(|i| format!("{{\"v\":{i}}}\n"))
        .collect::<String>();
    let (code, r) = report(&["{\"v\":0}"], &many, &["/v"]);
    assert_eq!(code, 2);
    let field = &r["watched_fields"][0];
    assert_eq!(field["values"].as_array().unwrap().len(), 64);
    assert_eq!(field["target"]["matched"], 65);
    assert_eq!(field["target"]["untracked"], 1);
    assert_eq!(field["target"]["first_problem"]["line_no"], 65);
    let long = json!({"v":"界".repeat(4096)}).to_string();
    let (code, r) = report(&["{\"v\":0}"], &long, &["/v"]);
    assert_eq!(code, 2);
    assert_eq!(r["watched_fields"][0]["target"]["untracked"], 1);
    assert_eq!(
        r["watched_fields"][0]["target"]["first_problem"]["truncated"],
        true
    );
    let large = json!({"v":0,"extra":"x".repeat(1024 * 1024)}).to_string();
    let (code, r) = report(&["{\"v\":0}"], &large, &["/v"]);
    assert_eq!(code, 2);
    assert_eq!(r["watched_fields"][0]["target"]["oversized_records"], 1);
}

#[test]
fn envelopes_and_custom_masks_do_not_destroy_exact_observations() {
    let wrappers = [
        "2026-10-07T10:00:00Z {\"v\":VALUE}",
        "2026-10-07T10:00:00Z stdout F {\"v\":VALUE}",
        "Oct  7 10:00:00 host svc[123]: {\"v\":VALUE}",
        "{\"log\":\"{\\\"v\\\":VALUE}\\n\",\"stream\":\"stdout\",\"time\":\"2026-10-07T10:00:00Z\"}",
    ];
    for wrapper in wrappers {
        let (code, text) = diff(
            &[&wrapper.replace("VALUE", "200")],
            &wrapper.replace("VALUE", "503"),
            &["/v"],
            &["--json", "--mask", "[0-9]+"],
        );
        assert_eq!(code, 1);
        let r: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(r["watched_fields"][0]["values"][1]["value_json"], "503");
    }
}

#[test]
fn invalid_duplicate_and_excessive_selectors_fail_before_reporting() {
    for pointers in [
        vec![""],
        vec!["status"],
        vec!["/a~"],
        vec!["/a~2b"],
        vec!["/v", "/v"],
        vec!["/v"; 17],
    ] {
        assert_eq!(diff(&["{}"], "{}", &pointers, &["--json"]).0, 2);
    }
}

#[test]
fn terminal_markdown_and_json_show_findings_coverage_and_honest_incompleteness() {
    for format in ["--color", "--markdown", "--json"] {
        let args = if format == "--color" {
            vec![format, "never"]
        } else {
            vec![format]
        };
        let (code, text) = diff(&["{\"v\":200}"], "{\"v\":503}", &["/v"], &args);
        assert_eq!(code, 1);
        assert!(text.contains("503"));
        assert!(text.contains("200"));
        let (code, text) = diff(&["{\"v\":200}"], "{\"v\":200}", &["/typo"], &args);
        assert_eq!(code, 2);
        assert!(!text.contains("No significant differences"));
        assert!(text.to_lowercase().contains(if format == "--json" {
            "\"complete\": false"
        } else {
            "incomplete"
        }));
    }
    let (_, text) = diff(
        &["{\"v\":200}"],
        "before\n{\"v\":503}\nafter",
        &["/v"],
        &["--json", "-C", "1"],
    );
    let r: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        r["watched_fields"][0]["values"][1]["context"]["before"],
        json!([[1, "before"]])
    );
}
