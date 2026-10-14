# Repeated JSON member names must not erase earlier diagnostics

At source `9e34dd6`, a target record contains both
`"message":"database corruption detected"` and a later
`"message":"health check passed"`. The actual CLI reports **zero findings and
exit 0**, matching a baseline containing only the latter value. A constructed
Docker-style envelope with repeated `log` members has the same failure.

The parser's ordinary JSON map kept the last member. Normalization then erased
the earlier text before template mining, including nested objects and array
elements. This is distinct from the existing exact-field lookup, which already
treated a repeated watched name as ambiguous and exited 2.

The correction parses JSON with locally unique member names before interpreting
it. Repeated decoded names at any object depth stop JSON unwrapping/flattening;
the record stays in the existing masked raw-text path, keeping each occurrence.
Normal timestamp/id/custom masking still applies. It does not select a winning
member or reject a whole mixed-format log. Separate objects can reuse a name;
literal JSON text inside a string is not parsed as an object. Exact field
watches retain their scoped lookup rules.

[RFC 8259, section 4](https://www.rfc-editor.org/rfc/rfc8259#section-4) recommends
unique object names and describes inconsistent behavior when names repeat.
Logdelta's raw fallback is a conservative application policy, not a claim that
the RFC forbids all such JSON. The Serde visitor follows its
[documented deserialization contract](https://serde.rs/impl-deserialize.html).

## Actual consumer and CLI replay

```sh
cargo build --locked --release
python3 studies/json-members/replay.py --cli target/release/logdelta \
  --crate-root . --out /tmp/json-members.json --label source
```

[`consumer.rs`](consumer.rs) is built as a separate crate with the tested
`logdelta` path dependency and `default-features=false`. It invokes the public
tokenizer and `diff_lines` API. [`replay.py`](replay.py) then runs the actual CLI
against files, gzip inputs, stdin on either side and novelty streaming. No
production service, Docker daemon or remote logging system is involved.

[`cases.json`](cases.json) contains 18 constructed cases: repeated message/log/
stream members; nested and array members; JSON-escaped equivalent names;
Unicode names; inner Docker payloads; CRI/color prefixes; late nested members;
identical repeated values; and six controls covering unique records, literal
JSON strings, separate objects, distinct Unicode names and primitive values.

The [baseline](results/before.json) disagrees in **72 of 109 workflows**;
[corrected source](results/after.json) disagrees in none. All 36 ordinary
public-API/CLI controls are identical. The extra exact-field watch keeps its
existing incomplete gate; its normal template findings now retain the earlier
diagnostic too. These are synthetic ambiguity counterexamples, not an ecosystem
accuracy benchmark. Source module, binary and input digests accompany captures.

For the baseline, build `git archive 9e34dd6`, pass its crate root and binary,
and add `--observe-only`. Consumer files and raw input/report artifacts stay in
a fresh temporary directory. The replay uses only Python's standard library;
building its consumer uses the existing cached Rust dependencies offline.

This change does not make template mining a complete semantic parser or secret
scanner. Variable masking and Drain's heuristic grouping still apply; use exact
field watches for scalar-value questions. Existing nested-value display bounds
also remain for unambiguous JSON. No crates.io release or hosted deployment is
part of this source correction.

## Installed artifacts, real WASM and public-input controls

Fresh `git archive` checkouts of baseline `9e34dd6` and implementation
`00aaff6` match all 316 and 322 archived files respectively. The
[verification receipt](results/verification.json) records input, production
artifact, package, check-log and screenshot hashes.

| Actual workflow | Result |
|---|---|
| Fresh Rust 1.91 source | Formatting, strict Clippy, 226 tests, 147 library-only tests, one doctest and release/package builds pass |
| Exact minimum Rust 1.85 | Locked build and 226 tests plus one doctest pass |
| Locally packed 0.3.4 crate | All 22 runtime source files and lockfile match the fresh checkout; three focused packed tests pass |
| Installed packed CLI on Rust 1.91 and 1.85 | All 109 workflows pass in each installation; complete captures match corrected source |
| Fresh Node 24.21.0 web checkout | Frozen install, Rust/WASM formatting and Clippy, three wrapper tests, lint, strict types, 83 web tests, build, 128 existing browser tests and 20 compiler-configuration checks pass |
| Node 24 and 26 production builds | All 39 files match byte for byte |
| Real compiled WASM engine | Eighteen complete reports match the native public API |
| Actual browser file imports and downloaded reports | Chromium and Firefox at 1440 and 375 px: 144 workflows; 48 baseline disagreements, zero after correction |
| Offline build identity | All 23 served and browser-loaded files match each tested build in all eight views; the server shuts down before comparisons |
| Visual and accessibility review | Eight screenshots inspected; no page overflow, script/CSP errors, external/late requests or axe findings in the replay |
| Existing cached public inputs | All 280 complete CLI outputs, stderr and exit statuses are identical before/after; all 536 input files (429,474,908 bytes) have verified hashes |

The [current](results/installed-current.json) and
[minimum-Rust](results/installed-floor.json) installation captures come from
the locally packed archive, not crates.io. Its unchanged `0.3.4` version is not
a released correction. The [WASM capture](results/wasm.json) and
[browser capture](results/browser.json) compare integer, text and source evidence
exactly; floating-point scores compare at 12 significant digits across engines.

```sh
# Run in web/ after npm ci and npm run build:
node ../studies/json-members/replay-wasm.mjs \
  src/generated/logdelta.wasm ../studies/json-members/results/after.json /tmp/wasm.json
# Run at the repository root, using separately built archived checkouts:
node studies/json-members/browser.mjs \
  /tmp/logdelta-before/web/dist /tmp/logdelta-after/web/dist /tmp/browser
python3 studies/json-members/replay-corpus.py \
  /tmp/logdelta-before/target/release/logdelta \
  /tmp/logdelta-after/target/release/logdelta /tmp/corpus.json
```

The browser driver uses the retained WASM capture as its expected report. Its
native file uploads assert the complete textarea contents before comparing.
An initial use of Playwright `fill` discarded literal ANSI ESC bytes; both
phases were rerun with actual file imports. The consumer build also fixes its
own target directory so an inherited `CARGO_TARGET_DIR` cannot redirect the
executable. These were evidence-driver corrections, not application failures.

The [public-input capture](results/corpus.json) replays the existing Kubernetes
and Loghub cache, after checking every file against its recorded size/hash.
Kubernetes comparisons use every passing baseline provided by the manifest;
Loghub comparisons use complete template reports. Cache acquisition instructions
remain in the [Kubernetes study](../kubernetes-ci/README.md) and
[Loghub study](../loghub/README.md). This is an output-stability control, not a
new dataset collection or an accuracy/performance claim.

## Captured production app

These are unedited local production-build screenshots from actual file imports.
The baseline shows zero findings despite the earlier corruption message. The
correction shows one New and one Gone template, retaining both repeated values
in the new raw-text template. Long source records scroll within their controls
on mobile. Application styling is unchanged.

| View | Baseline | Corrected |
|---|---|---|
| Chromium, 1440 px | [Before](../../docs/assets/json-members/before-chromium-1440.png) | [After](../../docs/assets/json-members/after-chromium-1440.png) |
| Chromium, 375 px | [Before](../../docs/assets/json-members/before-chromium-375.png) | [After](../../docs/assets/json-members/after-chromium-375.png) |
| Firefox, 1440 px | [Before](../../docs/assets/json-members/before-firefox-1440.png) | [After](../../docs/assets/json-members/after-firefox-1440.png) |
| Firefox, 375 px | [Before](../../docs/assets/json-members/before-firefox-375.png) | [After](../../docs/assets/json-members/after-firefox-375.png) |
