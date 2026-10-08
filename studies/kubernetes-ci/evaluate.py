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
- for the control, how many findings a pass against passes produces. Every one
  of those is noise.

Results are appended to results.json under the label, so two versions can be
compared on the same logs.
"""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

HERE = Path(__file__).parent
ROOT = HERE.parent.parent


EXTRA_ARGUMENTS: list[str] = []


def diff(binary: Path, baselines: list[Path], target: Path) -> dict[str, Any] | None:
    command = [str(binary), "diff", *map(str, baselines), "--target", str(target), "--json"]
    command += EXTRA_ARGUMENTS
    out = subprocess.run(command, capture_output=True, text=True, check=False)
    if out.returncode not in (0, 1):
        return None
    report: dict[str, Any] = json.loads(out.stdout)
    return report


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
        "another_day": day(case["failed_started"]) != day(case["baselines_started"][0]),
    }
    one = diff(binary, baselines[:1], failed)
    if one is None:
        return None
    row["one_baseline"] = {
        "names_a_failed_test": names_a_test(one, case["failed_tests"]),
        "findings": reading(one)[0],
        "ungrouped": reading(one)[1],
        "templates": one["total_templates"],
    }
    if len(baselines) > 1:
        several = diff(binary, baselines, failed)
        if several is not None:
            row["all_baselines"] = {
                "names_a_failed_test": names_a_test(several, case["failed_tests"]),
                "findings": reading(several)[0],
                "ungrouped": reading(several)[1],
            }
    control = logs / f"{case['control']}.log" if case["control"] else None
    if control is not None and control.exists():
        quiet = diff(binary, baselines, control)
        if quiet is not None:
            row["control"] = {"findings": reading(quiet)[0], "ungrouped": reading(quiet)[1]}
    return row


def summarize(rows: list[dict[str, Any]], key: str) -> dict[str, Any]:
    have = [row[key] for row in rows if key in row]
    if not have:
        return {"cases": 0}
    findings = sorted(entry["findings"] for entry in have)
    summary: dict[str, Any] = {
        "cases": len(have),
        "findings_median": statistics.median(findings),
        "findings_mean": round(statistics.fmean(findings), 1),
        "findings_p90": findings[int(0.9 * (len(findings) - 1))],
        "ungrouped_median": statistics.median(entry["ungrouped"] for entry in have),
    }
    if "names_a_failed_test" in have[0]:
        named = sum(entry["names_a_failed_test"] for entry in have)
        summary |= {"names_a_failed_test": named, "share": round(named / len(have), 3)}
    else:
        summary["no_findings"] = sum(entry["findings"] == 0 for entry in have)
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "release" / "logdelta")
    parser.add_argument("--label", default="checkout")
    parser.add_argument("--manifest", type=Path, default=HERE / "manifest.json")
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--out", type=Path, default=HERE / "results.json")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--threshold", help="pass --threshold to logdelta")
    args = parser.parse_args()
    if args.threshold:
        EXTRA_ARGUMENTS.extend(["--threshold", args.threshold])

    cases = json.loads(args.manifest.read_text())["cases"]
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
