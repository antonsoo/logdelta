# Changelog

All notable changes to this project are documented in this file.
Format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## Unreleased

### Changes in known field values' rates

- `--watch-rate-change PERCENT_POINTS` adds an opt-in rate comparison to exact
  field watches. A known value must move beyond every observed baseline rate
  by the requested amount and pass the existing variability-adjusted score.
  Denominators are matched field observations within the exact group, so changing
  traffic mix need not look like changing outcomes on an individual route.
- Terminal, Markdown and JSON reports retain counts, per-run denominators,
  baseline fractions, target fraction, effect, score and source evidence.
  Several changed values in one group count as one rate finding. Missing group
  observations or incomplete ledgers cannot pass a rate gate (exit 2).
- Available in the source CLI, Rust options, direct WASM request and browser.
  The browser adds an opt-in rate control, a replay of the captured HTTP experiment,
  count/denominator tables, baseline ranges, source records and unknown coverage.
  Rate findings are grouped, paged and filterable; sources render on demand.
  Version 3 portable reports retain the applied threshold and complete evidence
  through edits, searches and pagination. Default behavior is unchanged.
- [Controlled 6,000-response HTTP capture, independent oracle and limitations](docs/verification-field-rates.md).

### Browsing field evidence

- Watched-field ledgers load when opened, with case-insensitive search across exact
  group and value text, 32 rows per page, and source excerpts for the visible rows.
  Closing a ledger releases its page elements; reopening preserves its view.
- **New comparison** releases the controller's retained field data as well as
  clearing the report and resetting the engine.
- Filtering, paging, and closed folds never remove evidence from either download.
  Coverage always describes the whole input. Keyboard navigation includes the search
  box, page controls, tables, and source excerpts.

### Grouped field watches

- `--watch-by /route` compares exact watched values within each route, so a known
  503 on one route cannot hide a first 503 on another. Repeat for up to four
  components, such as service and route. The same engine powers the browser's
  **Compare within groups** control and mixed-route HTTP example.
- Reports distinguish new values in observed groups from groups absent from
  baseline observations. Counts, first source locations and context refer to
  the exact group/value pair; numeric spelling and JSON types remain intact.
- Missing, ambiguous or non-scalar group keys on selected records make the
  watch incomplete and exit 2. Grouped ledgers retain at most 256 pairs per
  field; limit overflow preserves counts and source problems without claiming
  a complete comparison. Ungrouped watches retain their existing JSON shape.
- Grouped browser downloads use report schema version 2 and record applied
  grouping pointers. Edits keep completed evidence until a successful new
  comparison; an engine that silently ignores grouping is rejected.
- [Controlled HTTP capture, verification and limits](docs/verification-grouped-fields.md).

### Diagnostic excerpts

- Terminal, Markdown and browser block previews keep source diagnostics and error
  markers visible in long blocks. Marked lines already visible at the boundaries are
  retained; additional markers replace ordinary middle lines within the existing budget.
  Every omitted stretch has a count and the remaining lines keep their source positions.
- The same 264 Kubernetes cases were run again. The default CLI now shows the study's
  recorded reason in 212 of 239 applicable cases (was 159), in a median of 40.5 report
  lines (was 34). It recovers 53 cases and loses none of the previous 159. All 264 native
  JSON reports are unchanged. This is an in-sample display improvement, not a change in
  finding detection or proof of a failure's cause.
- Long reports explain `--block-lines 0` and `--json`. The browser keeps one keyboard
  expansion control even when its preview has several gaps. Its raw-line preview and
  native template excerpt share diagnostic patterns.
- The Kubernetes evaluator rejects missing inputs, failed commands and empty reports
  instead of silently dropping cases; new results record binary and manifest digests.
- [Behavior, evidence and limitations](docs/diagnostic-excerpts.md).

### On real data

Two studies, in `studies/`, score the miner and the diff on public data with known answers.
What follows under "Template mining" is what they showed and what was changed for it.

- **Loghub-2k**, the benchmark log parsers are scored on: 2,000 hand-labelled lines from
  each of 16 systems. Grouping accuracy was 0.732 on the message text and 0.428 on whole
  lines; it is 0.815 and 0.707, with one configuration for all sixteen. Those two are
  in-sample: the changes below were chosen by reading this benchmark's errors. The
  reference Drain publishes 0.865 with settings chosen per system and scores 0.703 rerun
  with one setting for all. No system is worse on the message.
- **264 failed Kubernetes CI builds**, each diffed against a passing build of the same
  commit, with the failed test's name and its assertion line from the build's JUnit report.
  The diff names a failed test in all 264, before and after, as `grep -- '--- FAIL'` does.
  Before the diagnostic excerpt change above, it showed the test's own assertion line in
  159 of 239 cases, in a median of 34 lines (was 39). The median report went from 6 findings to 4 by grouping; the
  ungrouped count did not fall. A passing build against passing builds is silent in 127 of
  179 cases (was 122).

### Template mining

- **A new message in a JSON log is a new template.** Two JSON lines with the same keys
  agreed on every key, which was half their tokens and enough to make them one template
  whatever they said: a log of `"msg":"request handled"` with one new
  `"level":"error","msg":"database connection failed"` in it diffed to "No significant
  differences found." A field whose value is a sentence is now mined as its words, any
  other field is an attribute (`level="info"`, shown as `level=<*>` when it varies), and
  attributes and keys are structure: they must line up, and they are not evidence that two
  lines say the same thing. logfmt's `msg="..."` gets the same treatment.
- **A template no longer gets easier to join as it takes lines in.** A position that had
  become `<*>` counted as agreement with anything, so each line a template absorbed lowered
  the bar for the next. One template held 343 lines of 27 different Android log statements.
  A cluster's evidence is now the words of its first line, and a wildcard agrees with
  nothing, as in the Drain paper.
- **A line is filed under its first constant word, not its first token.** Messages that
  start with a value (`www.baidu.com:80 open through proxy …`,
  `attempt_1445144423722_0020_m_000000_0 TaskAttempt Transitioned …`,
  `1005 floating point alignment exceptions`) were a template per value.
- **Tokens are compared by shape**, every number in them written `#`.
  `blk_38865049064139660` and `blk_-7128370237687728475` are one token; so are klog's
  `I1004` and `I1005`, which made every line of a Kubernetes log new the day after the
  baseline was taken.
- **A short square-bracketed field is one token**: `[main]` and
  `[IPC Server handler 14 on 62270]` gave the lines of one Java log statement different
  lengths, and a template each.
- **More timestamps and durations are masked**: `ctime` (`Sun Dec 04 04:47:44 2005`, the
  form of Apache's error log, `date` and `git log`), Common Log Format
  (`04/Dec/2005:04:47:44 +0000`), RFC 2822, and durations in more than one unit
  (`1m6.046s`, `2h45m`).

### Added

- **GONE for one source of a shared template.** With instances no longer split into a
  template each, `[svc-search-3] health check ok` going silent while four other instances
  carry on is reported as GONE for that instance's line, with its baseline counts. Only
  when the position holds a fixed, small set of sources, every baseline had this one, and
  its share of each baseline put at least 8 lines in the target.
- NEW VALUE covers attributes: `level="error"` where every baseline had `level="info"`.
- `bench/cluster_ids.rs`, an example program that prints the template id of each line of
  a log, which is what a log-parsing benchmark scores.

### Changed

- JSON templates read `level="info" msg= server listening` where they read
  `level= "info" msg= "server listening"`.
- The CLI colours a placeholder wherever it is in a token (`trace=<UUID>`, `[<NUM>%]`).
- On this repository's synthetic three-baseline example the diff finds 16 templates where
  it found 407 (one per service instance per message), and the same 5 findings. Its passing
  runs against each other give 0 or 1 finding where they gave 0: the retry line, which the
  three runs print 200, 1,100 and 550 times, is now one template with a count that moved.
- About a quarter more CPU time per line (measured on the 1M-line benchmark log; see
  `bench/RESULTS.md`).

### Fixed

- Watched-field context has an 8 KiB window budget with explicit clipping labels,
  keeping large adjacent records from being copied in full for every new value.

- The web lockfile now uses source-map-js 1.2.2, fixing the indexed source-map
  offset denial of service in the build dependency (CVE-2026-93749).

- Browser builds stage Cargo's actual WASM artifact, including with custom target
  directories, and retain the previous staged engine when compilation, metadata or
  artifact selection fails. Rebuilds remove local source paths and stabilize Cargo's
  WASM crate metadata without dropping caller flags or compiler wrappers.
- The browser engine now has a fingerprinted asset URL tied to its worker build, avoiding
  an older module cached under the fixed `logdelta.wasm` URL after an interface update.
  HTTP failures, invalid binaries and incompatible module exports allow a fresh retry.
- Delayed browser imports keep their original baseline even when another tab is selected.
  Newer reads, edits, removal and reset cancel obsolete work; a failed read keeps the
  previous input. Example downloads follow the same ownership rules.
- Comparisons capture their inputs before analysis. Grouped excerpts no longer read a newer
  target after the worker finishes. Editing cancels pending analysis, and completed reports
  are visibly marked when their inputs change.
- Worker startup, message, parsing and runtime failures settle the pending comparison and
  allow retry. Gzip streams are read with a size bound instead of buffering unlimited
  expansion. Browser limits and the CLI path for larger logs are documented.
- Baseline tabs support arrow keys, Home and End and restore keyboard focus. Controls wrap
  on narrow screens; result filters and disclosures preserve focus and review state.

### Added

- Explicit JSON field watches in the CLI (`--watch-field`) and browser catch exact
  status/exit-code changes before masking. Reports include typed values, per-run
  counts, first source occurrences and coverage; incomplete watches exit 2.
  Tracking is bounded, duplicate path members are ambiguous, and large numbers
  remain exact in browser downloads. The library's `DiffOptions` gains
  `watch_fields`; `DiffResult` gains `watched_fields` and `complete()`.
- A reproducible loopback HTTP experiment checks actual responses and process
  exits against emitted log records. Its two good runs and injected-fault run
  demonstrate a regression that the default template comparison cannot detect.

- A real-compiler verification matrix covers identical builds from separate checkout
  paths, target-directory selection, Cargo flag precedence, compiler wrappers and
  preservation of the last valid staged engine after build failures.
- Portable reports include `engine.wasm_sha256`, calculated from the exact module bytes
  executed by the worker. The native engine and raw diff JSON remain at 0.3.4.
- Chromium/Firefox cache tests exercise two real production builds while the legacy URL
  remains cached; report checksums match the actual served module bytes. Browser workflows
  can also run against a deployed origin with `LOGDELTA_BASE_URL`.
- Cancel and New comparison controls; intentional empty logs; filenames and source order
  alongside the applied masks and context; a versioned report download that retains this
  information and all findings. The original raw JSON download remains available.
- Finding pagination (50 per page), with complete exports and bounded block expansion.
- Production Chromium/Firefox workflow tests in CI, covering races, recovery, downloads,
  responsive light/dark accessibility, local processing and Content-Security-Policy.

## [0.3.4] - 2026-10-03

### Fixed

- A file that is not text was read as a log. A PNG or an archive given by mistake (a wrong
  artifact path in CI) became hundreds of junk templates, and the diff exited 0 or 1 like any
  other run. A NUL byte in the first block now stops it with `<file>: not a text file`, in the
  CLI and in the web demo (where the editor keeps what it had).
- Read errors exit 2. A missing or unreadable log exited 1, the code that means "found
  something", so a CI gate could not tell one from a regression. 0, 1 and 2 now mean what they
  mean for `diff` and `grep`; the README and the GitHub Actions recipe say so.
- The GitHub Actions recipe downloaded a release binary that no release had ever carried, so
  its step failed with a 404. Releases now carry a static Linux x86-64 binary,
  `logdelta-x86_64-unknown-linux-musl.tar.gz`, with its SHA-256 (0.3.3's was added
  afterwards), and the recipe fetches that.

### Changed

- Gzip is recognized by its first two bytes, not by a `.gz` name, in the CLI as it already was
  in the web demo: a compressed log saved without the extension, or piped on stdin, is read.

## [0.3.3] - 2026-10-03

### Fixed

- A long path left the human report's locations without their file name and
  line numbers. Each location line is fitted to the terminal (120 columns in a
  pipe) by cutting its end, which is where `pytest-fail.log:14-23` is; with a
  CI workspace or a temporary directory in front, every location read
  `/home/runner/work/.../artifacts/lo…`. The path is now cut at its start
  (`…/artifacts/logs/pytest-fail.log:14-23`). The snapshot tests run on
  relative paths, so they no longer fail when the repository is checked out
  somewhere deep.

The rest of this release is in the web demo; the package is otherwise
unchanged.

### Changed

- The page's fonts are served by the page itself. They came from Google Fonts,
  the one request the page made to another origin; the same font files (every
  subset, as Google serves them to a current browser) are now in
  `web/src/fonts/`, with their SIL Open Font License texts. Nothing looks
  different: screenshots before and after match. The page now loads with
  every other host blocked.

### Security

- The built page carries a Content-Security-Policy. Scripts, styles, fonts and
  workers load from the page's own origin only, and `connect-src 'self'` has
  the browser refuse to send what you give the page to any other host, even
  for a script injected through a bug in how the page renders a file. Inline
  event handlers and `eval` are not allowed (`'wasm-unsafe-eval'` is: the
  engine is WebAssembly). Every control was exercised in Chromium and Firefox
  with a listener for policy violations: none.

### Accessibility

- Checked with axe-core (WCAG 2.1 A and AA, and its best-practice rules) in light and dark,
  at desktop and phone widths, on each example: no findings now. The
  faint text was 3.5:1 to 3.8:1. The baseline tab list held two buttons that
  are not tabs (add, remove): they sit beside it now. Log excerpts that scroll
  sideways can take keyboard focus.
- The log editors showed keyboard focus only as their 1-pixel border turning
  from dark to light grey (axe-core does not check focus indicators; tabbing
  through every site and comparing each stop focused and unfocused does). They
  get the 2-pixel focus ring every other control has.

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
