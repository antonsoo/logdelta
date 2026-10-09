#!/usr/bin/env python3
"""Check native grouped watches against HTTP-client observations and a separate counter.

Standard library only. Build with cargo build --release first. Optional timing uses
synthetic repeated traffic; neither it nor the loopback capture is production data.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import random
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def execute(binary, baselines, target, fields, groups, timing_file=None):
    command = [str(binary), "diff", *map(str, baselines), "--target", str(target), "--json"]
    for field in fields:
        command += ["--watch-field", field]
    for group in groups:
        command += ["--watch-by", group]
    if timing_file is not None:
        command = ["/usr/bin/time", "-f", "%M", "-o", str(timing_file), *command]
    started = time.perf_counter()
    process = subprocess.run(command, capture_output=True, text=True, check=False)
    duration = time.perf_counter() - started
    if process.returncode not in (0, 1):
        raise RuntimeError(f"analysis exited {process.returncode}: {process.stderr or process.stdout}")
    return process.returncode, json.loads(process.stdout), duration


def check_counts(field, runs, name, keys, raw_runs=None, line_offset=0):
    """An ordinary Python Counter over decoded records, independent of the Rust tracker."""
    counts = [Counter((tuple(encoded(row[k]) for k in keys), encoded(row[name]))
                      for row in run if name in row) for run in runs]
    baseline_groups = {group for counter in counts[:-1] for group, _ in counter}
    expected_keys = set().union(*counts)
    actual = {(tuple(value.get("group_values_json", [])), value["value_json"]): value
              for value in field["values"]}
    assert field["complete"] is True
    assert len(field["baselines"]) == len(runs) - 1
    for coverage, counter in zip([*field["baselines"], field["target"]], counts):
        assert coverage["matched"] == sum(counter.values())
    assert len(actual) == len(field["values"])  # No duplicate or collapsed ledger rows.
    assert set(actual) == expected_keys, (set(actual), expected_keys)
    for key, value in actual.items():
        baseline = [counter[key] for counter in counts[:-1]]
        target = counts[-1][key]
        assert value["baseline_counts"] == baseline
        assert value["target_count"] == target
        assert value["is_new"] == (target > 0 and sum(baseline) == 0)
        if keys:
            assert value["group_seen_in_baseline"] == (key[0] in baseline_groups)
        for source_name, indexes in [("first_baseline", range(len(runs) - 1)),
                                     ("first_target", [len(runs) - 1])]:
            hits = [(r, i + 1 + line_offset) for r in indexes for i, row in enumerate(runs[r])
                    if name in row and (tuple(encoded(row[k]) for k in keys), encoded(row[name])) == key]
            if not hits:
                assert source_name not in value
                continue
            run_index, line = hits[0]
            at = value[source_name]
            assert at["line_no"] == line
            if source_name == "first_baseline":
                assert at["baseline_index"] == run_index
            if raw_runs is not None:
                assert at["raw"] == raw_runs[run_index][line - 1]
    return len(actual)


def captured_http(binary):
    names = ["http-routes-good.log", "http-routes-good-2.log", "http-routes-failed.log"]
    observed = json.loads((ROOT / "examples/http-routes-observed.json").read_text())
    runs = [observed["runs"][name]["responses"] for name in names]
    raw = [(ROOT / "examples" / name).read_text().splitlines() for name in names]
    for rows, source in zip(runs, raw):
        assert rows == [{"route": (record := json.loads(line))["route"],
                         "status": record["http"]["status"]} for line in source[1:-1]]
    paths = [ROOT / "examples" / name for name in names]
    pooled_code, pooled, _ = execute(binary, paths[:-1], paths[-1], ["/http/status"], [])
    grouped_code, grouped, _ = execute(binary, paths[:-1], paths[-1], ["/http/status"], ["/route"])
    assert pooled_code == 0 and grouped_code == 1
    assert pooled["findings"] == grouped["findings"] == []
    assert pooled["value_findings"] == grouped["value_findings"] == []
    check_counts(pooled["watched_fields"][0], runs, "status", [], raw, line_offset=1)
    pairs = check_counts(grouped["watched_fields"][0], runs, "status", ["route"], raw, line_offset=1)
    new = [value for value in grouped["watched_fields"][0]["values"] if value["is_new"]]
    assert len(new) == 1 and new[0]["target_count"] == 2
    return {"provenance": observed["provenance"], "client_responses_checked": sum(map(len, runs)),
            "pooled_exit": pooled_code, "grouped_exit": grouped_code, "retained_pairs": pairs,
            "finding": new[0], "input_sha256": {
                name: hashlib.sha256((ROOT / "examples" / name).read_bytes()).hexdigest()
                for name in [*names, "http-routes-observed.json"]}}


def random_reference(binary, directory, cases, seed):
    rng = random.Random(seed)
    checked_rows = 0
    for case in range(cases):
        keys = ["route"] if case % 2 == 0 else ["service", "route"]
        runs = []
        paths = []
        raw_runs = []
        for r in range(2 + case % 3):
            target = r == 1 + case % 3
            rows = []
            for _ in range(rng.randint(35, 110)):
                row = {"service": rng.choice([True, 1, "1"] + (["canary"] if target else [])),
                       "route": rng.choice(["/checkout", "/maintenance", "/a|b", "/界"]),
                       "status": rng.choice([200, 503, False, None, "200", 200.0] + ([418] if target else [])),
                       "outcome": rng.choice(["ok", "retry"] + (["failed"] if target else []))}
                if rng.random() < 0.1:
                    del row["status"]
                rows.append(row)
            rng.shuffle(rows)
            runs.append(rows)
            raw = [encoded(row) for row in rows]
            raw_runs.append(raw)
            path = directory / f"reference-{r}.jsonl"
            path.write_text("\n".join(raw) + "\n")
            paths.append(path)
        _, result, _ = execute(binary, paths[:-1], paths[-1], ["/status", "/outcome"], [f"/{k}" for k in keys])
        assert len(result["watched_fields"]) == 2
        for field, name in zip(result["watched_fields"], ["status", "outcome"]):
            assert field["pointer"] == f"/{name}"
            assert field["group_by"] == [f"/{k}" for k in keys]
            checked_rows += check_counts(field, runs, name, keys, raw_runs)
    return {"seed": seed, "cases": cases, "field_comparisons": cases * 2,
            "ledger_rows_checked": checked_rows, "checks": ["exact keys", "per-run counts", "new pairs",
            "new versus observed groups", "first source lines", "original source excerpts"]}


def benchmark(binary, directory, lines):
    paths = [directory / "bench-good.jsonl", directory / "bench-target.jsonl"]
    for run, path in enumerate(paths):
        with path.open("w") as out:
            for i in range(lines):
                route = ["/checkout", "/maintenance", "/search", "/catalog"][(i // 4) % 4]
                status = 503 if route == "/maintenance" or (run and route == "/checkout" and i % 800 == 0) else 200
                out.write(encoded({"service": i % 4, "route": route, "status": status, "request_id": i}) + "\n")
    measurements = []
    for groups in [[], ["/service", "/route"]]:
        timing_file = directory / "rss.txt" if Path("/usr/bin/time").exists() else None
        code, result, seconds = execute(binary, paths[:-1], paths[-1], ["/status"], groups, timing_file)
        field = result["watched_fields"][0]
        assert field["complete"]
        measurement = {"watch_by": groups, "exit": code, "elapsed_seconds": round(seconds, 4),
                       "retained_pairs": len(field["values"])}
        if timing_file is not None:
            # GNU time may also write "Command exited with non-zero status 1".
            measurement["peak_rss_kib"] = int(timing_file.read_text().splitlines()[-1])
        measurements.append(measurement)
    return {"provenance": "Synthetic repeated traffic; one measured process per setting, warm filesystem cache.",
            "lines_per_run": lines, "input_bytes": sum(path.stat().st_size for path in paths),
            "measurements": measurements}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/logdelta")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cases", type=int, default=40)
    parser.add_argument("--seed", type=int, default=20261009)
    parser.add_argument("--benchmark-lines", type=int, default=0)
    args = parser.parse_args()
    if args.cases < 1 or args.benchmark_lines < 0:
        parser.error("cases must be positive and benchmark-lines nonnegative")
    binary = args.binary.resolve()
    result = {"created_at": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    result["captured_http"] = captured_http(binary)
    with tempfile.TemporaryDirectory(prefix="logdelta-grouped-") as temp:
        directory = Path(temp)
        result["reference"] = random_reference(binary, directory, args.cases, args.seed)
        if args.benchmark_lines:
            result["benchmark"] = benchmark(binary, directory, args.benchmark_lines)
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
