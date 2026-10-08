"""Download the Loghub-2k benchmark: 2,000 labelled lines from each of 16 systems.

    python3 studies/loghub/fetch.py

Loghub (https://github.com/logpai/loghub) is free for research and academic
work, with attribution; see its LICENSE. The datasets are not committed here:
this fetches each system's sample log and its hand-labelled templates, at a
pinned commit, into studies/loghub/cache/.

    Jieming Zhu, Shilin He, Pinjia He, Jinyang Liu, Michael R. Lyu. Loghub: A
    Large Collection of System Log Datasets for AI-driven Log Analytics.
    ISSRE 2023.
"""

from __future__ import annotations

import urllib.request
from pathlib import Path

HERE = Path(__file__).parent
COMMIT = "dd61d0952749ee7963bde24220d1be5ede023033"
BASE = f"https://raw.githubusercontent.com/logpai/loghub/{COMMIT}"
SYSTEMS = (
    "HDFS",
    "Hadoop",
    "Spark",
    "Zookeeper",
    "OpenStack",
    "BGL",
    "HPC",
    "Thunderbird",
    "Windows",
    "Linux",
    "Mac",
    "Android",
    "HealthApp",
    "Apache",
    "OpenSSH",
    "Proxifier",
)


def main() -> None:
    cache = HERE / "cache"
    cache.mkdir(exist_ok=True)
    for name in ("LICENSE", *(f"{s}/{s}_2k.log{suffix}" for s in SYSTEMS for suffix in ("", "_structured.csv"))):
        target = cache / name.rsplit("/", 1)[-1]
        if not target.exists():
            request = urllib.request.Request(f"{BASE}/{name}", headers={"User-Agent": "logdelta-study"})
            with urllib.request.urlopen(request, timeout=120) as response:
                target.write_bytes(response.read())
        print(f"{target.name}: {target.stat().st_size:,} bytes")


if __name__ == "__main__":
    main()
