# Encoding headers delivered across pipe writes

A pipe can return one byte even when its reader requests 256 KiB. At `1baa53b`,
Logdelta inspected only that first buffer for gzip magic or a Unicode byte-order
mark. Sending the first byte separately made a UTF-8 mark part of a template,
turning a known line into a finding. UTF-16 and compressed logs were processed as
binary garbage. Three damaged gzip controls even produced successful template
reports instead of read errors.

`7ac1d80` joins short initial reads only while they could still be a recognized
header. The replay buffer holds at most three bytes. A normal file retains its
existing buffer, and ordinary text does not wait for a fixed-size prefix or EOF.
The miner, masks and scoring rules are unchanged. This is a source fix;
the crates.io version remains 0.3.4.

## Actual CLI replay

[`replay.py`](replay.py) launches real CLI subprocesses, writes the first one or
two bytes, waits 100 ms, then writes the remainder. Expected lines come from the
labelled synthetic example. Expected HTTP and public-corpus reports come from
the same records delivered without a split header; the old and new reference
outputs must also match each other.

| Workflow | Before | Fresh Rust 1.85 installation |
| --- | ---: | ---: |
| Synthetic known/new lines, eight encodings and concatenated gzip | 16/25 | 25/25 |
| Complete JSON rate report for the existing HTTP capture | 1/8 | 8/8 |
| Truncated header, truncated stream and bad gzip checksum refused | 0/3 | 3/3 |
| Exact live line received while the writer remains open | 1/8 | 8/8 |
| Public Loghub template reports, 16 systems times eight encodings | 16/128 | 128/128 |
| **Total** | **34/172** | **172/172** |

The eight encodings are UTF-8, UTF-8 with a mark, both marked UTF-16 byte orders,
and gzip around each. Live gzip streams use `Z_SYNC_FLUSH`; the footer is sent
only **after** the output line is received. The live check has a three-second
deadline and checks that the child is still running with stdin open. It does
not merely inspect buffered output after process exit.

The HTTP case reuses the [controlled 6,000-response capture](../../examples/http-rates/README.md).
All eight complete JSON outputs match the plain-text reference byte for byte,
including exact grouped counts, denominators, source records and scores.
The [Loghub source pins](loghub-source-pins.json) record all 16 remote files
re-fetched at `dd61d0952749ee7963bde24220d1be5ede023033`: 4,458,875 bytes and
32,000 lines. Their templates, counts and source examples survive every encoding.
Non-UTF-8 bytes in the original corpus use the existing replacement-character
convention before re-encoding. This is an input-equivalence check, not a new
measurement of template grouping accuracy.

The [retained results](results.json) contain binary, driver, input and output
hashes, every case outcome, failure excerpts and live delivery observations.
The [checks](checks.json) identify the implementation revision and clean build.
The old executable was built from `1baa53b` with Rust 1.91; the final executable
was installed from a fresh checkout of `7ac1d80` with Rust 1.85. Both use the
committed lockfile.

## Reproduce

From the repository root, with Python 3.10+ and the required Rust toolchain:

```sh
cargo build --locked --release
python3 studies/loghub/fetch.py
python3 studies/streaming-inputs/replay.py --binary target/release/logdelta \
  --loghub studies/loghub/cache --output /tmp/logdelta-streams.json
```

To compare the previous source, build `1baa53b` in a separate checkout and add
`--before /path/to/old/logdelta`. Omit `--loghub` for the 44 self-contained cases.
The script exits nonzero if any current case fails or if the unsplit references
change. Corpus data stays in the ignored cache, under the source's existing
[license and attribution](../loghub/README.md).

The deterministic Rust checks additionally exercise one-to-nine-byte reads
through the complete opener, partial/mismatched signatures, interrupted reads
and a reader that fails if asked for bytes beyond an already complete line.
Both the current toolchain and Rust 1.85 passed 211 tests and one doctest.

## Limits and cost

This replay ran on 14-vCPU WSL2 Linux with 48 GB RAM. Delayed pipe writes do not
enumerate every OS scheduling pattern; the deterministic reader checks cover
the byte boundaries. No Windows or macOS process run was performed.

An unfinished possible header must wait for another byte or EOF. Streaming
output can precede a later gzip checksum failure; callers must still honor the
final exit status. Encoding detection does not validate the semantics of a log.

The [throughput record](throughput.json) compares the two Rust 1.91 executables
on 17,835,560 bytes (the 16 raw logs joined with newlines, repeated four times),
plain and gzipped. Three interleaved runs per executable gave median times of
0.961/0.964 seconds before/after for plain input and 0.757/0.744 for gzip.
Every full JSON output had the same hash. Other local build work overlapped the
measurement, so these are a coarse regression check, not a speed claim.
