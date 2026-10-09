#!/usr/bin/env python3
"""Check captured HTTP evidence and compare the native rate scores with SciPy.

Requires numpy/scipy only for this verification, not for logdelta.
  python examples/http-rates/verify.py --binary target/release/logdelta --output /tmp/rate-check
Optional: --before /path/to/baseline/logdelta
"""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

import numpy as np
import scipy
from scipy.stats import chi2_contingency, fisher_exact

HERE = Path(__file__).resolve().parent


def oracle(counts, totals, count, total):
    table = np.array([[sum(counts), sum(totals) - sum(counts)], [count, total - count]], dtype=float)
    statistic = float(chi2_contingency(table + 0.5, correction=False, lambda_="log-likelihood").statistic)
    rates = np.array([n / d for n, d in zip(counts, totals) if d], dtype=float)
    penalty = 1.0 + 2.0 * float(np.std(rates) / np.mean(rates)) if len(rates) > 1 and np.mean(rates) > 0 else 1.0
    return statistic / penalty


def command(binary, files, rate=True, form="--json", delta=5):
    args = [str(binary), "diff", *map(str, files[:-1]), "--target", str(files[-1]),
            "--watch-field", "/http/status", "--watch-by", "/route"]
    if rate:
        args += ["--watch-rate-change", str(delta)]
    if form:
        args.append(form)
    result = subprocess.run(args, text=True, capture_output=True, timeout=30)
    assert result.returncode in (0, 1, 2), result.stderr
    return result


def inspect_scores(field):
    checked = 0
    rates = field["rate_comparison"]
    for group in rates["groups"]:
        if group["status"] != "compared":
            assert group["changes"] == []
            continue
        expected = []
        for index, value in enumerate(field["values"]):
            if value.get("group_values_json", []) != group["group_values_json"] or not sum(value["baseline_counts"]):
                continue
            denominators = group["baseline_totals"]
            baseline_rates = [n / d for n, d in zip(value["baseline_counts"], denominators) if d]
            target_rate = value["target_count"] / group["target_total"]
            distance = target_rate - max(baseline_rates) if target_rate > max(baseline_rates) else min(0, target_rate - min(baseline_rates))
            score = oracle(value["baseline_counts"], denominators, value["target_count"], group["target_total"])
            if distance and abs(distance * 100) + 1e-12 >= rates["min_change_pp"] and score >= rates["significance"]:
                expected.append(index)
        assert [change["value_index"] for change in group["changes"]] == expected
        for change in group["changes"]:
            value = field["values"][change["value_index"]]
            score = oracle(value["baseline_counts"], group["baseline_totals"], value["target_count"], group["target_total"])
            assert math.isclose(change["score"], score, rel_tol=1e-10, abs_tol=1e-10)
            checked += 1
    return checked


def records(total, failures):
    return "".join(json.dumps({"route": "/checkout", "http": {"status": 503 if i < failures else 200}}) + "\n" for i in range(total))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--before", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    files = [HERE / f"http-rate-{name}.log" for name in ["good", "good-2", "failed"]]
    observed = json.loads((HERE / "http-rate-observed.json").read_text())["runs"]
    client_counts = []
    for path in files:
        client = Counter((row["route"], row["status"]) for row in observed[path.name]["responses"])
        server = Counter((row["route"], row["http"]["status"]) for line in path.read_text().splitlines()
                         if line.startswith("{") and (row := json.loads(line)).get("event") == "request_complete")
        assert client == server
        assert sum(client.values()) == 2000
        client_counts.append(client)
    plain = command(binary, files, rate=False)
    assert plain.returncode == 0
    changed = command(binary, files)
    assert changed.returncode == 1, changed.stderr
    report = json.loads(changed.stdout)
    field = report["watched_fields"][0]
    assert field["complete"] and field["rate_comparison"]["complete"]
    for value in field["values"]:
        key = (json.loads(value["group_values_json"][0]), json.loads(value["value_json"]))
        assert value["baseline_counts"] == [counts[key] for counts in client_counts[:-1]]
        assert value["target_count"] == client_counts[-1][key]
        assert value["is_new"] is False
    groups = field["rate_comparison"]["groups"]
    assert [group["group_values_json"] for group in groups if group["changes"]] == [['"/checkout"']]
    assert groups[0]["baseline_totals"] == [1000, 1000] and groups[0]["target_total"] == 1000
    score_checks = inspect_scores(field)
    (args.output / "native.json").write_text(changed.stdout)
    (args.output / "novelty-only.json").write_text(plain.stdout)
    for flag, name in [(None, "terminal.txt"), ("--markdown", "report.md")]:
        result = command(binary, files, form=flag)
        assert result.returncode == 1 and "CHANGED RATES" in result.stdout
        (args.output / name).write_text(result.stdout)
    if args.before:
        before = command(args.before.resolve(), files, rate=False)
        assert before.returncode == 0
        (args.output / "before.json").write_text(before.stdout)

    # Numerically independent score checks include sparse counts and the inclusive effect boundary.
    scenarios = [
        ("stable", [(1000, 10), (1000, 12)], (1000, 11), 5),
        ("rate-rise", [(1000, 10), (1000, 12)], (1000, 200), 5),
        ("rate-fall", [(1000, 200), (1000, 220)], (1000, 10), 5),
        ("baseline-range", [(1000, 10), (10000, 3000)], (10000, 2000), 5),
        ("small-fluctuation", [(1000, 10)], (5, 1), 5),
        ("sparse-score-limit", [(1000, 1)], (5, 1), 5),
        ("inclusive-boundary", [(10000, 1000)], (10000, 1500), 5),
        ("below-effect-cutoff", [(10000, 1000)], (10000, 1500), 5.01),
        ("known-value-vanishes", [(1000, 200)], (1000, 0), 5),
    ]
    cases = []
    with tempfile.TemporaryDirectory(prefix="logdelta-rates-oracle-") as scratch:
        for name, baselines, target, delta in scenarios:
            paths = []
            for index, (total, count) in enumerate([*baselines, target]):
                path = Path(scratch) / f"{index}.jsonl"
                path.write_text(records(total, count))
                paths.append(path)
            result = command(binary, paths, delta=delta)
            parsed = json.loads(result.stdout)
            field = parsed["watched_fields"][0]
            score_checks += inspect_scores(field)
            cases.append({"name": name, "exit": result.returncode, "rates": field["rate_comparison"]})

    timings = {"novelty_only_ms": [], "with_rates_ms": []}
    for repeat in range(4):
        for enabled in ([False, True] if repeat % 2 == 0 else [True, False]):
            started = time.perf_counter()
            command(binary, files, rate=enabled)
            timings["with_rates_ms" if enabled else "novelty_only_ms"].append((time.perf_counter() - started) * 1000)
    result = {"scipy": scipy.__version__, "numpy": np.__version__, "httpResponsesChecked": sum(sum(c.values()) for c in client_counts),
              "scoreChecks": score_checks, "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "sourceHashes": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in files},
              "timings": timings, "medianMs": {key: statistics.median(values) for key, values in timings.items()},
              "cases": cases,
              "sparseCaseCaution": {"counts": [[1, 999], [1, 4]], "smoothedGScore": oracle([1], [1000], 1, 5),
                                    "fisherTwoSidedP": float(fisher_exact([[1, 999], [1, 4]]).pvalue)}}
    (args.output / "verification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: result[key] for key in ["httpResponsesChecked", "scoreChecks", "medianMs", "sparseCaseCaution"]}, indent=2))


if __name__ == "__main__":
    main()
