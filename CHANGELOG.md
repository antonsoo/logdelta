# Changelog

All notable changes to this project are documented in this file.
Format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.3.2] - 2026-10-02

### Fixed

- Logs saved by Windows tools. `pytest > run.log` in Windows PowerShell writes UTF-16 with a
  byte-order mark: compared with UTF-8 baselines from CI, such a target shared no line with
  them and all of it was reported as new (34 templates and 9 findings for the pytest example,
  which has 21 and 3). A mark in front of UTF-8 made the log's first line a template of its
  own, one extra finding. Both encodings are recognized by the mark and read as the text
  they hold, in files, in `.gz` files and on stdin; the reports are then identical to those
  for the plain files. The web demo reads dropped files the same way.

## [0.3.1] - 2026-10-02

### Fixed

- The minimum Rust version. `rust-version` said 1.80, and on 1.80
  `cargo install logdelta` stopped inside a dependency: `failed to parse manifest ...
  clap_derive-4.6.7/Cargo.toml: feature edition2024 is required`. clap has needed Rust 1.85
  since 4.6. `rust-version` is 1.85 now, so an older toolchain is told that in one line, and
  CI has a job that builds and tests on 1.85.0 with the locked dependencies. No code changed.

## [0.3.0] - 2026-10-01

Checked against real CI logs for the first time: a failing `pytest` job of the pytest
project on GitHub Actions (854 lines) and three passing runs of the same job (about 1,270
lines each), then a vitest job and a tokio job the same way. The numbers below are 0.2.3 from
crates.io and this release on the same files.

### Added

- **Blocks.** Findings whose lines sit together are reported as one. The pytest failure was
  311 findings, 543 lines of report: every line of the traceback its own NEW finding, every
  line of the upload steps the job never reached its own GONE finding. It is now 4: the
  traceback (51 templates), the short test summary (27), the lines the job exited on (5),
  and one GONE block of 198 templates. vitest: 26 findings to 6. tokio: 99 to 13.
  - A NEW block is a run of new lines, with up to two known lines let through, found while
    the target is read. It prints one line per template with its line number, and `×N`
    where a template has more lines. The same stack trace logged 25 times is one block that
    says so.
  - A GONE block is located in the first baseline, the run that still had the lines.
  - A template folds into a block only if all of its lines are in such groups, so a message
    that also occurs on its own keeps its own finding and its count. Grouping never makes a
    report longer.
  - `--block-lines N` (default 12) sets how much of a long block is printed: its start and
    its end. `--flat` turns grouping off.
  - `--json` gains a `blocks` array and a `block` index on each finding that is part of
    one; `findings` still lists every template. Memory stays flat: a few numbers per
    template, nothing per line.
- A GONE finding shows the line the first baseline had, with its line number
  (`first_baseline_line_no` and `first_baseline_raw` in `--json`), where it used to show the
  template alone.
- Test-runner progress lines are masked: `....s....x.... [ 42%]` becomes
  `<PROGRESS> [ <NUM>%]`, or `<PROGRESS!>` when it holds an `F` or `E`. Which tests land on
  which line depends on scheduling, so two passing runs of the pytest job diffed to 27
  findings, all of them dots. Now 0.
- OSC escape sequences (hyperlinks, window titles) are stripped before mining, as CSI color
  codes already were.
- Library: `blocks::Block`, `DiffResult::blocks`, `DiffOptions::group`,
  `scoring::count_g_test`, `Drain::add_token_slice`, `mask::strip_escapes`,
  `output::printable`.

### Changed

- **CHANGED needs the count to move, not only the share.** The score compares a template's
  share of the log, and a share moves whenever the run is shorter or longer for other
  reasons: the failing pytest job printed its 257 `[new branch]` lines 258 times in a log a
  third shorter, and that was a CHANGED finding ("more often", score 38). A second G-test on
  the counts alone (the baselines' total against the target's, as two Poisson counts) now
  has to clear the same cutoff, in the same direction. A bigger run with the same mix still
  changes nothing, which is what the share test is for.
- A NEW VALUE on a line inside a NEW block is part of the block and is not listed again.
- Reports print log lines without the log's own escape sequences and control characters.
  A colored CI log put its color codes into the terminal report (76 lines of the pytest
  report carried them, also under `--color never`), where a line cut to the terminal width
  left the color on for everything after it, and into Markdown tables as stray `[31m`.
  `--json` keeps every line as it was read.
- The header counts what the report shows and says what `--flat` would:
  `4 findings (284 with --flat)`.
- The web demo shows the engine's blocks (it grouped new lines on its own before, by first
  line only), has a GONE block card, cuts a long block to its start and end with a button
  for the rest, and strips color codes from the lines it shows.

### Fixed

- **A line that is mostly placeholders never matched its own template.** The similarity
  between a line and a template skipped positions where both hold the same masking
  placeholder, but still divided by the full length, so `<NUM> <NUM> <HEX> ok` scored 1/4
  against itself and every such line started a new template. 100,000 lines of that shape
  were 100,000 templates, 77.55 s and 45.7 MB in `templates` (each line compared against
  every template before it); they are one template in 0.18 s and 7.2 MB. In a diff, a line
  identical in every run was reported NEW: the toolchain line
  `- 1.98.1 aarch64-apple-darwin 48a229ce…` in tokio's logs, between two passing runs.
  Such positions are now left out of both sides of the fraction.

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
