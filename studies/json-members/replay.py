#!/usr/bin/env python3
"""Real public Rust API and CLI/file/gzip/pipe replay; constructed ambiguous JSON."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--crate-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--toolchain", default="stable")
    parser.add_argument("--observe-only", action="store_true")
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    crate = args.crate_root.resolve(strict=True)
    here = Path(__file__).resolve().parent
    cases = json.loads((here / "cases.json").read_text())
    scratch = Path(tempfile.mkdtemp(prefix="logdelta-json-members-"))
    consumer = scratch / "consumer"
    (consumer / "src").mkdir(parents=True)
    shutil.copyfile(here / "consumer.rs", consumer / "src/main.rs")
    (consumer / "Cargo.toml").write_text(
        '[package]\nname="json-member-consumer"\nversion="0.1.0"\nedition="2021"\n'
        '[dependencies]\nlogdelta={path=' + json.dumps(str(crate))
        + ',default-features=false}\nserde_json="1"\n'
    )
    build = subprocess.run(
        ["cargo", f"+{args.toolchain}", "build", "--release", "--offline"], cwd=consumer,
        capture_output=True, text=True, timeout=300, check=True,
    )
    (scratch / "consumer-build.log").write_text(build.stdout + build.stderr)
    result = subprocess.run(
        [str(consumer / "target/release/json-member-consumer"), str(here / "cases.json")],
        capture_output=True, text=True, timeout=30, check=True,
    )
    library = {row["id"]: row for row in json.loads(result.stdout)}
    records = []

    def run(command: list[str], cwd: Path, payload: bytes | None = None) -> dict:
        result = subprocess.run([str(cli), *command], cwd=cwd, input=payload,
                                capture_output=True, timeout=30, check=False)
        assert result.returncode in (0, 1, 2), result.stderr
        return {"exit": result.returncode, "stdout": result.stdout.decode(),
                "stderr": result.stderr.decode()}

    def signal_present(item: dict, text: str) -> bool:
        return item["signal"] in text if "signal" in item else text.count("health") >= item["members"]

    for item in cases:
        folder = scratch / item["id"]
        folder.mkdir()
        baseline = (item["baseline"] + "\n").encode() * 12
        target = (item["target"] + "\n").encode() * 12
        (folder / "baseline.log").write_bytes(baseline)
        (folder / "target.log").write_bytes(target)
        (folder / "baseline.log.gz").write_bytes(gzip.compress(baseline, mtime=0))
        (folder / "target.log.gz").write_bytes(gzip.compress(target, mtime=0))
        row = library[item["id"]]
        control = item.get("control", False)
        tokens = " ".join(row["targetTokens"])
        agrees = not row["diff"]["findings"] if control else (
            signal_present(item, tokens) and any(f["kind"] == "new" for f in row["diff"]["findings"])
        )
        records.append({"id": item["id"], "kind": "public-api", "control": control,
                        "agrees": agrees, "inputSha256": hashlib.sha256(target).hexdigest(), **row})
        workflows = [
            ("file", ["diff", "baseline.log", "target.log", "--json"], None),
            ("gzip", ["diff", "baseline.log.gz", "target.log.gz", "--json"], None),
            ("stdin-target", ["diff", "baseline.log", "-", "--json"], target),
            ("stdin-baseline", ["diff", "-", "target.log", "--json"], baseline),
            ("novel-pipe", ["novel", "--baseline", "baseline.log"], target),
        ]
        for kind, command, payload in workflows:
            outcome = run(command, folder, payload)
            if kind == "novel-pipe":
                agrees = not outcome["stdout"] if control else signal_present(item, outcome["stdout"])
                report = None
            else:
                report = json.loads(outcome["stdout"])
                agrees = outcome["exit"] == (0 if control else 1) and (
                    not report["findings"] if control else any(f["kind"] == "new" for f in report["findings"])
                )
            records.append({"id": item["id"], "kind": kind, "control": control,
                            "agrees": agrees, "inputSha256": hashlib.sha256(target).hexdigest(),
                            "command": command, "report": report, **outcome})

    # Exact-field lookup already treated repeated watched members as incomplete.
    folder = scratch / "message"
    outcome = run(["diff", "baseline.log", "target.log", "--watch-field", "/message", "--json"], folder)
    records.append({"id": "watched-duplicate-control", "kind": "exact-field", "control": True,
                    "agrees": outcome["exit"] == 2, "report": json.loads(outcome["stdout"]), **outcome})
    runtime = {}
    for name in ("src/mask.rs", "src/json_value.rs", "src/fields.rs"):
        path = crate / name
        if path.exists(): runtime[name] = hashlib.sha256(path.read_bytes()).hexdigest()
    report = {"label": args.label, "fixtureScope": "Constructed ambiguous JSON; no production incident",
              "rust": subprocess.check_output(["rustc", f"+{args.toolchain}", "--version"], text=True).strip(),
              "cliVersion": subprocess.check_output([str(cli), "--version"], text=True).strip(),
              "binarySha256": hashlib.sha256(cli.read_bytes()).hexdigest(), "runtime": runtime,
              "workflows": len(records), "disagreements": sum(not row["agrees"] for row in records),
              "records": records}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n")
    print(json.dumps({key: report[key] for key in ("label", "workflows", "disagreements", "rust")}))
    if not args.observe_only: assert report["disagreements"] == 0


if __name__ == "__main__":
    main()
