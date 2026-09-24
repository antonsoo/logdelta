# Changelog

All notable changes to this project are documented in this file.
Format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.1.0] - 2026-09-24

Initial release.

### Added

- `logdelta diff <baseline>... --target <file>`: NEW / GONE / CHANGED template findings,
  scored with a smoothed G-test and a flakiness penalty across multiple baselines, plus
  NEW VALUE findings for a same-frequency content flip at a low-cardinality, established,
  non-identifier-like position (e.g. a test's outcome going from `PASSED` in every baseline
  to `FAILED`) — tuned so a diff between two passing runs stays near-silent; `-C N` context
  lines; human (colored, raw lines truncated to the terminal width), `--json`, and
  `--markdown` output.
- `logdelta templates <file>`: ranked template list with counts and an example line.
- `logdelta novel --baseline <file>... [target|-]`: streaming novelty filter, flushes per
  line, works with `tail -f`.
- Structural envelope stripping (CRI/containerd, journald/syslog, Docker `json-file`, bare
  leading timestamps) and JSON-aware tokenization (single-line JSON objects flatten into
  `key=`/value token pairs) before template mining.
- Masking for timestamps (ISO 8601/RFC 3339, syslog, epoch), UUIDs, hex ids/hashes (any
  length), IPv4/v6 with ports, emails, URL query strings, basic-auth credentials, and
  numeric/hex/UUID path segments, quantities and durations, temp paths, and ANSI escapes;
  `--mask REGEX` and `--mask-file` for custom rules.
- A Drain-inspired online template miner (He, Zhu, Zheng, Lyu, ICWS 2017), with a similarity
  rule that ignores incidental placeholder-vs-placeholder matches so unrelated lines can't
  merge into a useless over-generalized template.
- Transparent `.gz` and stdin input, lossy non-UTF-8 handling.
