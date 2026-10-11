"""Retain native exact-watch behavior for valid characters and damaged encoded logs.

All inputs are synthetic. The same payload is supplied as a file, stdin, or gzip;
ordinary comparisons remain available while exact watches must reject decoding loss.
"""

from __future__ import annotations

import argparse
import base64
import codecs
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def cases() -> list[tuple[str, bytes, int]]:
    literal = '{"route":"caf\ufffd"}\n'
    invalid = '{"route":"caf\ud800"}\n'
    return [
        ("literal-utf8", literal.encode(), 0),
        ("literal-utf8-bom", codecs.BOM_UTF8 + literal.encode(), 0),
        ("literal-utf16-le", codecs.BOM_UTF16_LE + literal.encode("utf-16le"), 0),
        ("literal-utf16-be", codecs.BOM_UTF16_BE + literal.encode("utf-16be"), 0),
        ("valid-nonbmp", '{"route":"café/🔥"}\n'.encode(), 1),
        ("invalid-utf8", b'{"route":"caf\xff"}\n', 2),
        ("invalid-utf16-le", codecs.BOM_UTF16_LE + invalid.encode("utf-16le", "surrogatepass"), 2),
        ("invalid-utf16-be", codecs.BOM_UTF16_BE + invalid.encode("utf-16be", "surrogatepass"), 2),
        ("odd-utf16-tail", codecs.BOM_UTF16_LE + literal.encode("utf-16le") + b"\x61", 2),
    ]


def run(binary: Path, payload: bytes, mode: str, watched: bool, bad_baseline: bool) -> dict:
    literal = '{"route":"caf\ufffd"}\n'.encode()
    with tempfile.TemporaryDirectory(prefix="logdelta-text-") as directory:
        root = Path(directory)
        (root / "baseline.log").write_bytes(payload if bad_baseline else literal)
        (root / "target.log").write_bytes(literal if bad_baseline else payload)
        args = [str(binary), "diff", "baseline.log", "--target", "-" if mode == "stdin" else "target.log", "--json"]
        if watched:
            args += ["--watch-field", "/route"]
        completed = subprocess.run(args, cwd=root, input=payload if mode == "stdin" else None,
                                   capture_output=True, timeout=30, check=False)
        body = json.loads(completed.stdout) if completed.stdout else None
        return {"exit": completed.returncode, "stdout_sha256": digest(completed.stdout),
                "report": body, "stderr": completed.stderr.decode()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expect-correct", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve()
    results = []
    for name, data, expected in cases():
        for compressed in (False, True):
            payload = gzip.compress(data, mtime=0) if compressed else data
            for mode in ("file", "stdin", "baseline"):
                if mode == "baseline" and expected != 2:
                    continue
                watched = run(binary, payload, mode, True, mode == "baseline")
                ordinary = run(binary, payload, mode, False, mode == "baseline")
                if args.expect_correct:
                    assert watched["exit"] == expected, (name, compressed, mode, watched)
                    if expected == 2:
                        assert watched["report"] is None
                        assert "exact field watches require valid" in watched["stderr"]
                results.append({"case": name, "gzip": compressed, "mode": mode,
                                "input_sha256": digest(payload), "input_base64": base64.b64encode(payload).decode(),
                                "expected_exit": expected, "watch": watched, "ordinary": ordinary})
    args.out.parent.mkdir(parents=True, exist_ok=True)
    result = {"binary_sha256": digest(binary.read_bytes()), "cases": results}
    args.out.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(results)} cases; {sum(r['watch']['exit'] == r['expected_exit'] for r in results)} expected exits")


if __name__ == "__main__":
    main()
