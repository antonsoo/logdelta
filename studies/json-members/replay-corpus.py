#!/usr/bin/env python3
"""Compare complete CLI captures on the existing, hash-pinned public log cache."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import subprocess
import time


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    before, after = args.before.resolve(), args.after.resolve()
    receipt = json.loads((repo / "studies/container-payloads/results/corpus.json").read_text())
    for name, record in receipt["inputs"].items():
        data = (repo / name).read_bytes()
        assert len(data) == record["bytes"] and sha(data) == record["sha256"], name

    cases = json.loads((repo / "studies/kubernetes-ci/manifest.json").read_text())["cases"]
    jobs = []
    for case in cases:
        folder = f"studies/kubernetes-ci/cache/{case['job']}"
        command = ["diff", *[f"{folder}/{build}.log" for build in case["baselines"]],
                   f"{folder}/{case['failed']}.log", "--json"]
        jobs.append((f"{case['job']}/{case['failed']}", command))
    for name in receipt["inputs"]:
        if "/loghub/" in name:
            jobs.append((Path(name).name, ["templates", name, "--json", "--top", "100000"]))

    def check(job):
        name, command = job
        outputs = []
        for cli in (before, after):
            result = subprocess.run([str(cli), *command], cwd=repo, capture_output=True, timeout=180)
            assert result.returncode in (0, 1), result.stderr
            outputs.append(result)
        old, new = outputs
        return {"case": name, "command": command,
                "identical": old.stdout == new.stdout and old.stderr == new.stderr
                and old.returncode == new.returncode,
                "beforeExit": old.returncode, "afterExit": new.returncode,
                "beforeSha256": sha(old.stdout), "afterSha256": sha(new.stdout)}

    start = time.monotonic()
    with ThreadPoolExecutor(max_workers=4) as pool:
        rows = list(pool.map(check, jobs))
    result = {
        "scope": "Current replay of existing cached public inputs; no new collection or ecosystem "
                 "accuracy claim. Kubernetes failures use every manifest-provided passing baseline; "
                 "Loghub uses full template reports.",
        "inputs": receipt["inputs"], "inputFiles": len(receipt["inputs"]),
        "inputBytes": sum(record["bytes"] for record in receipt["inputs"].values()),
        "comparisons": len(rows), "disagreements": sum(not row["identical"] for row in rows),
        "seconds": round(time.monotonic() - start, 3), "records": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: result[key] for key in
                     ("inputFiles", "inputBytes", "comparisons", "disagreements", "seconds")}))
    assert result["disagreements"] == 0


if __name__ == "__main__":
    main()
