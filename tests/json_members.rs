//! Exercise the public tokenizer, library and installed-style CLI entrypoints.
use std::process::Command;

use assert_cmd::prelude::*;
use logdelta::analysis::{diff_lines, DiffOptions, FindingKind};
use logdelta::mask::tokenize_line;

const GOOD: &str = r#"{"message":"health check passed"}"#;

fn ambiguous_records() -> Vec<&'static str> {
    vec![
        r#"{"message":"database corruption detected","message":"health check passed"}"#,
        r#"{"message":"database corruption detected","\u006dessage":"health check passed"}"#,
        r#"{"detail":{"message":"database corruption detected","message":"health check passed"}}"#,
        r#"{"items":[{"message":"database corruption detected","message":"health check passed"}]}"#,
        r#"{"log":"database corruption detected\n","log":"health check passed\n","stream":"stdout","time":"2026-10-14T00:00:00Z"}"#,
        r#"{"log":"health check passed\n","stream":"stderr","stream":"stdout","time":"2026-10-14T00:00:00Z"}"#,
    ]
}

#[test]
fn ambiguous_json_never_erases_an_earlier_member() {
    for line in ambiguous_records() {
        let tokens = tokenize_line(line, &[]).join(" ");
        let signal = if line.contains("database") {
            "corruption"
        } else {
            "stderr"
        };
        assert!(
            tokens.contains(signal),
            "earlier member disappeared: {tokens}"
        );
        assert!(tokens.contains("health"));
    }
}

#[test]
fn the_library_and_cli_both_surface_the_hidden_message() {
    let bad = ambiguous_records()[0];
    let result = diff_lines(
        vec![(0..12).map(|_| Ok(GOOD.to_string()))],
        (0..12).map(|_| Ok(bad.to_string())),
        &[],
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(result
        .findings
        .iter()
        .any(|finding| finding.kind == FindingKind::New
            && finding
                .first_target_raw
                .as_deref()
                .is_some_and(|raw| raw.contains("corruption"))));

    let folder = tempfile::tempdir().unwrap();
    let baseline = folder.path().join("baseline.log");
    let target = folder.path().join("target.log");
    std::fs::write(&baseline, format!("{GOOD}\n").repeat(12)).unwrap();
    std::fs::write(&target, format!("{bad}\n").repeat(12)).unwrap();
    let output = Command::cargo_bin("logdelta")
        .unwrap()
        .arg("diff")
        .arg(&baseline)
        .arg(&target)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!report["findings"].as_array().unwrap().is_empty());
    assert!(report.to_string().contains("corruption"));
}

#[test]
fn repeated_names_in_separate_objects_and_string_literals_are_not_duplicates() {
    let json =
        r#"{"items":[{"message":"first"},{"message":"second"}],"literal":"message message"}"#;
    let tokens = tokenize_line(json, &[]).join(" ");
    assert!(
        tokens.starts_with("items="),
        "valid JSON unexpectedly used raw fallback: {tokens}"
    );
    assert!(tokens.contains("first") && tokens.contains("second"));
}
