# Browsing large field ledgers

Checked against the local production WASM build on 2026-10-09 UTC. This extends
the [grouped-field verification](verification-grouped-fields.md); comparison semantics,
limits, native output and WASM bytes are unchanged.

## Reproduction and measured change

A controlled input has 128 distinct routes, each with a 3,900-character suffix, and
16 watched status fields. Each baseline status is 200; each target status is 503.
The two files total 1,065,216 bytes. They exercise the documented bounds, and are
synthetic; this is not a production log or a general throughput benchmark.

Before this change (`0595411`), every table and first-source excerpt was rendered
even when its field was closed. The native report is 52,119,152 bytes because it
contains repeated group keys, source excerpts and context. It remains that size:
this change bounds the visible browser view without removing exported evidence.

| Chromium measurement, all fields closed | Before | After |
| --- | ---: | ---: |
| Page elements | 79,859 | 1,555 |
| Report HTML bytes | 51,624,998 | 874,514 |
| Longest observed main-thread task, ms | 783 | 139 |
| Compare click to completed status, ms | 1,435 | 821 |
| Approximate reported JS heap, bytes | 286,000,000 | 116,000,000 |

Single local measurements, WSL2 Linux, 14 logical CPUs and 48 GB RAM, warm engine
assets, with other verification running on the machine. Timing and Chromium's
rounded heap estimates vary; DOM counts and report HTML bytes are structural
measurements (the HTML also includes a short elapsed-time label). Firefox produced the same DOM counts and equivalent downloads;
it does not expose the long-task or heap APIs used here. The result still needs
serialization, transport from the worker, and a bounded page of finding cards.
There is still a main-thread task over 50 ms in this stress case.

The committed [measurement and hashes](verification-field-ledger.json) include
both browsers, input hashes, engine hash and screenshot hashes. The original
measurement is retained in [the baseline record](verification-field-ledger-before.json).

## Releasing a completed comparison

Final review found that clearing the controller's view settings also needed to
clear its references to the completed field data. Without that, **New comparison**
removed the page elements but kept the old evidence alive until another comparison.
The same closed-ledger fixture retained 52,554,048 bytes of live main-thread JS
after reset and explicit garbage collection. Clearing those references reduced
that measurement to 1,957,852 bytes. The [paired reset measurement](verification-field-ledger-reset.json)
records both runs; this is live JS heap, not whole-process memory or a promise of
immediate operating-system memory reclamation.

The reproducible verification now checks this lifecycle through Chromium's heap
API, resets the real WASM workspace in both browsers, and compares again. After
the reset fix, all 34 field browser workflows passed. The earlier full browser
run passed 98 workflows.

## User workflow and evidence

- All 256 entries of a field are reachable across eight pages and agree, in order,
  with the complete exported ledger. The field summary retains its full finding count.
- Search for `/127/` selects that route's two outcomes and their line-128 sources.
  Counts remain exact, and input coverage remains unfiltered. A query with no matches
  says so; it does not imply a clean comparison.
- Opening sources loads excerpts for the visible page. Closing the outer field removes
  the table and excerpts from the DOM. Reopening restores search, page and source state.
- Finding pagination and edits to pending inputs preserve the completed ledger view.
  A new comparison resets it. Both JSON downloads retain the complete result.
- The stress-case report downloaded after filtering is deeply equal to the actual
  native `--json -C 2` output in Chromium 153.0.8010.12 and Firefox 155.0.
- Desktop, 375px and 320px checks cover keyboard search and table navigation, focus
  after paging, horizontal table scrolling, light/dark themes and accessibility
  checks. No page errors or external requests were observed.

Screenshots were opened and inspected: [Chromium](assets/field-ledger/chromium-search.png),
[Firefox](assets/field-ledger/firefox-search.png), and
[375px dark mode](assets/field-ledger/mobile-dark.png). Source rows and narrow tables
scroll horizontally; the whole page stays within the viewport.

## Repeat

```sh
cargo build --release --locked
cd web
npm ci
npm run build
npm run preview -- --host 127.0.0.1 --port 4197 --strictPort
# In another terminal, from web/:
node scripts/verify-ledger.mjs http://127.0.0.1:4197/logdelta/
npx playwright test
```

The verification script creates and removes its own temporary files, compares
actual downloads, and regenerates measurements and screenshots. Native/package
checks for the grouped engine are recorded in the earlier verification; this
follow-up changes browser presentation only.
