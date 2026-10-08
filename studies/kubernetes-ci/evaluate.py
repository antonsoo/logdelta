"""Diff each failed Kubernetes build's log against passing builds of the same commit.

    python3 studies/kubernetes-ci/evaluate.py                          # this checkout
    python3 studies/kubernetes-ci/evaluate.py --binary PATH --label 0.3.4

For every case in manifest.json (see collect.py) this runs `logdelta diff` twice
with a failed build as the target, once against one passing build of the same
commit and once against up to three, and once more with a passing build as the
target, the control. From the JSON report it records:

- whether a finding names a test that failed: a new template, or a new value,
  whose line or template holds the name of a test the build's JUnit report
  lists as failed;
- how much there is to read: findings as reported (lines that belong together
  are one finding) and ungrouped;
- whether the report shows the failure's own reason: the assertion or panic line
  the JUnit report holds for a failed test, `plugins_test.go:2247: Didn't expect
  the first pod to be scheduled`, among the lines the default terminal report
  prints, and how many lines that report has;
- for the control, how many findings a pass against passes produces. Every one
  of those is noise.

Results are appended to results.json under the label, so two versions can be
compared on the same logs. `--plain` measures instead what no log parser is
needed for: `grep -- '--- FAIL'`, a set difference of lines, and a grep for
failure markers. Intervals resample the commits the cases come from.
"""

from __future__ import annotations

import argparse
import json
import random
import re
import statistics
import subprocess
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

HERE = Path(__file__).parent
ROOT = HERE.parent.parent


EXTRA_ARGUMENTS: list[str] = []
LOCATION = re.compile(r"^(\S+_test\.go:\d+:)\s*(.*)$")
MARKERS = re.compile(r"--- FAIL|_test\.go:\d+:|^\s*panic:")
RESAMPLES = 2000


def squash(text: str) -> str:
    return " ".join(text.split())


def reason_key(reason: str, place_only: bool = False) -> str:
    """The start of an assertion line: where it was raised and the first words it says.

    With place_only, just `file_test.go:123:`: the same assertion with other values in it.
    """
    match = LOCATION.match(reason.strip())
    if match:
        return squash(match.group(1) if place_only else f"{match.group(1)} {match.group(2)[:30]}")
    return squash(reason)[:40]


def shown_reasons(case: dict[str, Any], text: str, place_only: bool = False) -> tuple[int, int]:
    """How many of the failed tests' reasons appear in text, and how many have one."""
    reasons = [r["reason"] for r in case.get("failure_reasons", []) if r["reason"]]
    flat = squash(text)
    return sum(reason_key(r, place_only) in flat for r in reasons), len(reasons)


def diff(binary: Path, baselines: list[Path], target: Path) -> dict[str, Any] | None:
    command = [str(binary), "diff", *map(str, baselines), "--target", str(target), "--json"]
    command += EXTRA_ARGUMENTS
    out = subprocess.run(command, capture_output=True, text=True, check=False)
    if out.returncode not in (0, 1):
        return None
    report: dict[str, Any] = json.loads(out.stdout)
    return report


def human(binary: Path, baselines: list[Path], target: Path, *extra: str) -> str:
    """The terminal report, by default as a reader gets it."""
    command = [str(binary), "diff", *map(str, baselines), "--target", str(target)]
    command += ["--color", "never", *extra]
    command += EXTRA_ARGUMENTS
    out = subprocess.run(command, capture_output=True, text=True, check=False)
    return out.stdout if out.returncode in (0, 1) else ""


def reading(report: dict[str, Any]) -> tuple[int, int]:
    """Findings as reported (a block is one) and ungrouped."""
    findings = report["findings"]
    values = report["value_findings"]
    blocks = report.get("blocks", [])
    loose = sum(f.get("block") is None for f in findings) + sum(
        v.get("block") is None for v in values
    )
    return len(blocks) + loose, len(findings) + len(values)


def names_a_test(report: dict[str, Any], tests: list[str]) -> bool:
    shown = [
        f"{f.get('first_target_raw') or ''}\n{f['template']}"
        for f in report["findings"]
        if f["kind"] == "new"
    ] + [f"{v.get('first_target_raw') or ''}\n{v['template']}" for v in report["value_findings"]]
    return any(test in text for test in tests for text in shown)


def day(timestamp: int) -> str:
    return datetime.fromtimestamp(timestamp, tz=timezone.utc).strftime("%Y-%m-%d")


def entry(
    case: dict[str, Any], binary: Path, baselines: list[Path], target: Path
) -> dict[str, Any] | None:
    report = diff(binary, baselines, target)
    if report is None:
        return None
    text = human(binary, baselines, target)
    everything = human(binary, baselines, target, "--block-lines", "0")
    shown, have = shown_reasons(case, text)
    return {
        "names_a_failed_test": names_a_test(report, case["failed_tests"]),
        "findings": reading(report)[0],
        "ungrouped": reading(report)[1],
        "templates": report["total_templates"],
        "lines_shown": len(text.splitlines()),
        "reasons": have,
        "reasons_shown": shown,
        "reason_shown": (shown > 0) if have else None,
        # the assertion's own line, whatever values it holds: one line of a template that is shown
        "reason_place_shown": (shown_reasons(case, text, True)[0] > 0) if have else None,
        # with every line of every block printed (--block-lines 0)
        "reason_in_blocks": (shown_reasons(case, everything)[0] > 0) if have else None,
        "lines_if_blocks_unclipped": len(everything.splitlines()),
    }


def evaluate(case: dict[str, Any], binary: Path, cache: Path) -> dict[str, Any] | None:
    logs = cache / case["job"]
    failed = logs / f"{case['failed']}.log"
    baselines = [logs / f"{build}.log" for build in case["baselines"]]
    if not failed.exists() or not all(path.exists() for path in baselines):
        return None
    row: dict[str, Any] = {
        "job": case["job"],
        "failed": case["failed"],
        "baselines": len(baselines),
        "commit": case["commit"],
        "another_day": day(case["failed_started"]) != day(case["baselines_started"][0]),
    }
    one = entry(case, binary, baselines[:1], failed)
    if one is None:
        return None
    row["one_baseline"] = one
    if len(baselines) > 1:
        several = entry(case, binary, baselines, failed)
        if several is not None:
            row["all_baselines"] = several
    control = logs / f"{case['control']}.log" if case["control"] else None
    if control is not None and control.exists():
        quiet = diff(binary, baselines, control)
        if quiet is not None:
            row["control"] = {"findings": reading(quiet)[0], "ungrouped": reading(quiet)[1]}
    return row


def interval(
    pairs: list[tuple[str, Any]], statistic: Any, seed: int = 20261008
) -> list[float]:
    """95% interval of a statistic over (commit, value) pairs, resampling whole commits.

    The 264 cases come from 121 commits, and cases of one commit are not independent: the
    same flaky test, the same baselines. Commits are drawn with replacement.
    """
    by_commit: dict[str, list[Any]] = {}
    for commit, value in pairs:
        by_commit.setdefault(commit, []).append(value)
    groups = list(by_commit.values())
    rng = random.Random(seed)
    draws = []
    for _ in range(RESAMPLES):
        sample = [v for g in (rng.choice(groups) for _ in groups) for v in g]
        draws.append(statistic(sample))
    draws.sort()
    return [round(draws[int(0.025 * RESAMPLES)], 3), round(draws[int(0.975 * RESAMPLES)], 3)]


def summarize(rows: list[dict[str, Any]], key: str) -> dict[str, Any]:
    pairs = [(row["commit"], row[key]) for row in rows if key in row]
    have = [entry for _, entry in pairs]
    if not have:
        return {"cases": 0}
    findings = sorted(entry["findings"] for entry in have)
    summary: dict[str, Any] = {
        "cases": len(have),
        "commits": len({commit for commit, _ in pairs}),
        "findings_median": statistics.median(findings),
        "findings_mean": round(statistics.fmean(findings), 1),
        "findings_p90": findings[int(0.9 * (len(findings) - 1))],
        "ungrouped_median": statistics.median(entry["ungrouped"] for entry in have),
        "ungrouped_mean": round(statistics.fmean(entry["ungrouped"] for entry in have), 1),
    }
    if "names_a_failed_test" in have[0]:
        named = sum(entry["names_a_failed_test"] for entry in have)
        summary |= {"names_a_failed_test": named, "share": round(named / len(have), 3)}
        lines = [(c, e["lines_shown"]) for c, e in pairs]
        summary["lines_shown_median"] = statistics.median(v for _, v in lines)
        summary["lines_shown_median_95"] = interval(lines, lambda v: statistics.median(v))
        summary["findings_median_95"] = interval(
            [(c, e["findings"]) for c, e in pairs], lambda v: statistics.median(v)
        )
        with_reason = [(c, e["reason_shown"]) for c, e in pairs if e["reason_shown"] is not None]
        shown = sum(v for _, v in with_reason)
        summary |= {
            "cases_with_a_reason": len(with_reason),
            "reason_shown": shown,
            "reason_shown_share": round(shown / len(with_reason), 3),
            "reason_shown_share_95": interval(with_reason, lambda v: sum(v) / len(v)),
        }
        for field in ("reason_place_shown", "reason_in_blocks"):
            flags = [(c, e[field]) for c, e in pairs if e[field] is not None]
            summary[field] = sum(v for _, v in flags)
            summary[f"{field}_95"] = interval(flags, lambda v: round(sum(v) / len(v), 3))
        summary["lines_if_blocks_unclipped_median"] = statistics.median(
            e["lines_if_blocks_unclipped"] for e in have
        )
    else:
        summary["no_findings"] = sum(entry["findings"] == 0 for entry in have)
    return summary


def plain(case: dict[str, Any], cache: Path) -> dict[str, Any] | None:
    """What a reader gets from tools that know nothing about templates."""
    logs = cache / case["job"]
    failed, passed = logs / f"{case['failed']}.log", logs / f"{case['baselines'][0]}.log"
    if not failed.exists() or not passed.exists():
        return None
    target = failed.read_text(encoding="utf-8", errors="replace").splitlines()
    seen = set(passed.read_text(encoding="utf-8", errors="replace").splitlines())
    reads = {
        "grep_fail": [line for line in target if "--- FAIL" in line],
        "set_difference": [line for line in target if line not in seen],
        "grep_markers": [line for line in target if MARKERS.search(line)],
    }
    row: dict[str, Any] = {"job": case["job"], "failed": case["failed"], "commit": case["commit"]}
    for name, lines in reads.items():
        text = "\n".join(lines)
        shown, have = shown_reasons(case, text)
        row[name] = {
            "names_a_failed_test": any(t in line for t in case["failed_tests"] for line in lines),
            "lines_shown": len(lines),
            "reasons": have,
            "reasons_shown": shown,
            "reason_shown": (shown > 0) if have else None,
        }
    return row


def summarize_plain(rows: list[dict[str, Any]]) -> dict[str, Any]:
    out: dict[str, Any] = {"cases": len(rows), "commits": len({r["commit"] for r in rows})}
    for name in ("grep_fail", "set_difference", "grep_markers"):
        pairs = [(r["commit"], r[name]) for r in rows]
        named = sum(e["names_a_failed_test"] for _, e in pairs)
        lines = [(c, e["lines_shown"]) for c, e in pairs]
        with_reason = [(c, e["reason_shown"]) for c, e in pairs if e["reason_shown"] is not None]
        shown = sum(v for _, v in with_reason)
        out[name] = {
            "names_a_failed_test": named,
            "lines_shown_median": statistics.median(v for _, v in lines),
            "lines_shown_median_95": interval(lines, lambda v: statistics.median(v)),
            "lines_shown_p90": sorted(v for _, v in lines)[int(0.9 * (len(lines) - 1))],
            "cases_with_a_reason": len(with_reason),
            "reason_shown": shown,
            "reason_shown_share": round(shown / len(with_reason), 3),
            "reason_shown_share_95": interval(with_reason, lambda v: sum(v) / len(v)),
        }
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "release" / "logdelta")
    parser.add_argument("--label", default="checkout")
    parser.add_argument("--manifest", type=Path, default=HERE / "manifest.json")
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--out", type=Path, default=HERE / "results.json")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--threshold", help="pass --threshold to logdelta")
    parser.add_argument(
        "--plain", action="store_true", help="measure grep and a set difference instead"
    )
    args = parser.parse_args()
    if args.threshold:
        EXTRA_ARGUMENTS.extend(["--threshold", args.threshold])

    cases = json.loads(args.manifest.read_text())["cases"]
    if args.plain:
        found = [r for r in (plain(c, args.cache) for c in cases) if r]
        results = json.loads(args.out.read_text()) if args.out.exists() else {}
        results["plain"] = {"summary": summarize_plain(found), "cases": found}
        args.out.write_text(json.dumps(results, indent=1) + "\n")
        print(json.dumps(results["plain"]["summary"], indent=1))
        return
    with ThreadPoolExecutor(args.workers) as pool:
        rows = [
            row for row in pool.map(lambda c: evaluate(c, args.binary, args.cache), cases) if row
        ]
    summary: dict[str, Any] = {"cases": len(rows), "by_job": {}}
    groups = {"all": rows} | {
        job: [row for row in rows if row["job"] == job] for job in sorted({r["job"] for r in rows})
    }
    for name, group in groups.items():
        summary["by_job"][name] = {
            "failed_vs_one_pass": summarize(group, "one_baseline"),
            "failed_vs_up_to_three": summarize(group, "all_baselines"),
            "pass_vs_passes": summarize(group, "control"),
            "failed_vs_one_pass_of_another_day": summarize(
                [row for row in group if row["another_day"]], "one_baseline"
            ),
        }
    results = json.loads(args.out.read_text()) if args.out.exists() else {}
    results[args.label] = {"summary": summary, "cases": rows}
    args.out.write_text(json.dumps(results, indent=1) + "\n")
    overall = summary["by_job"]["all"]
    print(f"{args.label}: {len(rows)} cases")
    for key, value in overall.items():
        print(f"  {key}: {value}")


if __name__ == "__main__":
    main()
