"""Replay synthetic Docker-envelope records through an actual installed CLI.

Usage: python replay.py BINARY NEW_DIRECTORY before|after
Only the standard library is used. No Docker daemon or remote service is called.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

STYLES = {
    "plain": ("", ""),
    "sgr": ("\x1b[31m", "\x1b[0m"),
    "osc-bel": ("\x1b]8;;https://example.test\x07", "\x1b]8;;\x07"),
    "osc-st": ("\x1b]8;;https://example.test\x1b\\", "\x1b]8;;\x1b\\"),
    "cri": ("2026-10-12T10:00:00Z stdout F ", ""),
    "journald": ("Oct 12 10:00:00 host worker[42]: ", ""),
    "sgr-cri": ("\x1b[31m2026-10-12T10:00:00Z stdout F ", "\x1b[0m"),
    "sgr-journald": ("\x1b[31mOct 12 10:00:00 host worker[42]: ", "\x1b[0m"),
    "timestamp": ("2026-10-12T10:00:00Z ", ""),
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def envelope(body, style="plain", compact=True, ending="\n"):
    prefix, suffix = STYLES[style]
    return json.dumps(
        {"log": prefix + body + suffix + ending, "stream": "stdout"},
        separators=(",", ":") if compact else None,
    )


def event(status, route="/checkout", **options):
    return envelope(
        json.dumps({"route": route, "status": status}, separators=(",", ":")), **options
    )


def cases():
    ordinary = event(200)
    maintenance = event(503, route="/maintenance")
    for style in STYLES:
        for compact in (True, False):
            options = {
                "style": style,
                "compact": compact,
                "ending": "\n" if compact else "\r\n",
            }
            yield {
                "name": f"grouped-{style}-{'compact' if compact else 'spaced'}",
                "kind": "novelty",
                "baseline": [ordinary, maintenance, event(200, **options)],
                "target": [ordinary, maintenance, event(503, **options)],
                "extra": ["--watch-by", "/route"],
            }
    yield {
        "name": "pooled-sgr",
        "kind": "novelty",
        "baseline": [ordinary, event(200, style="sgr")],
        "target": [ordinary, event(503, style="sgr")],
        "extra": [],
    }
    for style in ("sgr", "journald", "cri"):
        # The first 50 unstyled records are unchanged (2 errors). The other 50
        # change from 0 to 25 errors: the complete rate moves from 2% to 27%.
        common = [event(503 if i < 2 else 200) for i in range(50)]
        yield {
            "name": "rate-" + style,
            "kind": "rate",
            "baseline": common
            + [event(200, style=style, compact=False) for _ in range(50)],
            "target": common
            + [
                event(503 if i < 25 else 200, style=style, compact=False)
                for i in range(50)
            ],
            "extra": ["--watch-by", "/route", "--watch-rate-change", "5"],
        }
    for name, body, counter in [
        ("duplicate", '{"status":503,"status":200}', "ambiguous"),
        ("malformed", '{"status":503', "invalid_json"),
    ]:
        yield {
            "name": name,
            "kind": "incomplete",
            "baseline": [ordinary],
            "target": [ordinary, envelope(body, style="sgr")],
            "extra": [],
            "counter": counter,
        }
    body = json.dumps({"status": "\x1b[31m503\x1b[0m"}, separators=(",", ":"))
    yield {
        "name": "escaped-scalar",
        "kind": "literal",
        "baseline": [envelope(body)],
        "target": [envelope(body, style="sgr")],
        "extra": [],
    }
    yield {
        "name": "ordinary-control",
        "kind": "unchanged",
        "baseline": [ordinary, maintenance],
        "target": [ordinary, maintenance],
        "extra": ["--watch-by", "/route"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("phase", choices=["before", "after"])
    args = parser.parse_args()
    binary = args.binary.resolve()
    root = args.output.resolve()
    root.mkdir(parents=True)
    results = []
    for case in cases():
        directory = root / case["name"]
        directory.mkdir()
        inputs = {}
        for side in ("baseline", "target"):
            data = ("\n".join(case[side]) + "\n").encode()
            (directory / (side + ".log")).write_bytes(data)
            inputs[side] = {
                "bytes": len(data),
                "sha256": digest(data),
                "records": len(case[side]),
            }
        flags = ["--watch-field", "/status", *case["extra"]]
        command = [
            str(binary),
            "diff",
            "baseline.log",
            "--target",
            "target.log",
            *flags,
            "--json",
        ]
        result = subprocess.run(command, cwd=directory, capture_output=True, timeout=30)
        assert not result.stderr, result.stderr
        (directory / "report.json").write_bytes(result.stdout)
        report = json.loads(result.stdout)
        field = report["watched_fields"][0]
        relevant = [
            v
            for v in field["values"]
            if v["value_json"] == "503"
            and v.get("group_values_json", ['"/checkout"']) == ['"/checkout"']
        ]
        new = [v["value_json"] for v in field["values"] if v["is_new"] is True]
        if args.phase == "after":
            if case["kind"] in ("novelty", "rate", "unchanged", "literal"):
                assert field["complete"] is True, case["name"]
                assert field["target"]["matched"] == len(case["target"]), case["name"]
                assert field["baselines"][0]["matched"] == len(case["baseline"]), case[
                    "name"
                ]
            if case["kind"] == "novelty":
                assert result.returncode == 1 and new == ["503"], case["name"]
                source = relevant[0]["first_target"]
                assert source["line_no"] == len(case["target"])
                assert source["raw"] == case["target"][-1]
            elif case["kind"] == "rate":
                assert result.returncode == 1 and not new
                assert (
                    relevant[0]["baseline_counts"] == [2]
                    and relevant[0]["target_count"] == 27
                )
                rates = field["rate_comparison"]
                group = rates["groups"][0]
                assert (
                    rates["complete"]
                    and group["baseline_totals"] == [100]
                    and group["target_total"] == 100
                )
                assert len(group["changes"]) == 2
            elif case["kind"] == "incomplete":
                assert result.returncode == 2 and field["complete"] is False
                assert field["target"][case["counter"]] == 1
                assert field["target"]["first_problem"]["raw"] == case["target"][-1]
            else:
                assert result.returncode == 0 and not new
                if case["kind"] == "literal":
                    assert (
                        json.loads(field["values"][0]["value_json"])
                        == "\x1b[31m503\x1b[0m"
                    )
        results.append(
            {
                "case": case["name"],
                "kind": case["kind"],
                "flags": flags,
                "inputs": inputs,
                "exit": result.returncode,
                "report_sha256": digest(result.stdout),
                "watch": field,
            }
        )
    print(
        json.dumps(
            {
                "phase": args.phase,
                "scope": (
                    "Synthetic Docker-envelope records; actual CLI; "
                    "no Docker daemon or remote service."
                ),
                "cases": results,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
