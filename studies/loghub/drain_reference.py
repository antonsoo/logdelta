"""Run the reference Drain (logpai/logparser) on Loghub-2k, tuned and with one setting.

    python3 studies/loghub/fetch.py
    curl -L https://codeload.github.com/logpai/logparser/tar.gz/d9d4180 | tar xz -C DIR
    env -i PATH="$PATH" HOME=DIR/home DIR/venv/bin/python studies/loghub/drain_reference.py \\
        --logparser DIR/logparser-d9d4180

(the venv needs `regex pandas scipy numpy`; the reference code is third-party, so it
runs with an empty environment.) Three runs, scored with logparser's own evaluator:

- `tuned`: the settings in the reference's benchmark.py, per system a log format,
  masking regexes, a tree depth and a similarity threshold. This should reproduce the
  accuracy column of its README; it checks that this harness is the reference's.
- `one setting`: depth 4, threshold 0.5, no masking regexes, for all sixteen systems.
- `one setting, generic masking`: the same, plus three regexes that mean something in
  any log: an IPv4 address (with a port), a hexadecimal number, and a bare number.

The log format, which cuts the header off, is still each system's own: the reference
cannot run without one. That is the message column of logdelta's table.
"""

from __future__ import annotations

import argparse
import ast
import json
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
GENERIC = [r"(\d+\.){3}\d+(:\d+)?", r"\b0[Xx][0-9a-fA-F]+\b", r"\b\d+\b"]


def settings(benchmark: Path) -> dict[str, dict]:
    """benchmark_settings from the reference's benchmark.py, read without running it."""
    for node in ast.parse(benchmark.read_text()).body:
        if isinstance(node, ast.Assign) and node.targets[0].id == "benchmark_settings":  # type: ignore[attr-defined]
            return ast.literal_eval(node.value)
    raise SystemExit("benchmark_settings not found")


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--logparser", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--out", type=Path, default=HERE / "results.json")
    args = parser.parse_args()
    sys.path.insert(0, str(args.logparser))
    from logparser.Drain import LogParser  # noqa: E402
    from logparser.utils import evaluator  # noqa: E402

    tuned = settings(args.logparser / "logparser" / "Drain" / "benchmark.py")
    runs = {
        "tuned": lambda s: (s["regex"], s["st"], s["depth"]),
        "one setting": lambda s: ([], 0.5, 4),
        "one setting, generic masking": lambda s: (GENERIC, 0.5, 4),
    }
    summary: dict[str, dict] = {}
    for name, choose in runs.items():
        scores: dict[str, float] = {}
        for system, setting in tuned.items():
            regex, threshold, depth = choose(setting)
            with tempfile.TemporaryDirectory() as out:
                LogParser(
                    log_format=setting["log_format"],
                    indir=str(args.cache),
                    outdir=out,
                    rex=regex,
                    depth=depth,
                    st=threshold,
                ).parse(f"{system}_2k.log")
                _, accuracy = evaluator.evaluate(
                    groundtruth=str(args.cache / f"{system}_2k.log_structured.csv"),
                    parsedresult=str(Path(out) / f"{system}_2k.log_structured.csv"),
                )
            scores[system] = round(accuracy, 4)
        summary[name] = {
            "average": round(sum(scores.values()) / len(scores), 4),
            "systems": scores,
        }
        print(f"{name}: {summary[name]['average']:.4f}", file=sys.stderr)
    results = json.loads(args.out.read_text()) if args.out.exists() else {}
    results["drain_reference_rerun"] = {"logparser_commit": "d9d4180", **summary}
    args.out.write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
