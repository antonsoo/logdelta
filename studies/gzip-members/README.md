# Concatenated gzip in the actual browser

The CLI already opens concatenated gzip streams. The browser previously passed
compressed files through `DecompressionStream("gzip")`, whose
[specified format](https://compression.spec.whatwg.org/#dom-compressionformat-gzip)
accepts one member and rejects bytes following that member. The same file could
therefore work in the CLI and fail to open in Chromium and Firefox.

[GNU gzip supports concatenated members](https://www.gnu.org/software/gzip/manual/gzip.html).
`capture.py` uses the installed GNU gzip executable to compress the existing
[HTTP rate captures](../../examples/http-rates/). These are 6,000 recorded,
controlled loopback HTTP responses, with an independent client response ledger.
They are not a production incident or new network measurements. Each original
log has 2,002 lines: startup, 2,000 responses, and exit. Its six-member version
puts only startup in the first member and splits the remainder into chunks of
at most 500 lines. Reading just the first member would lose all HTTP evidence.
Single-member and six-member forms decompress to the exact same original bytes.

| Input | Earlier browser | Corrected browser | CLI |
| --- | --- | --- | --- |
| Single-member HTTP log | Opens | Opens | Opens |
| Six-member failing HTTP log | Rejects; keeps previous input/report | Opens all 2,002 lines | Opens all 2,002 lines |
| Corrupt later checksum, corrupt size, truncated trailer, trailing junk | Not replayed individually | Rejects the complete import; keeps previous input/report | Exits 2; no partial report |
| Expanded bytes or decoded UTF-8 over 25 MiB | Existing size policy | Rejects; keeps previous input/report | Browser cap does not apply |

With both good runs and the failing run loaded, the full downloaded result
matches the native CLI report. The independent client ledger supplies the
counts: checkout HTTP 503 observations are 10/1,000 and 12/1,000 in the good
runs, then 200/1,000 in the failing run. Maintenance stays at 1,000/1,000 in all
three runs. This is a rate change among observed requests, not a claim that
every 503 is new or that the logs prove a cause.

## Change and boundaries

The browser now uses `logdelta::io::gunzip_bounded` through the compiled WASM
engine. This uses the same `flate2::read::MultiGzDecoder` as CLI ingestion. It
reads one byte beyond the allowed output size to detect overflow, validates
trailers even at the exact limit, and returns no partial input on any error.
The new `ld_gunzip` C ABI accepts compressed bytes and returns expanded bytes;
it does not serialize large byte arrays or base64 into JSON.

Each file gets its own worker. Completion or cancellation terminates that
worker and releases its file data and WASM memory. Successfully initialized
executable bytes can be reused from page memory for another import or
comparison. Concurrent baseline/target imports do not supersede each other.
Queued results from a cancelled or edited source cannot replace newer text.

Text decoding happens once, after the full byte stream is validated. Controls
split a UTF-8 character, UTF-16 BOM and UTF-16 code unit between gzip members.
They retain the original text. Invalid UTF-8 remains marked as lossy and cannot
pass an exact field watch; an actual encoded U+FFFD remains valid text. The
compressed-byte, expanded-byte, decoded UTF-8, and total-comparison limits
remain in force. No new compression dependency is added.

Offline checks start after assets and engine code have loaded. They do not
claim that a page reload or an example download works without the site.
No registry release or tag is part of this change; the CLI already supported
multiple gzip members before this browser correction.

## Reproduce

From the repository root, with GNU gzip, Rust's WASM target, Node 24, and the
cached Playwright browsers available:

```sh
python3 studies/gzip-members/capture.py
cargo build --locked --release
cd web
npm ci
npm run build
cd ..
node studies/gzip-members/verify.mjs web/src/generated/logdelta.wasm target/release/logdelta /tmp/gzip-wasm.json
```

Run `npm run preview -- --host 127.0.0.1 --port 4362 --strictPort` from `web/`,
then run the retained replay from the repository root:

```sh
node studies/gzip-members/browser.mjs http://127.0.0.1:4362/logdelta/ /tmp/gzip-browser after
```

The same driver accepts `before` against an archived build of
`cbf72c0486056098594bf6cc146b907a7fec4e16` to reproduce rejection and prove that
the original input and complete previous downloaded report were preserved.
`web/tests/browser/gzip.spec.mjs` runs the shared import gates, independent-worker
races, and recovery after a failed first engine fetch in the production suite.
The race scheduler holds actual worker replies; it never replaces decoded bytes.

`captures/manifest.json` records compressed/expanded hashes and GNU gunzip exits.
`results/native.json` retains the complete original CLI result.
`results/wasm.json` compares every accepted binary result byte-for-byte with GNU
gunzip and checks both gzip forms against the native result and client ledger.
Browser receipts record actual saved downloads, their hashes, rejected-import
messages, worker ownership checks, and offline requests/errors. Verification
and publication receipts are added after the clean build and public replay.
