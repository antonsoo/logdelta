#!/usr/bin/env python3
"""Replay actual CLI pipes against separate-file controls; standard library only."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import tempfile
from pathlib import Path

ERROR = "stdin ('-') can only be used once"


def run(cli: Path, args: list[str], payload: bytes | None) -> dict:
    result = subprocess.run(
        [str(cli), *args], input=payload, capture_output=True, timeout=10, check=False
    )
    return {
        "exit": result.returncode,
        "stdout": result.stdout.decode(),
        "stderr": result.stderr.decode(),
    }


def before_eof(cli: Path, args: list[str]) -> dict:
    child = subprocess.Popen(
        [str(cli), *args],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        # Hold the pipe open with no bytes. Do not invent an empty log by
        # closing stdin before testing whether argument validation can finish.
        child.wait(timeout=1)
        result = {"exit": child.returncode, "waitedForStdin": False}
    except subprocess.TimeoutExpired:
        result = {"exit": None, "waitedForStdin": True}
        child.kill()
    finally:
        out, err = child.communicate(timeout=5)
    return {**result, "stdout": out.decode(), "stderr": err.decode()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--observe-only", action="store_true")
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    cases = []
    with tempfile.TemporaryDirectory(prefix="logdelta-stdin-") as temporary:
        directory = Path(temporary)
        before = directory / "before.log"
        after = directory / "after.log"
        good = directory / "good.jsonl"
        bad = directory / "bad.jsonl"
        baseline = b"startup complete\n"
        target = b"startup complete\npanic: unavailable database\n"
        good_text = b'{"service":"checkout","status":200}\n'
        bad_text = b'{"service":"checkout","status":500}\n'
        for path, data in [
            (before, baseline),
            (after, target),
            (good, good_text),
            (bad, bad_text),
        ]:
            path.write_bytes(data)
        b, a, g, f = map(str, [before, after, good, bad])

        invalid = [
            ("diff-positional", ["diff", "-", "-", "--json"]),
            ("diff-explicit-target", ["diff", "-", "--target", "-", "--json"]),
            ("diff-two-baselines", ["diff", "-", "-", "--target", a, "--json"]),
            (
                "diff-exact-watch",
                ["diff", "-", "-", "--watch-field", "/status", "--json"],
            ),
            ("novel-default-target", ["novel", "--baseline", "-"]),
            ("novel-explicit-target", ["novel", "--baseline", "-", "-"]),
            ("novel-two-baselines", ["novel", "--baseline", "-", "--baseline", "-", a]),
        ]
        for name, command in invalid:
            payload = good_text if name == "diff-exact-watch" else target
            result = run(cli, command, payload)
            agrees = (
                result["exit"] == 2
                and not result["stdout"]
                and ERROR in result["stderr"]
            )
            cases.append({"case": name, "args": command, "agrees": agrees, **result})

        controls = [
            (
                "diff-stdin-baseline",
                ["diff", "-", a, "--json"],
                baseline,
                ["diff", b, a, "--json"],
            ),
            (
                "diff-stdin-target",
                ["diff", b, "-", "--json"],
                target,
                ["diff", b, a, "--json"],
            ),
            (
                "watch-stdin-baseline",
                ["diff", "-", f, "--watch-field", "/status", "--json"],
                good_text,
                ["diff", g, f, "--watch-field", "/status", "--json"],
            ),
            (
                "watch-stdin-target",
                ["diff", g, "-", "--watch-field", "/status", "--json"],
                bad_text,
                ["diff", g, f, "--watch-field", "/status", "--json"],
            ),
            (
                "novel-stdin-baseline",
                ["novel", "--baseline", "-", a],
                baseline,
                ["novel", "--baseline", b, a],
            ),
            (
                "novel-stdin-target",
                ["novel", "--baseline", b],
                target,
                ["novel", "--baseline", b, a],
            ),
            (
                "diff-repeated-file",
                ["diff", b, b, b, "--json"],
                None,
                ["diff", b, b, "--json"],
            ),
        ]
        for name, command, payload, oracle_args in controls:
            expected = run(cli, oracle_args, None)
            result = run(cli, command, payload)
            if name == "diff-repeated-file":
                expected_report = json.loads(expected["stdout"])
                expected_report["baseline_totals"] = [1, 1]
                agrees = (
                    result["exit"] == 0
                    and not result["stderr"]
                    and json.loads(result["stdout"]) == expected_report
                )
            else:
                assert expected["exit"] == 1, (
                    "The separate samples must reveal a difference"
                )
                agrees = result == expected
            cases.append(
                {
                    "case": name,
                    "args": command,
                    "agrees": agrees,
                    "fileOnlyOracle": expected,
                    **result,
                }
            )

        for name, command in [
            ("diff-open-pipe", ["diff", "-", "-", "--json"]),
            ("novel-open-pipe", ["novel", "--baseline", "-"]),
        ]:
            result = before_eof(cli, command)
            agrees = (
                result["exit"] == 2
                and not result["waitedForStdin"]
                and not result["stdout"]
                and ERROR in result["stderr"]
            )
            cases.append({"case": name, "args": command, "agrees": agrees, **result})

        # Capture complete streams without publishing this machine's paths.
        cases = json.loads(
            json.dumps(cases).replace(str(directory), "<case-directory>")
        )
    result = {
        "python": platform.python_version(),
        "cliVersion": run(cli, ["--version"], None)["stdout"].strip(),
        "binarySha256": hashlib.sha256(cli.read_bytes()).hexdigest(),
        "population": "Synthetic log lines through a native CLI and real OS pipes. File-only controls compare entire reports. No production-log or deployment claim.",
        "count": len(cases),
        "disagreements": sum(not case["agrees"] for case in cases),
        "cases": cases,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(
        json.dumps(
            {key: value for key, value in result.items() if key != "cases"}, indent=2
        )
    )
    if result["disagreements"] and not args.observe_only:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
