#!/usr/bin/env python3
"""Print an installed CLI's real pipe results for the documentation image."""

from __future__ import annotations

import argparse
import subprocess
import tempfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    payload = b"startup complete\npanic: unavailable database\n"
    print("Repeated stdin: novel --baseline - (default target is also stdin)")
    invalid = subprocess.run(
        [str(cli), "novel", "--baseline", "-"],
        input=payload,
        capture_output=True,
        check=False,
    )
    assert invalid.returncode == 2 and not invalid.stdout
    print(invalid.stderr.decode(), end="")
    print(f"exit: {invalid.returncode}; stdout: {len(invalid.stdout)} bytes")
    print()
    with tempfile.TemporaryDirectory(prefix="logdelta-demo-") as temporary:
        baseline = Path(temporary) / "baseline.log"
        baseline.write_text("startup complete\n")
        print("Single stdin: separate file baseline, piped target")
        valid = subprocess.run(
            [str(cli), "novel", "--baseline", str(baseline)],
            input=payload,
            capture_output=True,
            check=False,
        )
        assert valid.returncode == 1 and not valid.stderr
        assert valid.stdout == b"panic: unavailable database\n"
        print(valid.stdout.decode(), end="")
        print(f"exit: {valid.returncode}; one novel line")


if __name__ == "__main__":
    main()
