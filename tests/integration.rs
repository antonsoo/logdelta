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

/// Runs from the crate root with relative paths, so a snapshot shows `examples/...` wherever
/// the repository is checked out (a long absolute path used to be clipped before it could be
/// normalized away).
fn cmd_in_crate() -> Command {
    let mut c = cmd();
    c.current_dir(env!("CARGO_MANIFEST_DIR"));
    c
}

#[test]
fn diff_human_output_snapshot() {
    let out = cmd_in_crate()
        .args([
            "diff",
            "examples/pytest-pass.log",
            "--target",
            "examples/pytest-fail.log",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    insta::assert_snapshot!(stdout);
}

#[test]
fn diff_markdown_output_snapshot() {
    let out = cmd_in_crate()
        .args([
            "diff",
            "examples/pytest-pass.log",
            "--target",
            "examples/pytest-fail.log",
            "--markdown",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    insta::assert_snapshot!(stdout);
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
    assert!(v["total_templates"].as_u64().unwrap() > 0);

    // The pytest status word flip (PASSED -> FAILED) is a same-count, same-position content
    // change frequency scoring alone can't see; it should show up as a NEW VALUE finding.
    let value_findings = v["value_findings"].as_array().unwrap();
    assert!(value_findings.iter().any(|f| f["new_value"] == "FAILED"
        && f["baseline_values"].as_array().unwrap() == &[serde_json::json!("PASSED")]));
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

#[test]
fn exits_cleanly_on_a_closed_stdout_pipe() {
    // `logdelta ... | head` closes its read end once it has what it wants; our next write
    // then fails with EPIPE. A well-behaved Unix tool exits 0 quietly instead of printing
    // "Broken pipe" and failing - see main.rs's `is_broken_pipe`.
    let mut child = cmd()
        .args([
            "templates",
            &fixture("large/baseline-2.log"),
            "-n",
            "1000",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Drop our end of the read pipe without reading anything: once the child's output
    // exceeds the OS pipe buffer (a few dozen KB; this command's JSON output is well over
    // that), its next write hits EPIPE.
    drop(child.stdout.take());

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected exit 0 on a closed pipe, got {:?}; stderr: {stderr}",
        output.status
    );
    assert!(
        !stderr.to_lowercase().contains("broken pipe"),
        "stderr should not mention the broken pipe, got: {stderr}"
    );
}

fn total_finding_count(v: &serde_json::Value) -> usize {
    v["findings"].as_array().unwrap().len() + v["value_findings"].as_array().unwrap().len()
}

#[test]
fn two_passing_runs_produce_almost_no_findings() {
    // A user's first try at `diff` is often two known-good runs, before they even have a
    // failing one to compare against - that has to be near-silent, or the tool looks broken
    // on the very first thing someone tries. Every pair of the three large synthetic
    // baselines is a real pass-vs-pass run of the same fixed test suite.
    let pairs = [
        ("large/baseline-1.log", "large/baseline-2.log"),
        ("large/baseline-1.log", "large/baseline-3.log"),
        ("large/baseline-2.log", "large/baseline-3.log"),
        ("large/baseline-2.log", "large/baseline-1.log"),
        ("large/baseline-3.log", "large/baseline-1.log"),
        ("large/baseline-3.log", "large/baseline-2.log"),
    ];
    for (baseline, target) in pairs {
        let out = cmd()
            .args([
                "diff",
                &fixture(baseline),
                "--target",
                &fixture(target),
                "--json",
            ])
            .output()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let count = total_finding_count(&v);
        assert!(
            count <= 2,
            "{baseline} vs {target}: expected at most 2 findings on a pass-vs-pass diff, got {count}: {v}"
        );
    }
}

#[test]
fn real_failure_diff_still_flags_new_gone_and_the_content_flip() {
    // The same three baselines against the run with an actual injected failure must still
    // surface it clearly: this is the tightened NEW VALUE logic's other half of the bargain
    // (near-silent on clean pairs, but not silent on a real regression).
    let out = cmd()
        .args([
            "diff",
            &fixture("large/baseline-1.log"),
            &fixture("large/baseline-2.log"),
            &fixture("large/baseline-3.log"),
            "--target",
            &fixture("large/target-failure.log"),
            "--json",
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "new"));
    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "gone"));
    assert!(
        v["value_findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["new_value"] == "FAILED"
                && f["baseline_values"] == serde_json::json!(["PASSED"])),
        "expected the PASSED -> FAILED content flip, got: {}",
        v["value_findings"]
    );
}

#[test]
fn pytest_pass_vs_fail_still_flags_the_content_flip() {
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
    assert!(
        v["value_findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["new_value"] == "FAILED"
                && f["baseline_values"] == serde_json::json!(["PASSED"])),
        "expected the PASSED -> FAILED content flip on the small pytest fixture, got: {}",
        v["value_findings"]
    );
}

/// One line of 120 KB (a base64 payload) is a single finding. The terminal and Markdown
/// reports must stay readable around it; `--json` keeps the line whole.
#[test]
fn a_huge_line_is_clipped_in_reports_and_whole_in_json() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let ordinary = "INFO start\nINFO request ok\n".repeat(50);
    let payload = "QUJD".repeat(30_000);
    std::fs::write(&base, &ordinary).unwrap();
    std::fs::write(
        &fail,
        format!("{ordinary}ERROR upload failed payload={payload} retry=3\n"),
    )
    .unwrap();
    let run = |extra: &[&str]| {
        let out = cmd()
            .args(["diff", base.to_str().unwrap(), fail.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };

    let human = run(&["--color", "never"]);
    assert!(human.len() < 3_000, "{} bytes", human.len());
    assert!(human.contains("ERROR upload failed payload=QUJD"));
    assert!(human.contains("more characters)"));

    let templates = String::from_utf8(
        cmd()
            .args(["templates", fail.to_str().unwrap(), "--color", "never"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(templates.len() < 3_000, "{} bytes", templates.len());

    let markdown = run(&["--markdown"]);
    assert!(markdown.len() < 3_000, "{} bytes", markdown.len());
    assert!(markdown.contains("more characters)"));

    let json = run(&["--json"]);
    assert!(json.contains(&payload));
}

/// A baseline of ordinary request lines and a target where one request ends in a stack
/// trace, written to a temporary directory. Returns the directory (keep it alive) and both
/// paths.
fn stack_trace_logs(copies: usize) -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let mut ordinary = String::new();
    for i in 0..40 {
        ordinary.push_str(&format!("INFO request {i} served in {}ms\n", 10 + i % 7));
    }
    let trace = "\
ERROR unhandled exception in worker
Traceback (most recent call last):
  File \"/srv/app/worker.py\", line 88, in run
    result = handler(job)
  File \"/srv/app/handlers.py\", line 41, in handler
    return charge(job.account, job.amount)
PaymentDeclined: card ending 4242 was declined
";
    std::fs::write(&base, &ordinary).unwrap();
    let mut target = ordinary.clone();
    for _ in 0..copies {
        target.push_str(trace);
        target.push_str(&ordinary);
    }
    std::fs::write(&fail, target).unwrap();
    (
        dir,
        base.to_str().unwrap().to_string(),
        fail.to_str().unwrap().to_string(),
    )
}

fn stdout_of(args: &[&str]) -> String {
    String::from_utf8(cmd().args(args).output().unwrap().stdout).unwrap()
}

#[test]
fn a_stack_trace_is_one_finding_and_flat_lists_its_lines() {
    let (_dir, base, fail) = stack_trace_logs(1);

    let grouped = stdout_of(&["diff", &base, &fail, "--color", "never"]);
    assert!(
        grouped.contains("· 1 finding (6 with --flat)"),
        "header should count the block once and say what --flat would list:\n{grouped}"
    );
    assert!(grouped.contains("NEW        6 templates · 7 lines"));
    assert!(grouped.contains(&format!("{fail}:41-47")));
    // One line per template, numbered, as the log has it; the two `File` lines are one
    // template and say so.
    assert!(grouped.contains("    41 | ERROR unhandled exception in worker"));
    assert!(grouped.contains("    43 |   File \"/srv/app/worker.py\", line 88, in run  ×2"));
    assert!(grouped.contains("    47 | PaymentDeclined: card ending 4242 was declined"));
    assert_eq!(grouped.matches("\nNEW").count(), 1);

    let flat = stdout_of(&["diff", &base, &fail, "--color", "never", "--flat"]);
    assert!(flat.contains("· 6 findings\n"), "{flat}");
    assert_eq!(flat.matches("\nNEW").count(), 6);
    assert!(!flat.contains("with --flat") && !flat.contains(" | "));

    // Either way there is something to report, and the exit status says so.
    for extra in [&[][..], &["--flat"][..]] {
        cmd()
            .args(["diff", &base, &fail])
            .args(extra)
            .assert()
            .code(1);
    }
}

#[test]
fn a_stack_trace_logged_again_and_again_is_still_one_finding() {
    let (_dir, base, fail) = stack_trace_logs(25);
    let grouped = stdout_of(&["diff", &base, &fail, "--color", "never"]);
    assert!(grouped.contains("· 1 finding (6 with --flat)"), "{grouped}");
    // 25 copies of 7 lines; the block points at the first and counts the rest.
    assert!(grouped.contains("NEW        6 templates · 175 lines"));
    assert!(grouped.contains(":41-47 (+168 of these lines further on)"));
    assert!(grouped.contains("    47 | PaymentDeclined: card ending 4242 was declined  ×25"));
}

#[test]
fn json_names_each_block_and_the_findings_in_it() {
    let (_dir, base, fail) = stack_trace_logs(1);
    let v: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", &base, &fail, "--json"])).unwrap();
    let blocks = v["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block["kind"], "new");
    assert_eq!(block["first_line_no"], 41);
    assert_eq!(block["last_line_no"], 47);
    assert_eq!(block["line_count"], 7);
    assert_eq!(block["lines_elsewhere"], 0);
    let members: Vec<usize> = block["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_u64().unwrap() as usize)
        .collect();
    // Every finding is still listed; each says which block it is part of.
    let findings = v["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 6);
    assert_eq!(members.len(), 6);
    let lines: Vec<u64> = members
        .iter()
        .map(|&i| findings[i]["first_target_line_no"].as_u64().unwrap())
        .collect();
    assert_eq!(
        lines,
        vec![41, 42, 43, 44, 46, 47],
        "members are in line order; line 45 is the second `File` line"
    );
    assert!(findings.iter().all(|f| f["block"] == 0));
    let score: f64 = findings.iter().map(|f| f["score"].as_f64().unwrap()).sum();
    assert!((block["score"].as_f64().unwrap() - score).abs() < 1e-9);

    // --flat turns the grouping off in the data too.
    let flat: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", &base, &fail, "--json", "--flat"])).unwrap();
    assert_eq!(flat["blocks"], serde_json::json!([]));
    assert!(flat["findings"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f.get("block").is_none()));
    assert_eq!(flat["findings"].as_array().unwrap().len(), 6);
}

#[test]
fn block_lines_limits_how_much_of_a_long_block_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let ordinary = "INFO tick\n".repeat(30);
    let mut target = ordinary.clone();
    let words = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra",
        "tango",
    ];
    for word in words {
        target.push_str(&format!("{word} step failed\n"));
    }
    std::fs::write(&base, &ordinary).unwrap();
    std::fs::write(&fail, &target).unwrap();
    let (base, fail) = (base.to_str().unwrap(), fail.to_str().unwrap());

    // Twenty distinct lines: by default the first eight and the last four.
    let default = stdout_of(&["diff", base, fail, "--color", "never"]);
    assert!(
        default.contains("NEW        20 templates · 20 lines"),
        "{default}"
    );
    assert!(default.contains("    38 | hotel step failed\n"));
    assert!(default.contains("⋯ 8 more\n"));
    assert!(!default.contains("india step failed"));
    assert!(default.contains("    47 | quebec step failed\n"));
    assert!(default.contains("    50 | tango step failed\n"));

    let all = stdout_of(&["diff", base, fail, "--color", "never", "--block-lines", "0"]);
    assert!(all.contains("india step failed") && !all.contains("⋯"));

    let three = stdout_of(&["diff", base, fail, "--color", "never", "--block-lines", "3"]);
    assert!(three.contains("    32 | bravo step failed\n"));
    assert!(three.contains("⋯ 17 more\n"));
    assert!(three.contains("    50 | tango step failed\n"));
    assert!(!three.contains("charlie step failed"));

    let markdown = stdout_of(&["diff", base, fail, "--markdown", "--block-lines", "3"]);
    // `INFO tick` is every baseline line and 30 of 50 here: a smaller share, the same
    // count, and so not a finding.
    assert!(
        markdown.contains("1 finding (20 before grouping)"),
        "{markdown}"
    );
    assert!(markdown.contains(&format!("**20 templates, 20 lines** at `{fail}:31-50`")));
    assert!(markdown.contains("```text\n    31 | alpha step failed\n"));
    assert!(markdown.contains("       ⋯ 17 more\n    50 | tango step failed\n```\n"));
}

#[test]
fn skipped_steps_are_one_gone_block_that_points_into_the_baseline() {
    // A deploy job whose target run stops after the build: the three later steps, each a
    // few lines of its own between lines every step prints, are gone.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let step = |name: &str, lines: &[&str]| {
        let mut s = format!("##[group]Run {name}\nshell: /usr/bin/bash -e {{0}}\nenv:\n  CI: true\n  NODE_ENV: production\n##[endgroup]\n");
        for line in lines {
            s.push_str(line);
            s.push('\n');
        }
        s
    };
    let build = step(
        "build",
        &["compiling 214 modules", "bundle written to dist/"],
    );
    let later = [
        step(
            "test",
            &[
                "running 96 tests",
                "96 passed",
                "coverage 91.2%",
                "report written",
            ],
        ),
        step(
            "package",
            &["archiving dist/", "checksum computed", "archive ready"],
        ),
        step(
            "deploy",
            &[
                "uploading archive",
                "release created",
                "smoke test passed",
                "done",
            ],
        ),
    ]
    .concat();
    std::fs::write(&base, format!("{build}{later}")).unwrap();
    std::fs::write(&fail, format!("{build}ERROR build output missing\n")).unwrap();
    let (base, fail) = (base.to_str().unwrap(), fail.to_str().unwrap());

    let out = stdout_of(&["diff", base, fail, "--color", "never", "--block-lines", "0"]);
    // The eleven lines of output of the three steps, in one block located in the baseline.
    assert!(out.contains("GONE       11 templates · 11 lines"), "{out}");
    assert!(out.contains(&format!("{base}:15-37")));
    assert!(out.contains("    15 | running 96 tests\n"));
    assert!(out.contains("    37 | done\n"));
    assert_eq!(out.matches("\nGONE").count(), 1);
    // The lines every step prints are still in the target and are not reported.
    assert!(!out.contains("NODE_ENV"));

    // On its own, a gone template shows the baseline line it stands for.
    let flat = stdout_of(&["diff", base, fail, "--color", "never", "--flat"]);
    assert_eq!(flat.matches("\nGONE").count(), 11);
    assert!(
        flat.contains(&format!("{base}:37\n        done\n")),
        "{flat}"
    );

    let markdown = stdout_of(&["diff", base, fail, "--markdown", "--flat"]);
    assert!(markdown.contains("| Template | Last seen |"));
    assert!(markdown.contains(&format!("`{base}:37` `done`")));
}

#[test]
fn reports_show_lines_without_their_escape_sequences() {
    // A colored CI log: every line of the failure is wrapped in SGR codes, and the reports
    // must neither pass those on to the reader's terminal nor show them as text.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let ordinary = "\x1b[32mok\x1b[0m compile step finished\n".repeat(20);
    std::fs::write(&base, &ordinary).unwrap();
    std::fs::write(
        &fail,
        format!(
            "{ordinary}\x1b[31m\x1b[1mFAILED\x1b[0m tests/test_api.py::test_login\n\
             \x1b[31mE   AssertionError: expected 200\x1b[0m\n\
             \x1b[36mhint:\x1b[0m rerun with -x\n\
             {ordinary}\x1b[33mwarning\x1b[0m: \x1b]8;;file:///src/a.rs\x1b\\src/a.rs\x1b]8;;\x1b\\ is unused\n"
        ),
    )
    .unwrap();
    let (base, fail) = (base.to_str().unwrap(), fail.to_str().unwrap());

    for extra in [
        &["--color", "never"][..],
        &["--color", "never", "--flat"][..],
        &["--color", "never", "-C", "2"][..],
        &["--markdown"][..],
    ] {
        let out = stdout_of(&[&["diff", base, fail][..], extra].concat());
        assert!(
            !out.contains('\x1b'),
            "{extra:?} passed an escape through:\n{out}"
        );
        assert!(
            out.contains("FAILED tests/test_api.py::test_login"),
            "{out}"
        );
        assert!(out.contains("E   AssertionError: expected 200"));
        assert!(out.contains("warning: src/a.rs is unused"));
    }
    let templates = stdout_of(&["templates", fail, "--color", "never"]);
    assert!(!templates.contains('\x1b'), "{templates}");

    // With color on, the only escapes are logdelta's own styling: the log's red is gone.
    let colored = stdout_of(&["diff", base, fail, "--color", "always"]);
    assert!(!colored.contains("\x1b[31m\x1b[1mFAILED"));
    assert!(!colored.contains("\x1b]8;;"));

    // The data keeps every line as it was read.
    let v: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", base, fail, "--json"])).unwrap();
    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["first_target_raw"]
            == "\x1b[31m\x1b[1mFAILED\x1b[0m tests/test_api.py::test_login"));
}

#[test]
fn a_new_value_inside_a_block_is_part_of_the_block() {
    // The middle line of the failure fits a template the baseline has
    // (`> fetch <*> from cache <*>`), with a value it never had there. It is a line of the
    // traceback, not a finding of its own.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.log");
    let fail = dir.path().join("fail.log");
    let mut ordinary = String::new();
    for package in ["numpy", "pandas", "scipy", "attrs", "pluggy", "iniconfig"] {
        ordinary.push_str(&format!("> fetch {package} from cache ok\n"));
        ordinary.push_str("resolved in 12ms\n");
    }
    std::fs::write(&base, &ordinary).unwrap();
    std::fs::write(
        &fail,
        format!(
            "{ordinary}def charge(account):\n> fetch ledger from cache failed\nE   PaymentDeclined: no funds\n"
        ),
    )
    .unwrap();
    let (base, fail) = (base.to_str().unwrap(), fail.to_str().unwrap());

    let v: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", base, fail, "--json"])).unwrap();
    let values = v["value_findings"].as_array().unwrap();
    assert!(
        values
            .iter()
            .any(|f| f["new_value"] == "failed" && f["block"] == 0),
        "expected the `failed` value, marked as inside block 0: {values:?}"
    );
    let out = stdout_of(&["diff", base, fail, "--color", "never"]);
    assert!(out.contains("NEW        2 templates · 2 lines"), "{out}");
    assert!(!out.contains("NEW VALUE"), "{out}");
    // Ungrouped, it is listed like any other new value.
    let flat = stdout_of(&["diff", base, fail, "--color", "never", "--flat"]);
    assert!(
        flat.contains("NEW VALUE  > fetch <*> from cache failed"),
        "{flat}"
    );
}

/// The JSON report of `baseline` against `target`, with the paths (which differ between the
/// copies under test) taken out.
fn diff_json_without_paths(baseline: &str, target: &str) -> serde_json::Value {
    let out = cmd()
        .args(["diff", baseline, "--target", target, "--json"])
        .output()
        .unwrap();
    // Like diff(1): 1 says there are findings; anything above is an error.
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout)
        .unwrap()
        .replace(baseline, "BASELINE")
        .replace(target, "TARGET");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn logs_saved_by_windows_tools_give_the_same_report() {
    // `pytest > run.log` in Windows PowerShell writes UTF-16 with a byte-order mark and CRLF
    // line endings; other tools put a mark in front of UTF-8. The mark used to make the
    // first line a template of its own, and a UTF-16 target shared no line with a UTF-8
    // baseline: every line of it was reported as new.
    let dir = tempfile::tempdir().unwrap();
    let plain_pass = fixture("pytest-pass.log");
    let plain_fail = fixture("pytest-fail.log");
    let expected = diff_json_without_paths(&plain_pass, &plain_fail);
    assert!(total_finding_count(&expected) > 0);

    let save = |name: &str, source: &str, encode: &dyn Fn(&str) -> Vec<u8>| -> String {
        let text = std::fs::read_to_string(source)
            .unwrap()
            .replace('\n', "\r\n");
        let path = dir.path().join(name);
        std::fs::write(&path, encode(&text)).unwrap();
        path.to_str().unwrap().to_string()
    };
    let utf8_bom = |text: &str| [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat();
    let utf16_le = |text: &str| {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    };
    let utf16_be = |text: &str| {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        bytes
    };

    let pass_bom = save("pass-bom.log", &plain_pass, &utf8_bom);
    let fail_bom = save("fail-bom.log", &plain_fail, &utf8_bom);
    assert_eq!(diff_json_without_paths(&pass_bom, &fail_bom), expected);

    let fail_16 = save("fail-utf16.log", &plain_fail, &utf16_le);
    // The usual mix: baselines from CI as UTF-8, the failing run captured by hand.
    assert_eq!(diff_json_without_paths(&plain_pass, &fail_16), expected);
    let pass_16be = save("pass-utf16be.log", &plain_pass, &utf16_be);
    assert_eq!(diff_json_without_paths(&pass_16be, &fail_16), expected);
}

#[test]
fn a_file_that_is_not_text_is_refused_with_exit_code_2() {
    // A PNG given by mistake (a wrong artifact path) was "mined" into hundreds of junk
    // templates, and the diff exited 0 or 1 like any other run.
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("screenshot.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
    bytes.extend((0..4096u32).map(|i| (i * 7 % 251) as u8));
    std::fs::write(&png, bytes).unwrap();
    cmd()
        .args([
            "diff",
            png.to_str().unwrap(),
            "--target",
            &fixture("pytest-fail.log"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("screenshot.png: not a text file"));
    // A missing file is the same kind of failure: 2, not the 1 that means "found something".
    cmd()
        .args([
            "diff",
            "no-such.log",
            "--target",
            &fixture("pytest-fail.log"),
        ])
        .assert()
        .code(2);
}

#[test]
fn gzip_is_recognized_by_its_content_not_its_name() {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let dir = tempfile::tempdir().unwrap();
    let compressed = dir.path().join("pytest-fail.log");
    let mut enc = GzEncoder::new(
        std::fs::File::create(&compressed).unwrap(),
        Compression::default(),
    );
    enc.write_all(&std::fs::read(fixture("pytest-fail.log")).unwrap())
        .unwrap();
    enc.finish().unwrap();
    let plain = diff_json_without_paths(&fixture("pytest-pass.log"), &fixture("pytest-fail.log"));
    let gzipped =
        diff_json_without_paths(&fixture("pytest-pass.log"), compressed.to_str().unwrap());
    assert_eq!(plain, gzipped);
}

/// Writes `lines` to a file in `dir` and returns its path.
fn log_file(dir: &tempfile::TempDir, name: &str, lines: &[String]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn a_new_message_in_a_json_log_is_a_new_template() {
    // Every line has the same keys. Counted as agreement, the keys alone made any two
    // lines one template, and a new error among them changed nothing a report could show.
    let event = |second: u32, level: &str, msg: &str| {
        format!(
            r#"{{"ts":"2024-01-01T00:00:{second:02}Z","level":"{level}","msg":"{msg}","service":"api","port":8080}}"#
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let mut good = vec![event(0, "info", "server started")];
    good.extend((1..40).map(|s| {
        event(
            s,
            "info",
            if s % 3 == 0 {
                "cache refreshed"
            } else {
                "request handled"
            },
        )
    }));
    let mut bad = good.clone();
    bad.insert(20, event(20, "error", "database connection failed"));
    let (good, bad) = (
        log_file(&dir, "good.log", &good),
        log_file(&dir, "bad.log", &bad),
    );

    let v: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", &good, &bad, "--json"])).unwrap();
    let new: Vec<&serde_json::Value> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "new")
        .collect();
    assert_eq!(new.len(), 1, "{v}");
    assert!(new[0]["template"]
        .as_str()
        .unwrap()
        .contains("database connection failed"));
    assert_eq!(v["total_templates"], 4);

    // The same message at a level it never had is a new value of that template.
    let mut louder = (0..40)
        .map(|s| event(s, "info", "request handled"))
        .collect::<Vec<_>>();
    let quiet = log_file(&dir, "quiet.log", &louder);
    louder[30] = event(30, "error", "request handled");
    let louder = log_file(&dir, "louder.log", &louder);
    let out = stdout_of(&["diff", &quiet, &louder, "--color", "never"]);
    assert!(out.contains("NEW VALUE"), "{out}");
    assert!(out.contains(r#"level="error""#), "{out}");
}

#[test]
fn an_instance_that_stops_writing_a_line_is_reported_gone() {
    // Five instances write the same line; the template is one, `<*> health check ok`.
    let run = |silent: Option<u32>| -> Vec<String> {
        (0..60)
            .flat_map(|tick| (0..5).map(move |instance| (tick, instance)))
            .filter(|(_, instance)| Some(*instance) != silent)
            .map(|(tick, instance)| {
                format!("2024-01-01T00:{tick:02}:00Z [svc-search-{instance}] health check ok")
            })
            .collect()
    };
    let dir = tempfile::tempdir().unwrap();
    let good = log_file(&dir, "good.log", &run(None));
    let bad = log_file(&dir, "bad.log", &run(Some(3)));
    let out = stdout_of(&["diff", &good, &bad, "--color", "never"]);
    assert!(
        out.contains("2 templates") || out.contains("1 template"),
        "{out}"
    );
    assert!(
        out.contains("GONE       baseline=60 target=0")
            && out.contains("[svc-search-3] health check ok"),
        "{out}"
    );
    // A pass against a pass says nothing, and neither do workers that change their names.
    let again = log_file(&dir, "again.log", &run(None));
    assert!(stdout_of(&["diff", &good, &again, "--color", "never"]).contains("0 findings"));
    let renamed: Vec<String> = run(None)
        .iter()
        .map(|line| line.replace("svc-search-", "svc-search-1"))
        .collect();
    let renamed = log_file(&dir, "renamed.log", &renamed);
    let out = stdout_of(&["diff", &good, &renamed, "--color", "never"]);
    assert!(!out.contains("GONE"), "{out}");
}

#[test]
fn a_log_from_another_day_is_not_new_for_its_date() {
    // klog puts the month and day in the first token of every line: `I1004`, `I1005`.
    let run = |day: &str| -> Vec<String> {
        (0..30)
            .map(|n| {
                format!(
                    "I{day} 00:40:{n:02}.152211   44882 controller.go:667] quota admission added evaluator for: pods"
                )
            })
            .chain(std::iter::once(format!(
                "W{day} 00:41:00.000000   44882 mutation_detector.go:54] Mutation detector is enabled"
            )))
            .collect()
    };
    let dir = tempfile::tempdir().unwrap();
    let monday = log_file(&dir, "monday.log", &run("1004"));
    let tuesday = log_file(&dir, "tuesday.log", &run("1005"));
    let out = stdout_of(&["diff", &monday, &tuesday, "--color", "never"]);
    assert!(out.contains("2 templates · 0 findings"), "{out}");
}

#[test]
fn a_new_value_after_a_bracketed_field_is_shown_in_its_place() {
    // `[worker pool main]` is one token of three words. The value that flipped is the
    // template's fifth token and its seventh word.
    let dir = tempfile::tempdir().unwrap();
    let line = |n: u32, status: &str| format!("INFO [worker pool main] job {n} {status} cleanly");
    let good: Vec<String> = (0..12).map(|n| line(n, "PASSED")).collect();
    let mut bad = good.clone();
    bad[7] = line(7, "FAILED");
    let (good, bad) = (
        log_file(&dir, "good.log", &good),
        log_file(&dir, "bad.log", &bad),
    );
    let out = stdout_of(&["diff", &good, &bad, "--color", "never"]);
    assert!(
        out.contains(
            "NEW VALUE  INFO [worker pool main] job <*> FAILED cleanly  (baseline: PASSED)"
        ),
        "{out}"
    );
    let v: serde_json::Value =
        serde_json::from_str(&stdout_of(&["diff", &good, &bad, "--json"])).unwrap();
    let flipped = &v["value_findings"][0];
    let words: Vec<&str> = flipped["template"].as_str().unwrap().split(' ').collect();
    assert_eq!(words[flipped["position"].as_u64().unwrap() as usize], "<*>");
    assert_eq!(flipped["position"], 6);
}
