"""Serial alternating CLI control: BEFORE AFTER NEW_DIRECTORY. Linux only."""

import argparse
import hashlib
import json
import platform
import resource
import statistics
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True)
    row = json.dumps({"log": json.dumps({"level": "info", "msg": "request completed", "status": 200}) + "\n",
                      "stream": "stdout", "time": "2026-10-15T12:00:00Z"}, separators=(",", ":")) + "\n"
    data = (row * 100_000).encode()
    path = args.output.resolve() / "docker.log"
    path.write_bytes(data)
    runs = {"baseline": [], "current": []}
    hashes = set()
    for _ in range(3):
        for label, binary in [("baseline", args.before.resolve()), ("current", args.after.resolve())]:
            usage = resource.getrusage(resource.RUSAGE_CHILDREN)
            start = time.perf_counter()
            result = subprocess.run([str(binary), "templates", str(path), "--json"], capture_output=True, check=True)
            elapsed = time.perf_counter() - start
            end = resource.getrusage(resource.RUSAGE_CHILDREN)
            hashes.add(hashlib.sha256(result.stdout).hexdigest())
            runs[label].append({"wall_seconds": elapsed, "cpu_seconds": end.ru_utime + end.ru_stime - usage.ru_utime - usage.ru_stime})
    assert len(hashes) == 1
    print(json.dumps({
        "scope": "Synthetic 100,000-record genuine Docker-wrapper control; serial alternating CLI calls on shared WSL2. No universal speed claim.",
        "input": {"records": 100000, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "line": row.rstrip()},
        "platform": platform.platform(), "runs": runs,
        "median_wall_seconds": {label: statistics.median(row["wall_seconds"] for row in values) for label, values in runs.items()},
        "all_template_reports_identical": True, "report_sha256": hashes.pop(),
    }, indent=2))


if __name__ == "__main__":
    main()
