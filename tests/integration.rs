//! End-to-end tests that exercise the compiled binary, mirroring how a user (or a CI job)
//! actually invokes it: real files under `examples/`, real stdin, a real gzip stream.

use std::io::Write;
use std::process::{Command, Stdio};

use assert_cmd::prelude::*;
use predicates::prelude::*;

fn fixture(name: &str) -> String {
    format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn cmd() -> Command {
    Command::cargo_bin("logdelta").unwrap()
}

#[test]
fn diff_human_output_snapshot() {
    let out = cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Absolute paths embed the checkout location; normalize before snapshotting.
    let normalized = stdout.replace(&fixture(""), "examples/");
    insta::assert_snapshot!(normalized);
}

#[test]
fn diff_markdown_output_snapshot() {
    let out = cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
            "--markdown",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let normalized = stdout.replace(&fixture(""), "examples/");
    insta::assert_snapshot!(normalized);
}

#[test]
fn diff_json_has_expected_shape() {
    let out = cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
            "--json",
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["findings"].as_array().unwrap().len() >= 5);
    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "new" && f["template"].as_str().unwrap().contains("FAILURES")));
    assert_eq!(v["target_total"], 23);
}

#[test]
fn templates_human_snapshot() {
    let out = cmd()
        .args(["templates", &fixture("k8s-service.log"), "--color", "never"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    insta::assert_snapshot!(stdout);
}

#[test]
fn templates_json_smoke() {
    cmd()
        .args(["templates", &fixture("npm-build.log"), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_lines\": 13"));
}

#[test]
fn novel_streams_only_unseen_templates_from_stdin() {
    let mut child = cmd()
        .args(["novel", "--baseline", &fixture("k8s-service.log")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        // First two lines match known baseline templates; the third is a novel panic line.
        writeln!(
            stdin,
            "2024-03-02T09:40:00.000000000Z stdout F 10.244.2.5 - - \"GET /healthz HTTP/1.1\" 200 2"
        )
        .unwrap();
        writeln!(
            stdin,
            "2024-03-02T09:40:01.000000000Z stdout F {{\"level\":\"info\",\"msg\":\"server listening\",\"addr\":\"0.0.0.0:8080\"}}"
        )
        .unwrap();
        writeln!(
            stdin,
            "2024-03-02T09:40:02.000000000Z stderr F panic: runtime error: invalid memory address or nil pointer dereference"
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one novel line, got: {lines:?}"
    );
    assert!(lines[0].contains("panic:"));
}

#[test]
fn gzip_input_is_transparently_decompressed() {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("access.log.gz");
    let raw = std::fs::read(fixture("k8s-service.log")).unwrap();
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = GzEncoder::new(file, Compression::default());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();

    cmd()
        .args(["templates", path.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_lines\": 10"));
}

#[test]
fn stdin_input_with_dash() {
    let mut child = cmd()
        .args(["templates", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"hello world\nhello world\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["total_lines"], 2);
    assert_eq!(v["clusters"][0]["count"], 2);
}

#[test]
fn context_flag_shows_surrounding_lines() {
    let out = cmd()
        .args([
            "diff",
            &fixture("k8s-service.log"),
            "--target",
            &fixture("k8s-service-incident.log"),
            "-C",
            "1",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("panic:"));
    // context line before the panic (the DB connection failure) should be present.
    assert!(stdout.contains("database connection failed"));
}

#[test]
fn diff_requires_at_least_two_inputs_without_target() {
    cmd()
        .args(["diff", &fixture("pytest-pass.log")])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least 2 files"));
}

#[test]
fn custom_mask_flag_collapses_matching_tokens() {
    let out = cmd()
        .args([
            "templates",
            &fixture("k8s-service.log"),
            "--mask",
            r"10\.244\.1\.\d+",
            "--json",
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let templates: Vec<String> = v["clusters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c["tokens"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert!(templates.iter().any(|t| t.contains("<CUSTOM>")));
}
