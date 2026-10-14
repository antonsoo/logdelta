//! A stream cannot supply two independent comparison samples. Exercise actual pipes,
//! including rejection before EOF and valid stdin inputs against file-only oracles.
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::prelude::*;
use logdelta::analysis::{diff_runs, DiffOptions};

fn cmd() -> Command {
    Command::cargo_bin("logdelta").unwrap()
}

#[test]
fn duplicate_stdin_rejects_before_reading_or_waiting_for_eof() {
    let cases = [
        vec!["diff", "-", "-", "--json"],
        vec!["diff", "-", "--target", "-", "--json"],
        vec!["diff", "-", "-", "--target", "unopened-target.log"],
        vec!["novel", "--baseline", "-"],
        vec!["novel", "--baseline", "-", "-"],
        vec![
            "novel",
            "--baseline",
            "-",
            "--baseline",
            "-",
            "unopened-target.log",
        ],
    ];
    for args in cases {
        let mut child = cmd()
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Keep stdin open and unwritten. A reader waiting for a header or EOF
        // must not prevent an invalid invocation from failing immediately.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{args:?} waited for stdin instead of rejecting it");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("stdin ('-') can only be used once"),
            "{args:?}"
        );
    }
}

#[test]
fn library_rejects_duplicate_stdin_before_opening_any_files() {
    for (baselines, target) in [
        (vec!["unopened-baseline.log", "-"], "-"),
        (vec!["-", "-"], "unopened-target.log"),
    ] {
        let error = diff_runs(&baselines, target, &[], &DiffOptions::default())
            .err()
            .expect("duplicate stdin must reject");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("save separate input logs to files"));
    }
}

#[test]
fn repeated_files_remain_independent_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same.log");
    std::fs::write(&path, "startup complete\n").unwrap();
    let path = path.to_str().unwrap();
    let result = diff_runs(&[path, path], path, &[], &DiffOptions::default()).unwrap();
    assert_eq!(result.baseline_totals, vec![1, 1]);
    assert_eq!(result.target_total, 1);
    assert_eq!(result.finding_count(), 0);
}

#[test]
fn single_stdin_diff_matches_file_samples() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before.log");
    let after = dir.path().join("after.log");
    let baseline = "startup complete\n";
    let target = "panic: unavailable database\n";
    std::fs::write(&before, baseline).unwrap();
    std::fs::write(&after, target).unwrap();
    let before = before.to_str().unwrap();
    let after = after.to_str().unwrap();
    let oracle = cmd()
        .args(["diff", before, after, "--json"])
        .output()
        .unwrap();
    assert_eq!(oracle.status.code(), Some(1));
    for (args, payload) in [
        (vec!["diff", "-", after, "--json"], baseline),
        (vec!["diff", before, "-", "--json"], target),
    ] {
        let mut child = cmd()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), oracle.status.code());
        assert_eq!(output.stdout, oracle.stdout);
        assert_eq!(output.stderr, oracle.stderr);
    }
}

#[test]
fn single_stdin_novel_baseline_matches_file_sample() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before.log");
    let after = dir.path().join("after.log");
    let baseline = "startup complete\n";
    std::fs::write(&before, baseline).unwrap();
    std::fs::write(&after, "startup complete\npanic: unavailable database\n").unwrap();
    let before = before.to_str().unwrap();
    let after = after.to_str().unwrap();
    let oracle = cmd()
        .args(["novel", "--baseline", before, after])
        .output()
        .unwrap();
    assert_eq!(oracle.status.code(), Some(1));
    let mut child = cmd()
        .args(["novel", "--baseline", "-", after])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(baseline.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), oracle.status.code());
    assert_eq!(output.stdout, oracle.stdout);
    assert_eq!(output.stderr, oracle.stderr);
}
