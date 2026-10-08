"""Score logdelta's template miner on Loghub-2k by grouping accuracy.

    python3 studies/loghub/fetch.py
    cargo build --release --example cluster_ids
    python3 studies/loghub/score.py                       # this checkout
    python3 studies/loghub/score.py --binary PATH --label 0.3.4

Grouping accuracy is the benchmark's own measure (Zhu et al., "Tools and
Benchmarks for Automated Log Parsing", ICSE-SEIP 2019): a line counts as
correctly parsed when the set of lines the parser grouped it with is exactly
the set of lines that share its hand-labelled template. Splitting one
template in two, or merging two into one, makes every line involved wrong.

Each system is scored twice:

- on the message alone, the `Content` column, which is what the benchmark
  gives every parser after cutting the header off with a per-system format;
- on the whole line as the system wrote it, which is what logdelta is given
  in use. Nobody tells it where the header ends.

`DRAIN` holds the accuracies the reference Drain implementation publishes for
the same data, for comparison. Those runs use a log format, masking regexes
and a similarity threshold chosen per system; logdelta runs with one
configuration for all sixteen.
"""

from __future__ import annotations

import argparse
import csv
import json
import subprocess
import tempfile
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

from fetch import COMMIT, SYSTEMS

HERE = Path(__file__).parent
ROOT = HERE.parent.parent
# logparser/Drain/README.md, "Benchmark", at logpai/logparser d9d4180.
DRAIN = {
    "HDFS": 0.9975,
    "Hadoop": 0.9475,
    "Spark": 0.92,
    "Zookeeper": 0.9665,
    "BGL": 0.9625,
    "HPC": 0.887,
    "Thunderbird": 0.955,
    "Windows": 0.997,
    "Linux": 0.69,
    "Android": 0.911,
    "HealthApp": 0.78,
    "Apache": 1.0,
    "Proxifier": 0.5265,
    "OpenSSH": 0.7875,
    "OpenStack": 0.7325,
    "Mac": 0.7865,
}


def cluster_ids(binary: Path, lines: list[str], threshold: str | None = None) -> list[str]:
    """The template id logdelta gives each line, in order."""
    with tempfile.NamedTemporaryFile("w", suffix=".log", encoding="utf-8", newline="\n") as handle:
        # One record per line: a message that holds a line break would shift every id after it.
        handle.write("\n".join(line.replace("\r", " ").replace("\n", " ") for line in lines) + "\n")
        handle.flush()
        command = [str(binary), handle.name, *([threshold] if threshold else [])]
        out = subprocess.run(command, check=True, capture_output=True, text=True)
    ids = out.stdout.split()
    if len(ids) != len(lines):
        raise SystemExit(f"{binary} returned {len(ids)} ids for {len(lines)} lines")
    return ids


def grouping_accuracy(predicted: list[str], truth: list[str]) -> float:
    members: dict[str, set[int]] = defaultdict(set)
    for index, label in enumerate(predicted):
        members[label].add(index)
    events: dict[str, set[int]] = defaultdict(set)
    for index, label in enumerate(truth):
        events[label].add(index)
    correct = sum(len(group) for group in members.values() if group in events.values())
    return correct / len(truth)


def errors(predicted: list[str], truth: list[str], lines: list[str]) -> dict[str, Any]:
    """Where the wrong lines are: templates split apart, and templates merged together."""
    by_event: dict[str, Counter[str]] = defaultdict(Counter)
    by_cluster: dict[str, Counter[str]] = defaultdict(Counter)
    example: dict[tuple[str, str], str] = {}
    for label, event, line in zip(predicted, truth, lines, strict=True):
        by_event[event][label] += 1
        by_cluster[label][event] += 1
        example.setdefault((event, label), line)
    split = {event: parts for event, parts in by_event.items() if len(parts) > 1}
    merged = {label: parts for label, parts in by_cluster.items() if len(parts) > 1}
    worst_split = sorted(split.items(), key=lambda item: -sum(item[1].values()))[:3]
    worst_merged = sorted(merged.items(), key=lambda item: -sum(item[1].values()))[:3]
    return {
        "templates": len(by_event),
        "found": len(by_cluster),
        "templates_split": len(split),
        "lines_in_split_templates": sum(sum(parts.values()) for parts in split.values()),
        "groups_merging_templates": len(merged),
        "lines_in_merged_groups": sum(sum(parts.values()) for parts in merged.values()),
        "largest_splits": [
            {
                "event": event,
                "lines": sum(parts.values()),
                "into": len(parts),
                "examples": [example[(event, label)][:200] for label, _ in parts.most_common(3)],
            }
            for event, parts in worst_split
        ],
        "largest_merges": [
            {
                "lines": sum(parts.values()),
                "templates": len(parts),
                "examples": [example[(event, label)][:200] for event, _ in parts.most_common(3)],
            }
            for label, parts in worst_merged
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument(
        "--binary", type=Path, default=ROOT / "target" / "release" / "examples" / "cluster_ids"
    )
    parser.add_argument("--label", default="checkout")
    parser.add_argument("--threshold", help="similarity threshold, if not the default")
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--out", type=Path, default=HERE / "results.json")
    args = parser.parse_args()

    systems: dict[str, Any] = {}
    for system in SYSTEMS:
        with (args.cache / f"{system}_2k.log_structured.csv").open(
            newline="", encoding="utf-8", errors="replace"
        ) as handle:
            rows = list(csv.DictReader(handle))
        raw = (
            (args.cache / f"{system}_2k.log").read_text(encoding="utf-8", errors="replace").splitlines()
        )
        if len(raw) != len(rows):
            raise SystemExit(f"{system}: {len(raw)} lines but {len(rows)} labels")
        truth = [row["EventId"] for row in rows]
        content = [row["Content"] for row in rows]
        on_content = cluster_ids(args.binary, content, args.threshold)
        on_lines = cluster_ids(args.binary, raw, args.threshold)
        systems[system] = {
            "lines": len(rows),
            "message": {
                "grouping_accuracy": round(grouping_accuracy(on_content, truth), 4),
                **errors(on_content, truth, content),
            },
            "whole_line": {
                "grouping_accuracy": round(grouping_accuracy(on_lines, truth), 4),
                **errors(on_lines, truth, raw),
            },
            "drain_published": DRAIN[system],
        }
        print(
            f"{system:12s} message {systems[system]['message']['grouping_accuracy']:.3f}  "
            f"whole line {systems[system]['whole_line']['grouping_accuracy']:.3f}  "
            f"Drain, tuned {DRAIN[system]:.3f}"
        )

    def average(key: str) -> float:
        return round(sum(s[key]["grouping_accuracy"] for s in systems.values()) / len(systems), 4)

    summary = {
        "label": args.label,
        "loghub_commit": COMMIT,
        "average": {
            "message": average("message"),
            "whole_line": average("whole_line"),
            "drain_published": round(sum(DRAIN.values()) / len(DRAIN), 4),
        },
        "systems": systems,
    }
    print(
        f"{'average':12s} message {summary['average']['message']:.3f}  "
        f"whole line {summary['average']['whole_line']:.3f}  "
        f"Drain, tuned {summary['average']['drain_published']:.3f}"
    )
    results = json.loads(args.out.read_text()) if args.out.exists() else {}
    results[args.label] = summary
    args.out.write_text(json.dumps(results, indent=1) + "\n")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
