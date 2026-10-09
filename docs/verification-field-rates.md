# Field-rate verification, 9 October 2026

Local work only. The package version remains 0.3.4; these changes are unreleased.
No repository, package or site was published. Implementation: `ee45551`; retained
captures and replay drivers: `daa34a7`. The starting checkout was `17fb1d3`.

## The reproduced gap

Two passing checkout runs already contained HTTP 503s (10/1,000 and 12/1,000).
The target contained 200/1,000. Template comparison and grouped exact-value
watching both remained quiet because the log shapes and set of outcomes were
unchanged. The baseline executable reported zero findings and exited 0.

The opt-in `--watch-rate-change 5` now finds this group: checkout's 503 fraction
is 18.8 percentage points above the highest observed baseline fraction. Its
complementary 200 decrease appears in the same rate finding. Maintenance's
1,000/1,000 503 responses in every run remain unchanged. Omitting the option
preserves the original novelty-only behavior.

| Observation | Baseline executable | Current executable |
| --- | --- | --- |
| Grouped watch without rate option | 0 findings; exit 0 | 0 findings; exit 0 |
| Grouped watch with 5 pp minimum | Option unavailable | 1 rate-group finding; exit 1 |
| Existing values | 200 and 503 both known | Still known; rate evidence attached separately |
| A new target-only route with rates requested | No rate feature | Unknown baseline rate; exit 2 |
| Malformed selected evidence with rates requested | No rate feature | No partial denominators; exit 2 |

See [usage, JSON contract and interpretation](field-rates.md).

## Retained evidence

The [HTTP capture](../examples/http-rates/README.md) contains actual loopback
requests and responses, not manufactured log lines. The faults were deliberately
injected into the fixture server. This is not a production incident or a
benchmark of general detection accuracy. The independent client records every
status, and verification matches all **6,000 responses** against server records
and the exact field ledger.

- [Baseline output](field-rate-evidence/before.json), [current default output](field-rate-evidence/novelty-only.json),
  [current rate output](field-rate-evidence/native.json), [generated Markdown report](field-rate-evidence/report.md).
- [Independent numerical results, cases, source/binary hashes and timings](field-rate-evidence/verification.json).
- [Fresh installed Rust 1.85 results](field-rate-evidence/installed-rust-1.85.json).
- [Actual WASM/native comparison and engine hash](field-rate-evidence/wasm-verification.json).
- [Build provenance and screenshot hash](field-rate-evidence/provenance.json).

The SciPy 1.18.0 oracle uses
[`chi2_contingency` with `lambda_="log-likelihood"`](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.contingency.chi2_contingency.html)
and `correction=False` on the four smoothed counts. NumPy 2.5.3 independently
calculates the baseline variability penalty. Twelve reported score components
agree to a relative/absolute tolerance of `1e-10`. The driver also verifies which
values qualify, including cases with no rate finding.

Nine additional generated scenarios exercise stable rates, rises, falls,
baseline variation, ordinary small-count variation, a sparse-score limitation,
the inclusive effect boundary, a shift below that boundary, and a known value
falling to zero. Separate CLI checks cover changing route mix, groups missing
on either side, one unobserved baseline group, malformed/truncated ledgers,
source context, Markdown escaping and invalid options.

The actual compiled WASM module and native executable produce equivalent full
JSON reports for five workflows: enabled rates, disabled rates, unchanged
target, missing group and malformed record. Comparison rounds non-integral
numbers to 12 significant digits to allow last-bit native/WASM math differences.
The same replay passed with the fresh Rust 1.85 installed binary. This verifies
the engine request boundary; this initial check did not exercise the browser controls. The later
[browser verification](verification-browser-rates.md) covers the full page workflow.

## Checks run

| Command or workflow | Result |
| --- | --- |
| `cargo fmt --check` | Clean |
| `cargo clippy --locked --all-targets -- -D warnings` | Clean |
| `cargo test --locked` | 206 tests and 1 doctest passed |
| `cargo build --locked --release` | Completed with Rust 1.91.0 |
| `cargo +1.85.0 check --locked --lib --no-default-features` | Passed at the declared MSRV |
| `cargo package --locked --allow-dirty` | Local archive built and verified; no upload |
| Fresh-target `cargo +1.85.0 install --locked --path target/package/logdelta-0.3.4 --root ...` | Installed; HTTP/oracle/WASM replays passed |
| `cargo fmt --manifest-path web/wasm/Cargo.toml -- --check` | Clean |
| `cargo clippy --locked --manifest-path web/wasm/Cargo.toml --all-targets -- -D warnings` | Clean |
| `cargo test --locked --manifest-path web/wasm/Cargo.toml` | 3 passed |
| `npm ci` in `web/` | Fresh install from lockfile completed |
| `npm run lint` and `npm run typecheck` in `web/` | Clean |
| `npm test` in `web/` | 67 passed |
| `npm run build` in `web/` | Rust WASM, TypeScript and production Vite build completed |
| `npx playwright test` in `web/` | Final run: 98 passed, Chromium and Firefox |
| Grouped keyboard checks repeated three times in each browser/theme | 12 passed |
| `python3 examples/capture_http.py --scenario rate-change` | Captured and checked 6,000 HTTP responses |
| Existing single-route and mixed-route capture scenarios into separate scratch folders | Both still capture and validate correctly |

The first browser run exposed a timing race in two Chromium keyboard checks:
Tab was sent before the asynchronous `toggle` handler had populated the lazy
ledger. The checks now wait for the search field to be visible, then use real
Tab navigation and assert focus. They do not force focus onto the expected
destination. No browser UI behavior was changed for this timing correction.

The [capture README](../examples/http-rates/README.md) contains copyable replay
commands and pinned verification dependencies. SciPy and NumPy are not runtime
dependencies of Logdelta. Large captures are excluded from the installed crate.

## Visible output and measured cost

The [terminal screenshot](assets/field-rates.png) renders
[unedited captured CLI output](field-rate-evidence/terminal.ansi). It was opened
and inspected: the two route groups, count ledger, minimum effect, changed
fractions and source locations are readable without overflow. Long source
records are explicitly clipped; JSON retains their recorded excerpts.

Four alternating runs of the complete 6,006-line comparison, including process
startup and JSON serialization, had these medians on this machine:

| Executable | Novelty-only watch | With rate comparison |
| --- | ---: | ---: |
| Rust 1.91 release build | 88.13 ms | 88.51 ms |
| Fresh Rust 1.85 package installation | 79.70 ms | 82.88 ms |

These are small local samples, not a throughput guarantee or a meaningful
comparison between Rust versions. The raw samples are retained. The extra
comparison works over the existing bounded ledger (64 pooled values or 256
group/value pairs per watched field); the template miner's memory remains
dependent on template diversity.

## Limits worth keeping visible

The score is a triage heuristic, not a calibrated p-value. The retained sparse
case 1/1,000 versus 1/5 has score 11.63, exceeding 10.83, while SciPy's independent
two-sided Fisher exact result is approximately 0.00993. The default cutoff must
not be advertised as a guaranteed 0.001 false-alarm probability. Correlated
logs, retries, missing individual fields, multiple comparisons and the observed
baseline range further limit statistical interpretation. The user-selected
effect threshold and actual denominators remain necessary evidence to inspect.

Rates describe **logged scalar observations**, not all requests that actually
occurred unless the supplied logs establish that coverage. No traffic-time
normalization or root-cause claim is made. Rate checks are now available through the browser as well as the source CLI,
Rust options and direct WASM JSON request; see the
[browser follow-up](verification-browser-rates.md).
