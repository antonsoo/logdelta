"""Compress retained HTTP observations with GNU gzip, with unchanged plain bytes."""
from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
MAX_BYTES = 25 * 1024 * 1024


def gzip(data):
    return subprocess.check_output(["gzip", "--no-name", "--stdout"], input=data)


def capture():
    output = HERE / "captures"
    output.mkdir(parents=True, exist_ok=True)
    observations = []

    def write(name, members, plain=None, valid=True):
        body = b"".join(members)
        path = output / name
        path.write_bytes(body)
        result = subprocess.run(["gzip", "--decompress", "--stdout", str(path)], capture_output=True)
        assert (result.returncode == 0) == valid, (name, result.stderr)
        if valid:
            assert result.stdout == plain
        observations.append({"file": name, "bytes": len(body),
                             "sha256": hashlib.sha256(body).hexdigest(), "members": len(members),
                             "plain_bytes": len(plain) if plain is not None else None,
                             "plain_sha256": hashlib.sha256(plain).hexdigest() if plain is not None else None,
                             "gnu_gunzip_exit": result.returncode})

    for label, source in [("good", "http-rate-good.log"), ("good-2", "http-rate-good-2.log"),
                          ("failed", "http-rate-failed.log")]:
        path = ROOT / "examples/http-rates" / source
        plain = path.read_bytes()
        lines = plain.splitlines(keepends=True)
        # The first gzip member contains only startup. Every actual HTTP status
        # follows it; accepting just the first member cannot represent this run.
        parts = [lines[:1], *[lines[i:i + 500] for i in range(1, len(lines), 500)]]
        members = [gzip(b"".join(part)) for part in parts]
        write(f"{label}.single.gz", [gzip(plain)], plain)
        write(f"{label}.multi.gz", members, plain)
        observations[-1]["source"] = path.relative_to(ROOT).as_posix()
        if label == "failed":
            crc = bytearray(members[1]); crc[-8] ^= 1
            write("bad-second-crc.gz", [members[0], bytes(crc), *members[2:]], valid=False)
            size = bytearray(members[-1]); size[-1] ^= 1
            write("bad-last-size.gz", [*members[:-1], bytes(size)], valid=False)
            write("truncated-last-member.gz", [*members[:-1], members[-1][:-4]], valid=False)
            write("trailing-junk.gz", [*members, b"junk"], valid=False)
            observations[-1]["members"] = len(members)
    unicode_text = '{"message":"café 🔥", "status":503}\r\n'
    for label, raw, boundary in [
        ("utf8-split", unicode_text.encode(), unicode_text.encode().index("🔥".encode()) + 1),
        ("utf16-split-bom", b"\xff\xfe" + unicode_text.encode("utf-16le"), 1),
        ("utf16-split-unit", b"\xff\xfe" + unicode_text.encode("utf-16le"), 7),
        ("literal-replacement", 'a\ufffd\n'.encode(), 2),
        ("malformed-utf8", b"a\xff\n", 1),
    ]:
        write(f"{label}.multi.gz", [gzip(raw[:boundary]), gzip(raw[boundary:])], raw)
    raw = b"first\n" + b"x" * MAX_BYTES
    write("expanded-over-limit.multi.gz", [gzip(raw[:6]), gzip(raw[6:])], raw)
    raw = b"\xff\xfe" + "\u0800".encode("utf-16le") * ((MAX_BYTES - 2) // 2)
    write("decoded-over-limit.multi.gz", [gzip(raw[:1]), gzip(raw[1:])], raw)
    manifest = {"producer": subprocess.check_output(["gzip", "--version"], text=True).splitlines()[0],
                "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                                           text=True).strip(),
                "scope": "Existing controlled loopback HTTP responses; encoding/limit/corruption controls authored",
                "captured_http_responses": 6000, "files": observations}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"files": len(observations), "http_logs": 3, "actual_http_responses": 6000}))


if __name__ == "__main__":
    capture()
