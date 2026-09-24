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

#[test]
fn mask_file_supports_named_and_bare_patterns_and_skips_comments() {
    let dir = tempfile::tempdir().unwrap();
    let mask_path = dir.path().join("masks.txt");
    std::fs::write(
        &mask_path,
        "# comment, and a blank line follow\n\nPOD=pod-[a-z0-9]+\n10\\.244\\.1\\.\\d+\n",
    )
    .unwrap();

    let out = cmd()
        .args([
            "templates",
            &fixture("k8s-service.log"),
            "--mask-file",
            mask_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    // The bare pattern falls back to the generic <CUSTOM> placeholder.
    assert!(templates.iter().any(|t| t.contains("<CUSTOM>")));
}

#[test]
fn mask_file_reports_the_path_and_line_on_a_bad_regex() {
    let dir = tempfile::tempdir().unwrap();
    let mask_path = dir.path().join("bad.txt");
    std::fs::write(&mask_path, "fine-one\n[unterminated\n").unwrap();

    cmd()
        .args([
            "templates",
            &fixture("k8s-service.log"),
            "--mask-file",
            mask_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("bad.txt:2"));
}

#[test]
fn novel_exit_code_reflects_whether_anything_was_printed() {
    // Nothing novel: target is identical to the baseline.
    cmd()
        .args([
            "novel",
            "--baseline",
            &fixture("k8s-service.log"),
            &fixture("k8s-service.log"),
        ])
        .assert()
        .success();

    // Something novel: the incident log has lines the baseline never saw.
    cmd()
        .args([
            "novel",
            "--baseline",
            &fixture("k8s-service.log"),
            &fixture("k8s-service-incident.log"),
        ])
        .assert()
        .code(1);
}

#[test]
fn color_always_emits_ansi_even_when_not_a_tty() {
    let out = cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
            "--color",
            "always",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("\x1b["),
        "expected ANSI escapes in: {stdout}"
    );
}

#[test]
fn color_defaults_to_plain_when_not_a_tty() {
    let out = cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("\x1b["),
        "expected no ANSI escapes in: {stdout}"
    );
}

#[test]
fn threshold_and_significance_are_range_checked() {
    cmd()
        .args(["templates", &fixture("k8s-service.log"), "--threshold", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--threshold"));

    cmd()
        .args([
            "diff",
            &fixture("pytest-pass.log"),
            "--target",
            &fixture("pytest-fail.log"),
            "--significance=-1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--significance"));
}

#[test]
fn lower_threshold_merges_more_lines_into_fewer_templates() {
    let strict = cmd()
        .args([
            "templates",
            &fixture("pytest-fail.log"),
            "--threshold",
            "0.9",
            "--json",
        ])
        .output()
        .unwrap();
    let loose = cmd()
        .args([
            "templates",
            &fixture("pytest-fail.log"),
            "--threshold",
            "0.1",
            "--json",
        ])
        .output()
        .unwrap();
    let strict_v: serde_json::Value = serde_json::from_slice(&strict.stdout).unwrap();
    let loose_v: serde_json::Value = serde_json::from_slice(&loose.stdout).unwrap();
    let strict_count = strict_v["clusters"].as_array().unwrap().len();
    let loose_count = loose_v["clusters"].as_array().unwrap().len();
    assert!(
        loose_count <= strict_count,
        "loose={loose_count} strict={strict_count}"
    );
}
