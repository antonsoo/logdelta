"""Capture controlled application events with python-json-logger==4.2.0.

This is a real formatter invocation with constructed outcomes and fixed UTC
record times, not captured production traffic. It does not use Docker.
"""

import hashlib
import importlib.metadata
import io
import json
import logging
import platform
import time
from pathlib import Path

from pythonjsonlogger.json import JsonFormatter


def capture(status, message):
    output = io.StringIO()
    handler = logging.StreamHandler(output)
    logger = logging.Logger("controlled-checkout", level=logging.INFO)
    logger.addHandler(handler)
    for index, renamed in enumerate((False, True)):
        formatter = JsonFormatter(
            "%(asctime)s %(message)s",
            rename_fields={"asctime": "time", "message": "log"} if renamed else {},
            datefmt="%Y-%m-%dT%H:%M:%SZ",
        )
        formatter.converter = time.gmtime
        handler.setFormatter(formatter)
        record = logger.makeRecord(
            logger.name, logging.INFO, __file__, 0,
            message if renamed else "health check passed", (), None,
            extra={"route": "/checkout", "http": {"status": status if renamed else 200}},
        )
        record.created = 1_792_065_600 + index
        record.msecs = 0
        logger.handle(record)
    rows = output.getvalue()
    decoded = [json.loads(row) for row in rows.splitlines()]
    assert [row["http"]["status"] for row in decoded] == [200, status]
    assert decoded[1]["log"] == message and "time" in decoded[1]
    return rows.encode()


def main():
    root = Path(__file__).resolve().parent
    inputs = root / "inputs"
    inputs.mkdir(exist_ok=True)
    files = {}
    for kind, message in [("native", "request completed"), ("native-shadow", '{"http":{"status":200}}')]:
        for side, status in [("baseline", 200), ("target", 503)]:
            name = f"{kind}-{side}.log"
            data = capture(status, message)
            (inputs / name).write_bytes(data)
            files[name] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    receipt = {
        "scope": "Controlled Python logging calls; fixed timestamps, constructed statuses; no incident or Docker daemon.",
        "python": platform.python_version(),
        "python_json_logger": importlib.metadata.version("python-json-logger"),
        "renamed_fields": {"asctime": "time", "message": "log"},
        "inputs": files,
    }
    (root / "results").mkdir(exist_ok=True)
    (root / "results" / "logger.json").write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
