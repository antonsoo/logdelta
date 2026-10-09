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
fn large_field_context_is_bounded_and_clipping_is_visible_without_changing_values() {
    let line = |v| json!({"v":v,"padding":"界".repeat(3000)}).to_string();
    let baseline = line(0);
    let target = format!("{}\n{}\n{}", line(0), line(1), line(0));
    let (code, text) = diff(&[&baseline], &target, &["/v"], &["--json", "-C", "2"]);
    assert_eq!(code, 1);
    let report: Value = serde_json::from_str(&text).unwrap();
    let value = &report["watched_fields"][0]["values"][1];
    assert_eq!(value["value_json"], "1");
    assert_eq!(value["is_new"], true);
    assert_eq!(value["first_target"]["line_no"], 2);
    assert_eq!(value["context_truncated"], true);
    let context = &value["context"];
    assert_eq!(context["before"][0][0], 1);
    assert_eq!(context["after"][0][0], 3);
    let bytes: usize = ["before", "after"]
        .iter()
        .flat_map(|side| context[side].as_array().unwrap())
        .map(|row| row[1].as_str().unwrap().len())
        .sum();
    assert!(bytes <= 8192);
    let target = (0..31)
        .map(|i| format!("{{\"v\":{}}}", u8::from(i == 15)))
        .collect::<Vec<_>>()
        .join("\n");
    let (_, text) = diff(&["{\"v\":0}"], &target, &["/v"], &["--json", "-C", "20"]);
    let report: Value = serde_json::from_str(&text).unwrap();
    let value = &report["watched_fields"][0]["values"][1];
    assert_eq!(value["context_truncated"], true);
    assert_eq!(value["context"]["before"].as_array().unwrap().len(), 10);
    assert_eq!(value["context"]["before"][0], json!([6, "{\"v\":0}"]));
    assert_eq!(value["context"]["after"].as_array().unwrap().len(), 10);
    assert_eq!(value["context"]["after"][0], json!([17, "{\"v\":0}"]));
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

fn grouped_report(baselines: &[&str], target: &str, by: &[&str]) -> (i32, Value) {
    let mut args = vec!["--json"];
    for pointer in by {
        args.extend(["--watch-by", pointer]);
    }
    let (code, text) = diff(baselines, target, &["/v"], &args);
    (code, serde_json::from_str(&text).unwrap())
}

#[test]
fn recorded_http_responses_expose_a_route_failure_hidden_by_pooled_statuses() {
    let good = include_str!("../examples/http-routes-good.log");
    let other = include_str!("../examples/http-routes-good-2.log");
    let failed = include_str!("../examples/http-routes-failed.log");
    // The capture script checks these emitted records against the actual HTTP responses.
    let (pooled_code, pooled) = report(&[good, other], failed, &["/http/status"]);
    assert_eq!(pooled_code, 0);
    assert!(pooled["watched_fields"][0].get("group_by").is_none());
    let (code, text) = diff(
        &[good, other],
        failed,
        &["/http/status"],
        &["--watch-by", "/route", "--json", "-C", "1"],
    );
    assert_eq!(code, 1);
    let grouped: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(grouped["findings"], json!([]));
    assert_eq!(grouped["value_findings"], json!([]));
    let field = &grouped["watched_fields"][0];
    assert_eq!(field["complete"], true);
    assert_eq!(field["group_by"], json!(["/route"]));
    let values = field["values"].as_array().unwrap();
    assert_eq!(values.len(), 3);
    let new: Vec<_> = values.iter().filter(|v| v["is_new"] == true).collect();
    assert_eq!(new.len(), 1);
    let value = new[0];
    assert_eq!(value["group_values_json"], json!(["\"/checkout\""]));
    assert_eq!(value["group_seen_in_baseline"], true);
    assert_eq!(value["value_json"], "503");
    assert_eq!(value["baseline_counts"], json!([0, 0]));
    assert_eq!(value["target_count"], 2);
    assert_eq!(value["first_target"]["line_no"], 14);
    assert_eq!(value["context"]["before"][0][0], 13);
    assert_eq!(values[2]["group_values_json"], json!(["\"/maintenance\""]));
    assert_eq!(values[2]["baseline_counts"], json!([20, 20]));
    assert_eq!(values[2]["target_count"], 20);
    assert_eq!(values[2]["is_new"], false);
    assert_eq!(
        diff(
            &[good],
            other,
            &["/http/status"],
            &["--watch-by", "/route", "--json"]
        )
        .0,
        0
    );
}

#[test]
fn groups_use_the_union_of_baselines_and_distinguish_new_keys_from_changed_outcomes() {
    let (code, report) = grouped_report(
        &["{\"g\":\"a\",\"v\":200}", "{\"g\":\"b\",\"v\":503}"],
        "{\"g\":\"a\",\"v\":503}\n{\"g\":\"b\",\"v\":503}\n{\"g\":\"c\",\"v\":503}",
        &["/g"],
    );
    assert_eq!(code, 1);
    let values = report["watched_fields"][0]["values"].as_array().unwrap();
    assert_eq!(values[1]["group_seen_in_baseline"], true);
    assert_eq!(values[1]["is_new"], true);
    assert_eq!(values[2]["baseline_counts"], json!([0, 1]));
    assert_eq!(values[2]["first_baseline"]["baseline_index"], 1);
    assert_eq!(values[2]["is_new"], false);
    assert_eq!(values[3]["group_seen_in_baseline"], false);
    assert_eq!(values[3]["is_new"], true);
    // Every run needs a usable observation, but an individual key need not occur in every run.
    assert_eq!(report["watched_fields"][0]["complete"], true);
}

#[test]
fn composite_group_keys_preserve_boundaries_types_number_spelling_and_decoded_strings() {
    let baseline = r#"{"x":"a|b","y":"c","v":0}
{"x":"a","y":"b|c","v":1}
{"x":9007199254740992,"y":null,"v":0}
{"x":200,"y":false,"v":0}
{"x":"é","y":"\u0061","v":0}"#;
    let target = r#"{"x":"a|b","y":"c","v":1}
{"x":"a","y":"b|c","v":1}
{"x":9007199254740993,"y":null,"v":0}
{"x":200.0,"y":false,"v":0}
{"x":"200","y":false,"v":0}
{"x":"\u00e9","y":"a","v":0}"#;
    let (code, report) = grouped_report(&[baseline], target, &["/x", "/y"]);
    assert_eq!(code, 1);
    let values = report["watched_fields"][0]["values"].as_array().unwrap();
    let changed = values
        .iter()
        .find(|v| v["group_values_json"] == json!(["\"a|b\"", "\"c\""]) && v["value_json"] == "1")
        .unwrap();
    assert_eq!(changed["is_new"], true);
    assert_eq!(changed["group_seen_in_baseline"], true);
    let new_groups: Vec<_> = values
        .iter()
        .filter(|v| v["group_seen_in_baseline"] == false)
        .map(|v| &v["group_values_json"])
        .collect();
    assert_eq!(
        new_groups,
        vec![
            &json!(["\"200\"", "false"]),
            &json!(["200.0", "false"]),
            &json!(["9007199254740993", "null"])
        ]
    );
    let decoded = values
        .iter()
        .find(|v| v["group_values_json"] == json!(["\"é\"", "\"a\""]))
        .unwrap();
    assert_eq!(decoded["baseline_counts"], json!([1]));
    assert_eq!(decoded["target_count"], 1);
    assert_eq!(decoded["is_new"], false);
}

#[test]
fn unassignable_selected_records_make_novelty_unknown_but_unrelated_events_do_not() {
    let good = "{\"g\":null,\"v\":200}";
    for (problem, reason) in [
        (r#"{"v":503}"#, "group_missing"),
        (r#"{"g":[],"v":503}"#, "group_non_scalar"),
        (r#"{"g":{},"v":503}"#, "group_non_scalar"),
        (r#"{"g":"one","\u0067":"two","v":503}"#, "group_ambiguous"),
    ] {
        // Test both sides: a dropped baseline record can falsely imply a new target value.
        for baseline_problem in [false, true] {
            let mixed = format!("{good}\n{problem}");
            let (baselines, target) = if baseline_problem {
                (vec![mixed.as_str()], good)
            } else {
                (vec![good], mixed.as_str())
            };
            let (code, report) = grouped_report(&baselines, target, &["/g"]);
            assert_eq!(code, 2, "{problem}");
            let field = &report["watched_fields"][0];
            assert_eq!(field["complete"], false);
            let coverage = if baseline_problem {
                &field["baselines"][0]
            } else {
                &field["target"]
            };
            assert_eq!(coverage[reason], 1);
            assert_eq!(coverage["first_problem"]["line_no"], 2);
            assert_eq!(coverage["matched"], 1);
            for value in field["values"].as_array().unwrap() {
                assert!(value["is_new"].is_null());
                assert!(value.get("group_seen_in_baseline").is_none());
            }
        }
    }
    let unrelated = format!("{good}\nplain text\n{{\"g\":[]}}\n{{}}");
    let (code, report) = grouped_report(&[&unrelated], &unrelated, &["/g"]);
    assert_eq!(code, 0);
    assert_eq!(report["watched_fields"][0]["target"]["missing"], 2);
    assert_eq!(
        report["watched_fields"][0]["values"][0]["group_values_json"],
        json!(["null"])
    );
    assert_eq!(grouped_report(&[good, "{}"], good, &["/g"]).0, 2);
}

#[test]
fn grouped_limits_bound_pairs_and_total_key_bytes_without_erasing_retained_counts() {
    let target = (0..257)
        .chain([0])
        .map(|i| format!("{{\"g\":{i},\"v\":200}}\n"))
        .collect::<String>();
    let (code, report) = grouped_report(&["{\"g\":0,\"v\":200}"], &target, &["/g"]);
    assert_eq!(code, 2);
    let field = &report["watched_fields"][0];
    assert_eq!(field["values"].as_array().unwrap().len(), 256);
    assert_eq!(field["target"]["untracked"], 1);
    assert_eq!(field["target"]["matched"], 258);
    assert_eq!(field["target"]["first_problem"]["line_no"], 257);
    assert_eq!(field["values"][0]["target_count"], 2);
    assert!(field["values"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["is_new"].is_null()));
    // The bound is shared by all components, not 4 KiB for each of four components.
    let large = json!({"g": "界".repeat(800), "h": "界".repeat(800), "v": 200}).to_string();
    let (code, report) = grouped_report(&[r#"{"g":0,"h":0,"v":200}"#], &large, &["/g", "/h"]);
    assert_eq!(code, 2);
    assert_eq!(report["watched_fields"][0]["target"]["untracked"], 1);
    assert_eq!(
        report["watched_fields"][0]["target"]["first_problem"]["truncated"],
        true
    );
}

#[test]
fn grouping_runs_before_masks_and_uses_the_same_envelope_and_pointer_rules() {
    let wrapper = |g| {
        json!({"log": format!("{{\"a/b\":{{\"~key\":[{g}]}},\"v\":200}}\n"), "stream": "stdout"})
            .to_string()
    };
    let good = wrapper("9007199254740992");
    let bad = wrapper("9007199254740993");
    let (code, text) = diff(
        &[&good],
        &bad,
        &["/v"],
        &["--watch-by", "/a~1b/~0key/0", "--mask", "[0-9]+", "--json"],
    );
    assert_eq!(code, 1);
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        report["watched_fields"][0]["values"][1]["group_values_json"],
        json!(["9007199254740993"])
    );
    assert_eq!(
        report["watched_fields"][0]["values"][1]["group_seen_in_baseline"],
        false
    );
}

#[test]
fn grouping_configuration_and_human_reports_cannot_silently_drop_the_requested_scope() {
    for by in [
        vec!["g"],
        vec!["/bad~"],
        vec!["/g", "/g"],
        vec!["/g", "/h", "/i", "/j", "/k"],
    ] {
        let mut args = vec!["--json"];
        for p in by {
            args.extend(["--watch-by", p]);
        }
        let (code, text) = diff(&["{}"], "{}", &["/v"], &args);
        assert_eq!(code, 2);
        assert!(text.contains("--watch-by"));
    }
    assert_eq!(diff(&["{}"], "{}", &[], &["--watch-by", "/g"]).0, 2);
    let baseline = r#"{"g":"old","v":200}"#;
    let target = r#"{"g":"old","v":503}
{"g":"<b>|new","v":503}"#;
    for format in ["--markdown", "--json", "--color"] {
        let mut args = vec!["--watch-by", "/g", format];
        if format == "--color" {
            args.push("never");
        }
        let (code, text) = diff(&[baseline], target, &["/v"], &args);
        assert_eq!(code, 1);
        assert!(text.contains("old"));
        assert!(text.contains("503"));
        if format != "--json" {
            assert!(text.contains("NEW GROUP"));
        }
        if format == "--markdown" {
            assert!(text.contains("&lt;b&gt;&#124;new"));
            assert!(!text.contains("<b>"));
        }
    }
}
