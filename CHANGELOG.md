# Changelog

All notable changes to this project are documented in this file.
Format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.1.0] - 2026-09-24

Initial release.

### Added

- `logdelta diff <baseline>... --target <file>`: NEW / GONE / CHANGED template findings,
  scored with a smoothed G-test and a flakiness penalty across multiple baselines; `-C N`
  context lines; human (colored), `--json`, and `--markdown` output.
- `logdelta templates <file>`: ranked template list with counts and an example line.
- `logdelta novel --baseline <file>... [target|-]`: streaming novelty filter, flushes per
  line, works with `tail -f`.
- Masking for timestamps (ISO 8601/RFC 3339, syslog, epoch), UUIDs, hex ids/hashes, IPv4/v6
  with ports, emails, URL query strings and numeric/hex/UUID path segments, quantities and
  durations, temp paths, and ANSI escapes; `--mask REGEX` and `--mask-file` for custom rules.
- A Drain-inspired online template miner (He, Zhu, Zheng, Lyu, ICWS 2017).
- Transparent `.gz` and stdin input, lossy non-UTF-8 handling.
