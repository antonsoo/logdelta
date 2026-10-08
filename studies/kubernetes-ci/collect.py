"""Choose failing and passing Kubernetes CI builds of the same commit, and fetch their logs.

    python3 studies/kubernetes-ci/collect.py            # the builds manifest.json lists
    python3 studies/kubernetes-ci/collect.py --rescan   # list the jobs afresh, rewrite it

Kubernetes' CI (Prow) publishes every build of every job to a public bucket:
when it started and on which commit, how it ended, its JUnit report and its
console log, `build-log.txt`. Two periodic jobs build `master` about hourly, so
a commit is often built several times, and sometimes one of those builds fails.

That gives a comparison with a known answer. The failing build and a passing
build ran the same code, so what differs between their logs is the failure
(and whatever differs between any two runs). The names of the tests that
failed are in the failing build's JUnit report.

`--rescan` picks, for each failed build whose commit also has a passing build:
up to three passing builds of that commit as baselines, nearest in time, and
when the commit has one more, another passing build as a control (a pass
diffed against passes should show nothing). It records the failed tests'
names from the report. Without it, the logs the manifest lists are fetched
into cache/. No Kubernetes code is run.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

HERE = Path(__file__).parent
BUCKET = "kubernetes-ci-logs"
API = f"https://storage.googleapis.com/storage/v1/b/{BUCKET}/o"
OBJECTS = f"https://storage.googleapis.com/{BUCKET}"
JOBS = ("ci-kubernetes-integration-master", "ci-kubernetes-unit")
FAILED_TEST = re.compile(r"^[ \t]*--- FAIL: (\S+)", re.MULTILINE)
BASELINES = 3


def fetch(url: str, attempts: int = 4) -> bytes | None:
    """The object's bytes, or None when the bucket does not have it."""
    for attempt in range(attempts):
        request = urllib.request.Request(url, headers={"User-Agent": "logdelta-study"})
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                return bytes(response.read())
        except urllib.error.HTTPError as exc:
            if exc.code == 404:
                return None
            if attempt == attempts - 1:
                raise
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            if attempt == attempts - 1:
                raise
        time.sleep(2.0 * (attempt + 1))
    return None


def listing(prefix: str, fields: str) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []
    token = ""
    while True:
        query = {"prefix": prefix, "delimiter": "/", "maxResults": "1000", "fields": fields}
        if token:
            query["pageToken"] = token
        page = json.loads(fetch(f"{API}?{urllib.parse.urlencode(query)}") or b"{}")
        found += [{"name": name} for name in page.get("prefixes", [])] + page.get("items", [])
        token = page.get("nextPageToken", "")
        if not token:
            return found


def build_record(job: str, build: str) -> dict[str, Any]:
    base = f"{OBJECTS}/logs/{job}/{build}"
    started = json.loads(fetch(f"{base}/started.json") or b"null") or {}
    finished = json.loads(fetch(f"{base}/finished.json") or b"null") or {}
    return {
        "build": build,
        "started": started.get("timestamp"),
        "commit": started.get("repo-commit"),
        "result": finished.get("result"),
    }


def failed_tests(job: str, build: str) -> list[str]:
    """Top-level Go tests the build's JUnit report names as failed, in report order."""
    names: dict[str, None] = {}
    reports = listing(f"logs/{job}/{build}/artifacts/junit", "items(name)")
    for report in reports:
        if not report["name"].endswith(".xml"):
            continue  # beside the report sits the whole test output, hundreds of megabytes
        text = (fetch(f"{OBJECTS}/{report['name']}") or b"").decode("utf-8", "replace")
        # The report is XML; `&#xA;` is the line break inside a failure's text.
        for name in FAILED_TEST.findall(text.replace("&#xA;", "\n")):
            names[name.split("/", 1)[0]] = None
    return list(names)


def choose(job: str, builds: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_commit: dict[str, list[dict[str, Any]]] = {}
    for record in builds:
        if record["commit"] and record["started"]:
            by_commit.setdefault(record["commit"], []).append(record)
    cases = []
    for record in builds:
        if record["result"] != "FAILURE" or not record["commit"] or not record["started"]:
            continue
        passing = [b for b in by_commit[record["commit"]] if b["result"] == "SUCCESS"]
        passing.sort(key=lambda b: abs(b["started"] - record["started"]))
        if not passing:
            continue
        tests = failed_tests(job, record["build"])
        if not tests:
            continue  # a build that failed without a test failing: a timeout, a panic
        cases.append(
            {
                "job": job,
                "commit": record["commit"],
                "failed": record["build"],
                "failed_started": record["started"],
                "failed_tests": tests,
                "baselines": [b["build"] for b in passing[:BASELINES]],
                "baselines_started": [b["started"] for b in passing[:BASELINES]],
                "control": passing[BASELINES]["build"] if len(passing) > BASELINES else None,
            }
        )
    return cases


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=HERE / "manifest.json")
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--workers", type=int, default=24)
    parser.add_argument("--rescan", action="store_true")
    args = parser.parse_args()

    if args.rescan:
        cases: list[dict[str, Any]] = []
        for job in JOBS:
            names = [
                row["name"].rstrip("/").rsplit("/", 1)[1]
                for row in listing(f"logs/{job}/", "prefixes,nextPageToken")
                if row["name"].endswith("/")
            ]
            with ThreadPoolExecutor(args.workers) as pool:
                builds = list(
                    pool.map(lambda b, job=job: build_record(job, b), sorted(names, key=int))
                )
            chosen = choose(job, builds)
            failed = sum(b["result"] == "FAILURE" for b in builds)
            print(f"{job}: {len(builds)} builds, {failed} failed, {len(chosen)} cases", file=sys.stderr)
            cases += chosen
        manifest = {
            "source": f"https://storage.googleapis.com/{BUCKET}/logs/<job>/<build>/build-log.txt",
            "listed": datetime.now(tz=timezone.utc).isoformat(timespec="seconds"),
            "cases": cases,
        }
        args.manifest.write_text(json.dumps(manifest, indent=1) + "\n")
    else:
        manifest = json.loads(args.manifest.read_text())

    wanted = {
        (case["job"], build)
        for case in manifest["cases"]
        for build in (case["failed"], *case["baselines"], case["control"])
        if build
    }

    def download(item: tuple[str, str]) -> int:
        job, build = item
        target = args.cache / job / f"{build}.log"
        if target.exists():
            return 0
        body = fetch(f"{OBJECTS}/logs/{job}/{build}/build-log.txt")
        if body is None:
            return 0
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body)
        return len(body)

    with ThreadPoolExecutor(args.workers) as pool:
        sizes = list(pool.map(download, sorted(wanted)))
    print(f"{len(wanted)} logs, {sum(sizes) / 1e6:.1f} MB fetched", file=sys.stderr)


if __name__ == "__main__":
    main()
