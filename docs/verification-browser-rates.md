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

## Interpretation limits

Rates describe logged scalar observations, not all requests unless the supplied
logs establish that coverage. Missing field records remain separately counted.
The existing score is a triage heuristic, not a calibrated p-value; its sparse
counterexample and baseline-variation limits remain in the
[engine verification](verification-field-rates.md). This UI does not change that
score, choose an automatic error definition, identify a change point, or infer a
root cause. Full input logs are not bundled with the downloads.

A separate Chromium editor limitation surfaced during this check: native text
insertion of 6,400 JSON lines took 37.5 seconds in the page, 28.1 seconds in a
standalone plain textarea, and 30.5 seconds in a styled standalone textarea on
this machine. These are single diagnostic observations, not a benchmark. The
large-report workflow uses real file imports, which avoid that insertion path.
Use **Open file** for large logs. Native paste performance remains unresolved;
this work does not alter the browser's editing or undo behavior.
