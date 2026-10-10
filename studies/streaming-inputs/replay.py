#!/usr/bin/env python3
"""Replay encoded logs through real CLI pipes; Python standard library only.

Run from the repository root. --before is optional; it records failures without
requiring an old binary to succeed. Every current-binary case must pass.
"""

from __future__ import annotations

import argparse
import codecs
import gzip
import hashlib
import json
import platform
from queue import Empty, Queue
from pathlib import Path
import subprocess
import tempfile
from threading import Thread
import time
import zlib

ROOT = Path(__file__).resolve().parents[2]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def encodings(text: str) -> dict[str, bytes]:
    plain = {
        "utf8": text.encode(),
        "utf8-bom": codecs.BOM_UTF8 + text.encode(),
        "utf16-le": codecs.BOM_UTF16_LE + text.encode("utf-16-le"),
        "utf16-be": codecs.BOM_UTF16_BE + text.encode("utf-16-be"),
    }
    return plain | {f"gzip-{key}": gzip.compress(value, mtime=0) for key, value in plain.items()}


def invoke(binary: Path, args: list[str], data: bytes | None, split: int, delay: float):
    started = time.monotonic()
    proc = subprocess.Popen([str(binary), *args], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        if split and data:
            proc.stdin.write(data[:split])
            proc.stdin.flush()
            time.sleep(delay)
            data = data[split:]
        out, err = proc.communicate(data, timeout=30)
    except BaseException:
        proc.kill()
        proc.communicate()
        raise
    return proc.returncode, out, err, time.monotonic() - started


def result(name, payload, actual, expected, **extra):
    code, out, err, elapsed = actual
    ok = (code, out, err) == expected[:3]
    return {
        "case": name, "input_bytes": len(payload), "input_sha256": digest(payload),
        "passed": ok, "exit": code, "stdout_sha256": digest(out),
        "expected_exit": expected[0], "expected_stdout_sha256": digest(expected[1]),
        "stdout_excerpt_on_failure": None if ok else out[:240].decode(errors="replace"),
        "stderr": err.decode(errors="replace"), "elapsed_seconds": round(elapsed, 6),
        **extra,
    }


def live(binary, baseline, encoding, delay):
    # A flushable gzip stream deliberately has no footer until AFTER the line is received.
    raw = encodings("x\n")[encoding.removeprefix("gzip-")]
    compressor = zlib.compressobj(wbits=31) if encoding.startswith("gzip-") else None
    payload = compressor.compress(raw) + compressor.flush(zlib.Z_SYNC_FLUSH) if compressor else raw
    proc = subprocess.Popen([str(binary), "novel", "--baseline", str(baseline)],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    received = Queue()
    thread = Thread(target=lambda: received.put(proc.stdout.readline()), daemon=True)
    thread.start()
    started = time.monotonic()
    first = b""
    got_line = False
    before_eof = False
    try:
        proc.stdin.write(payload[:1])
        proc.stdin.flush()
        time.sleep(delay)
        proc.stdin.write(payload[1:])
        proc.stdin.flush()
        try:
            first = received.get(timeout=3)
            got_line = True
            before_eof = first == b"x\n" and proc.poll() is None
        except Empty:
            pass
        latency = time.monotonic() - started
        if compressor:
            proc.stdin.write(compressor.flush(zlib.Z_FINISH))
            proc.stdin.flush()
        proc.stdin.close()
        proc.stdin = None
        thread.join(timeout=5)
        if thread.is_alive():
            raise RuntimeError("line reader did not finish after EOF")
        if not got_line:
            first = received.get_nowait()
        out, err = proc.communicate(timeout=5)
    except BaseException:
        proc.kill()
        proc.communicate()
        raise
    return {
        "case": f"live/{encoding}", "input_sha256": digest(payload),
        "line_before_eof": before_eof, "exit": proc.returncode,
        "passed": before_eof and proc.returncode == 1 and first + out == b"x\n" and not err,
        "line_wait_seconds": round(latency, 6),
        "stdout": (first + out).decode(errors="replace"), "stderr": err.decode(errors="replace"),
    }


def run(binary, directory, delay, loghub):
    baseline = directory / "baseline.log"
    baseline.write_text("known status ready\n", encoding="utf-8")
    text = "known status ready\nERROR café 🔥 disk unavailable\n"
    args = ["novel", "--baseline", str(baseline)]
    expected = (1, "ERROR café 🔥 disk unavailable\n".encode(), b"")
    cases = []
    for encoding, payload in encodings(text).items():
        for split in (0, 1, 2):
            actual = invoke(binary, args, payload, split, delay)
            cases.append(result(f"novel/{encoding}/first-{split or 'whole'}", payload, actual, expected))
    members = gzip.compress(b"", mtime=0) + gzip.compress(codecs.BOM_UTF8, mtime=0)
    members += gzip.compress(text.encode(), mtime=0)
    cases.append(result("novel/concatenated-gzip/first-1", members,
                        invoke(binary, args, members, 1, delay), expected))

    # Identical semantic records must produce the entire same report, including raw source
    # evidence and group denominators. This capture contains real loopback HTTP responses.
    capture = ROOT / "examples/http-rates"
    target = (capture / "http-rate-failed.log").read_bytes()
    rate_args = ["diff", str(capture / "http-rate-good.log"), str(capture / "http-rate-good-2.log"),
                 "--target", "-", "--watch-field", "/http/status", "--watch-by", "/route",
                 "--watch-rate-change", "5", "--json"]
    reference = invoke(binary, rate_args, target, 0, delay)
    assert reference[0] == 1 and not reference[2], reference[:3]
    for encoding, payload in encodings(target.decode()).items():
        cases.append(result(f"http-rate-report/{encoding}/first-1", payload,
                            invoke(binary, rate_args, payload, 1, delay), reference,
                            source_sha256=digest(target)))

    # A corrupt compressed log must not be accepted as a successful comparison of gibberish.
    valid = gzip.compress(text.encode(), mtime=0)
    bad_crc = bytearray(valid)
    bad_crc[-8] ^= 0x01
    for name, payload in {"header-only": valid[:2], "truncated": valid[:-5],
                          "bad-crc": bytes(bad_crc)}.items():
        code, out, err, _ = invoke(binary, ["templates", "-", "--json"], payload, 1, delay)
        cases.append({"case": f"invalid-gzip/{name}/first-1", "passed": code == 2 and not out and bool(err),
                      "exit": code, "stdout_sha256": digest(out), "stderr": err.decode(errors="replace"),
                      "input_sha256": digest(payload)})
    cases.extend(live(binary, baseline, encoding, delay) for encoding in encodings("x\n"))

    sources = []
    if loghub:
        paths = sorted(loghub.glob("*_2k.log"))
        if len(paths) != 16:
            raise ValueError(f"expected 16 Loghub sample files, found {len(paths)}")
        for path in paths:
            raw = path.read_bytes()
            # Preserve Logdelta's existing invalid-UTF-8 replacement convention when
            # re-encoding public logs that contain non-UTF-8 bytes.
            text = raw.decode("utf-8", errors="replace")
            ref = invoke(binary, ["templates", "-", "--json"], text.encode(), 0, delay)
            assert ref[0] == 0 and not ref[2], ref[:3]
            sources.append({"file": path.name, "bytes": len(raw), "sha256": digest(raw),
                            "decoded_utf8_sha256": digest(text.encode()),
                            "total_lines": json.loads(ref[1])["total_lines"]})
            for encoding, payload in encodings(text).items():
                cases.append(result(f"loghub/{path.stem}/{encoding}/first-1", payload,
                                    invoke(binary, ["templates", "-", "--json"], payload, 1, delay), ref))
    return {"binary_sha256": digest(binary.read_bytes()), "cases": cases,
            "passed": sum(row["passed"] for row in cases), "total": len(cases), "loghub_sources": sources}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--before", type=Path)
    parser.add_argument("--loghub", type=Path, help="optional studies/loghub/cache directory")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--delay", type=float, default=0.1, help="seconds between first write and remainder")
    args = parser.parse_args()
    if args.delay < 0.01:
        parser.error("--delay must be at least 0.01 seconds")
    report = {"schema_version": 1, "platform": platform.platform(), "python": platform.python_version(),
              "header_write_delay_seconds": args.delay,
              "live_line_deadline_seconds": 3,
              "driver_sha256": digest(Path(__file__).read_bytes())}
    with tempfile.TemporaryDirectory(prefix="logdelta-streams-") as scratch:
        for key, binary in (("before", args.before), ("current", args.binary)):
            if binary:
                report[key] = run(binary.resolve(), Path(scratch), args.delay, args.loghub)
                print(f"{key}: {report[key]['passed']}/{report[key]['total']} cases passed", flush=True)
    if "before" in report:
        references = [
            {row["case"]: (row["expected_exit"], row["expected_stdout_sha256"])
             for row in report[key]["cases"] if "expected_stdout_sha256" in row}
            for key in ("before", "current")
        ]
        report["unchanged_reference_outputs"] = references[0] == references[1]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    if report["current"]["passed"] != report["current"]["total"]:
        raise SystemExit("current CLI failed: inspect the retained report")
    if report.get("unchanged_reference_outputs") is False:
        raise SystemExit("unfragmented reference outputs changed: inspect the retained report")


if __name__ == "__main__":
    main()
