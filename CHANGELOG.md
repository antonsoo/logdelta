# Changelog

All notable changes to this project are documented in this file.
Format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.2.3] - 2026-10-01

### Changed

- Reports show at most 400 characters of a template, a value or a log line,
  and say how much they left out. Logs do contain lines of hundreds of
  kilobytes (a minified bundle, a base64 payload, one JSON document), and one
  finding on such a line printed all of it: a 120 KB line made a 120 KB
  terminal report and a 240 KB Markdown table, too large for a PR comment.
  The same finding is now about 1 KB in either. `--json` still carries every
  line whole, and output for ordinary logs is unchanged.

## [0.2.2] - 2026-10-01

### Added

- Crate-level documentation with a tested example of the library API
  (`analysis::diff_lines` over two logs in memory), so the docs.rs page opens
  with more than a module list. The README has a short "As a library" section.

### Fixed

- Four documentation links pointed at private items and rendered as dead links.

## [0.2.1] - 2026-10-01

### Added

- Published to crates.io: `cargo install logdelta`. The crate leaves out the
  browser demo, the screenshots and the large benchmark fixtures.

## [0.2.0] - 2026-09-30

### Added

- A browser demo at <https://antonsoo.github.io/logdelta/> (`web/`): this crate's library
  compiled to `wasm32-unknown-unknown` through a small C-ABI wrapper crate (`web/wasm`, no
  wasm-bindgen), run in a Web Worker. Paste, open, or drop baseline and target logs (`.gz`
  included), or load the bundled synthetic examples (a pytest pass/fail pair, a Kubernetes
  incident, and the three-baseline CI run from `tests/fixtures/large`). Findings match
  `logdelta diff --json`; consecutive new lines are grouped into one card, and the result can
  be downloaded as JSON.
- `diff_lines` and `mine_lines`: the diff and template mining over any
  `Iterator<Item = io::Result<String>>`, so in-memory text needs no file. `diff_runs` is now a
  thin wrapper over `diff_lines`.

### Changed

- `clap` and `terminal_size` are behind a default `cli` feature; the library builds with
  `default-features = false` (the binary requires `cli`).

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
