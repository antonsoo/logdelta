# Browser evidence for known-value rate changes

The browser now compares rates using the same Rust engine as the CLI and exposes
the evidence needed to assess them. This closes a reproduced gap: the retained
HTTP capture had 503 in every run, so a browser novelty watch reported **zero
findings** even as checkout's error fraction increased from 1–1.2% to 20%.

| Applied browser settings | Captured checkout result |
| --- | --- |
| Watch `/http/status`, group by `/route` | 0 findings; both values already known |
| Same watch, rate comparison enabled at 5 pp | 1 rate finding, containing the 503 increase and complementary 200 decrease |
| Same watch, rate comparison enabled at 19 pp | 0 findings; observed change is below the chosen effect threshold |
| Same watch, rate comparison enabled at 18.8 pp | 1 rate finding; threshold is inclusive |

The maintenance route has 1,000/1,000 503 responses in all three runs and no rate
finding. These are actual requests under deliberately injected local faults,
not a production incident. [Capture and independent client observations](../examples/http-rates/README.md).

## Inspectable results

- [Before: actual browser download without rates](field-rate-evidence/browser/before-report.json).
- [After: actual schema 3 browser download](field-rate-evidence/browser/report.json), with the applied threshold, engine digest, source names and complete result.
- [Eight browser/theme/width captures and offline checks](field-rate-evidence/browser/verification.json).
- [Repeated independent 6,000-response and 12-score verification](field-rate-evidence/browser/oracle-verification.json).
- [Repeated native/compiled-WASM comparison](field-rate-evidence/browser/wasm-verification.json).

The browser result matches the retained native report in full, after rounding
non-integral numbers to 12 significant digits to permit last-bit differences
between native and WASM math libraries. The source HTML, core library and UI do
not contain a second scoring implementation. The response boundary rejects a
worker that omits the requested rate comparison, uses different thresholds,
omits groups, changes denominators, or references an incompatible value.

Missing baseline or target groups make the rate comparison incomplete without
changing the completeness of the exact novelty ledger. An unobserved group is
not 0%. A known value absent from an otherwise observed target group can be
measured at 0%, and its source fold points to the baseline. Truncated or malformed
field evidence never supplies partial rate denominators.

Reports remain snapshots through checkbox/threshold edits, searches, filters and
pagination. Editing cancels an in-flight comparison; a held old worker reply
cannot replace the previous result. Reset clears the report, disclosures and
engine. New examples replace the rate settings along with their log inputs.
The default remains novelty-only.

## Reproduce the browser checks

```sh
cd web
npm ci
npm run lint
npm run typecheck
npm test
npm run test:browser
npm run preview -- --host 127.0.0.1 --port 4341 --strictPort
# In a second shell, from web/:
node scripts/capture-rates.mjs
```

The capture driver runs the real production page in Chromium and Firefox at
375px and 1440px, in light and dark themes. It opens the exact source record,
checks page overflow, disconnects the network, compares again, downloads the
report and checks it against the native evidence. It hashes the actual engine
response and requires that hash to match the report. It rejects runtime errors
and external requests.

All eight screenshots were opened and inspected. The phone summary shows the
baseline range and target share before the horizontal table. Tables and source
records have keyboard-scrollable regions. The browser scenarios also exercise
real Tab/Enter/ArrowRight navigation and axe checks in each browser/theme/width.

Finding lists render 50 groups per page; rate tables show up to 32 changes per
group. Larger sets remain available in the searchable, 32-row field ledger and
both downloads. Source excerpts enter the DOM only when their fold is opened,
and closing it releases those nodes. A 64-group constructed control checks
pagination, escaped keys, search and complete downloads; it is not incident data.

## Clean build checks

A fresh `git archive` of `c9d4e90` was extracted into an empty directory. `npm ci`
installed from the lockfile; Rust targets and the WASM module were compiled in
that fresh checkout. Subsequent changes add verification records, preserve
captured bytes and correct the automation-only paste diagnosis; application code
is unchanged.

| Check | Result |
| --- | --- |
| Fresh `npm ci` | 134 packages installed; audit reported zero vulnerabilities |
| `npm run lint`, `npm run typecheck` | Passed without warnings or errors |
| `npm test` | 78 passed in 7 files |
| `npm run build` | Complete Rust/WASM, TypeScript and Vite build |
| `npx playwright test` | 122 passed in Chromium and Firefox, 1.5 minutes |
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Clean |
| `cargo test --locked` | 206 tests and 1 doctest passed |
| `cargo build --locked --release` | Fresh native release build completed |
| Fresh-build HTTP oracle and native/WASM replay | 6,000 responses, 12 scores and all 5 request workflows matched |
| WASM SHA-256 across working and fresh checkout builds | Identical: `3f08f442b55baf3d2d88a7386824a13a4b2de9cdf0f8da9be62c0ddbe6e799cd` |

The site is built with `npm --prefix web run build`, output **`web/dist/`**, base
**`/logdelta/`**. Main JS is 55.13 kB / 18.14 kB gzip, CSS 30.06 kB / 7.10 kB gzip,
and the unchanged WASM engine 1,266.41 kB / 458.01 kB gzip. The eight recorded
browser comparisons took 78–114 ms inside the engine after loading; these are
individual observations on a 14-vCPU, 48-GB WSL2 machine, not a performance
guarantee. No runtime dependency was added.

## Published page

Source `9b58fa9` was pushed to `main`; the verified build is on `gh-pages` at
`f8c1e78`. No package registry release was made. All
[40 served files](field-rate-evidence/browser/hosted-assets.json) match the clean
build byte for byte, including the HTML, worker, WASM and all captured examples.

[Hosted browser verification](field-rate-evidence/browser/hosted-verification.json)
repeated the eight Chromium/Firefox, desktop/phone, light/dark workflows against
https://antonsoo.github.io/logdelta/. Each loaded the capture, displayed the rate
finding, inspected the source, then compared and exported offline. Every result
matched the native report. No runtime errors, external requests or page overflow
were observed. The application remains a static page; nothing was uploaded.

## Interpretation limits

Rates describe logged scalar observations, not all requests unless the supplied
logs establish that coverage. Missing field records remain separately counted.
The existing score is a triage heuristic, not a calibrated p-value; its sparse
counterexample and baseline-variation limits remain in the
[engine verification](verification-field-rates.md). This UI does not change that
score, choose an automatic error definition, identify a change point, or infer a
root cause. Full input logs are not bundled with the downloads.

A large-input check initially stalled in Playwright's `fill()`/`Input.insertText`
path in Chromium. The same operation took 28.1 seconds in a plain standalone
textarea and 37.5 seconds in this page for 6,400 lines. That did **not** establish
a product paste defect: actual clipboard paste of the same size completed in
197 ms; the retained reproduction with the exact larger escaped-key fixture
completed paste in 156 ms, undo in 20 ms and redo in 146 ms. A separate [clipboard/undo verification](field-rate-evidence/browser/paste.json)
uses real Ctrl+V, Ctrl+Z and Ctrl+Shift+Z with exact content checks. The large
report scenarios use real file inputs to avoid the automation insertion path.
No editor behavior or timeout was changed to hide this discrepancy.
