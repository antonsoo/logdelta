"""Replay application/Docker identity cases through the actual command.

python replay.py BINARY NEW_DIRECTORY before|after
Uses the standard library and committed native formatter captures.
"""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def event(status, **extra):
    return {"route": "/checkout", "http": {"status": status}, **extra}


def encoded(row):
    return json.dumps(row, separators=(",", ":"))


def docker(row, **extra):
    return encoded({"log": encoded(row) + "\n", "stream": "stdout", **extra})


def cases():
    root = Path(__file__).resolve().parent
    for kind in ("native", "native-shadow"):
        for grouped in (False, True):
            yield {
                "name": kind + ("-grouped" if grouped else "-pooled"),
                "kind": "novelty",
                "baseline": (root / "inputs" / f"{kind}-baseline.log").read_text(),
                "target": (root / "inputs" / f"{kind}-target.log").read_text(),
                "extra": ["--watch-by", "/route"] if grouped else [],
            }
    companion = encoded(event(200)) + "\n"
    for stream in ("stdout", "payments"):
        for message in ("request completed", '{"http":{"status":200},"route":"/checkout"}'):
            def application(status):
                return companion + encoded(event(status, log=message, stream=stream, time="2026-10-15T12:00:00Z")) + "\n"
            yield {"name": f"application-{stream}-{'shadow' if message.startswith('{') else 'text'}",
                   "kind": "novelty", "baseline": application(200), "target": application(503), "extra": []}
    for kind in ("native", "native-shadow"):
        rows = {side: (root / "inputs" / f"{kind}-{side}.log").read_text().splitlines() for side in ("baseline", "target")}
        yield {"name": "docker-wrapped-" + kind, "kind": "novelty",
               **{side: "\n".join(docker(json.loads(row), time="2026-10-15T12:00:00Z", attrs={"service":"checkout"}) for row in lines) + "\n" for side, lines in rows.items()},
               "extra": ["--watch-by", "/route"]}
    for name, metadata in [("minimal", {}), ("full", {"time":"2026-10-15T12:00:00Z", "attrs":{"service":"checkout"}})]:
        yield {"name": "docker-control-" + name, "kind": "novelty",
               "baseline": docker(event(200), **metadata) + "\n", "target": docker(event(503), **metadata) + "\n", "extra": []}
    def rate_rows(changed):
        rows = [encoded(event(503 if i < 2 else 200)) for i in range(50)]
        rows += [encoded(event(503 if changed and i < 25 else 200, log="request completed", time="2026-10-15T12:00:00Z")) for i in range(50)]
        return "\n".join(rows) + "\n"
    yield {"name":"application-rate", "kind":"rate", "baseline":rate_rows(False), "target":rate_rows(True),
           "extra":["--watch-by", "/route", "--watch-rate-change", "5"]}
    for key, duplicate in [("log", '"log":"{}"'), ("stream", '"stream":"stderr"'), ("time", '"time":"later"'), ("attrs", '"attrs":{}')]:
        row = docker(event(503), time="2026-10-15T12:00:00Z", attrs={})
        row = row[:-1] + "," + duplicate + "}"
        yield {"name":"duplicate-wrapper-" + key, "kind":"incomplete", "baseline":companion, "target":companion + row + "\n", "extra":[]}
    row = docker(event(503), attrs={"service":"checkout"})
    row = row.replace('"service":"checkout"', '"service":"checkout","service":"other"')
    yield {"name":"duplicate-attribute-name", "kind":"incomplete", "baseline":companion, "target":companion + row + "\n", "extra":[]}
    yield {"name":"unchanged-application", "kind":"unchanged",
           "baseline":(root / "inputs/native-baseline.log").read_text(),
           "target":(root / "inputs/native-baseline.log").read_text(), "extra":[]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("phase", choices=["before", "after"])
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True)
    results = []
    for case in cases():
        directory = args.output / case["name"]
        directory.mkdir()
        inputs = {}
        for side in ("baseline", "target"):
            data = case[side].encode()
            (directory / f"{side}.log").write_bytes(data)
            inputs[side] = {"bytes":len(data), "records":len(data.splitlines()), "sha256":hashlib.sha256(data).hexdigest()}
        flags = ["--watch-field", "/http/status", *case["extra"]]
        result = subprocess.run([str(binary), "diff", "baseline.log", "--target", "target.log", *flags, "--json"],
                                cwd=directory, capture_output=True, timeout=30)
        assert not result.stderr, result.stderr
        report = json.loads(result.stdout)
        (directory / "report.json").write_bytes(result.stdout)
        field = report["watched_fields"][0]
        if args.phase == "after":
            if case["kind"] == "incomplete":
                assert result.returncode == 2 and not field["complete"], case["name"]
                assert field["target"]["ambiguous"] == 1, case["name"]
                assert field["target"]["first_problem"]["raw"] == case["target"].splitlines()[-1]
            else:
                assert field["complete"], case["name"]
                assert field["target"]["matched"] == inputs["target"]["records"], case["name"]
                assert field["baselines"][0]["matched"] == inputs["baseline"]["records"], case["name"]
                assert result.returncode == (0 if case["kind"] == "unchanged" else 1), case["name"]
                if case["kind"] == "novelty":
                    value = next(v for v in field["values"] if v["value_json"] == "503")
                    assert value["is_new"] is True, case["name"]
                    assert value["first_target"]["raw"] == case["target"].splitlines()[-1]
                elif case["kind"] == "rate":
                    group = field["rate_comparison"]["groups"][0]
                    assert group["baseline_totals"] == [100] and group["target_total"] == 100
                    value = next(v for v in field["values"] if v["value_json"] == "503")
                    assert value["baseline_counts"] == [2] and value["target_count"] == 27
        results.append({"case":case["name"], "kind":case["kind"], "flags":flags, "inputs":inputs,
                        "exit":result.returncode, "report_sha256":hashlib.sha256(result.stdout).hexdigest(), "watch":field})
    print(json.dumps({"phase":args.phase, "scope":"Controlled application records and synthetic Docker-compatible wrappers; not production incidents.",
                      "baseline_revision":"fad282595b202c1be1153015ec0a52fb9061f09a", "cases":results}, indent=2))


if __name__ == "__main__":
    main()
