# The native formatter correction in the public browser

The source correction was already published, but the live page still used an
older engine. On 17 October 2026 a fresh public replay of both native formatter
captures produced **zero findings**, with both watches marked complete. This
follow-up checks that actual failure, rebuilds the existing correction, and
separates the static deployment from the unchanged crates.io release.

## The public failure

The inputs come from real calls to `python-json-logger` 4.2.0 with constructed
outcomes and fixed timestamps. Renaming `message` to `log` and `asctime` to
`time` does not turn an application event into a Docker wrapper. A fresh
Python 3.12.3 invocation reproduces all four retained files byte for byte;
the [formatter receipt](results/hosted/native-formatter.json) records their
sizes and SHA-256 values. These are controlled records, not a production incident
or a recording from a Docker daemon.

| Input | Actual public before | Corrected native and local browser result |
| --- | --- | --- |
| Renamed formatter, grouped by `/route` | Only 1 of 2 target records matched; the renamed record was treated as non-JSON; no 503 finding | Both records match; target line 2 contributes the outer 503 |
| Renamed formatter with JSON text inside `log`, pooled | Both target records became 200, including the inner message value; no 503 finding | Target counts are 200: 1, 503: 1; the outer application field is retained |

The [public before replay](results/hosted/before/receipt.json) downloads both
reports in Chromium and Firefox at 1440 and 375 px after setting the browser
context offline. The old page attempts two HTTP asset requests after reset;
those requests are retained in the receipt, not described as absent. Its engine
is `3f08f442b55baf3d2d88a7386824a13a4b2de9cdf0f8da9be62c0ddbe6e799cd`.
The [before artifact observation](results/hosted/before-artifact.json) also
matches that canonical module to the pinned raw bytes of Pages commit
`f8c1e783c0d9b7e024f8d6c9f4ca554e389d4bdb`. It does not infer an older main-source
revision from that deployment.

| Public before | Desktop | 375 px |
| --- | --- | --- |
| Chromium | [Zero findings](../../docs/assets/envelope-identity-hosted/before-chromium-1440.png) | [Zero findings](../../docs/assets/envelope-identity-hosted/before-chromium-375.png) |
| Firefox | [Zero findings](../../docs/assets/envelope-identity-hosted/before-firefox-1440.png) | [Zero findings](../../docs/assets/envelope-identity-hosted/before-firefox-375.png) |

All four screenshots were opened and inspected. The eight actual before
downloads are alongside their receipt.

## Clean build and actual reports

A [fresh archive](results/hosted/clean-archive.json) of
`f4530200b26853887641a4b4cab6ab6cd27a9263` builds the existing correction with
Rust 1.91.0 and Node 24.21.0. Formatting, Clippy, 230 Rust checks including the
doctest, web lint/types, 83 web checks, and native/WASM production builds pass.
The initial browser invocation prints 132 passes but exits 143 after the
summary; its exit is retained in the receipt. Separate invocations exit 0:
130 workflows against an existing local preview, plus the two local two-build/
cache fixtures under an isolated config. The former intentionally skips those
two fixtures when `LOGDELTA_BASE_URL` is set. No skipped case is counted as a
pass. The [minimal isolated config](results/hosted/cache-only.config.mjs) can be
copied into `web/` to repeat those two original fixtures.
The fresh WASM is 1,276,572 bytes with SHA-256
`be02eca56a0aac5faa211fbdeb01ff62f6b8fe8632b130a572e5ea839057b5fc`, matching
the earlier source study's reproducible engine. No staged binary from the
working checkout was used.

The [fresh CLI replay](results/hosted/native-replay.json) checks all 19 cases.
The [actual compiled WASM](results/hosted/wasm.json) produces the same full
reports: counts, strings and evidence match exactly; floating scores are compared
at 12 significant digits. Docker controls retain their inner payload, duplicate
members remain incomplete, the unchanged application stays unchanged, and the
rate example retains 2/100 baseline errors against 27/100 in the target.

The [clean local browser replay](results/hosted/local/receipt.json) repeats
all 19 cases in each engine/viewport combination: **76 actual comparisons and
JSON downloads**. Every full downloaded result matches its fresh CLI report.
All contexts disconnect before the first native comparison, and the corrected
page makes no HTTP request after disconnection. Incomplete duplicate-member
cases wait for the explicit incomplete status and retain their refusal; they
are not treated as completed clean comparisons. Tables and source excerpts keep
component scroll regions at 375 px, with no horizontal page overflow or page
errors. All four local report screenshots were also inspected before publication.

## Published result

Pages commit `a80e0bea04cbf95b7f001b851e05fa7d950de5f3` serves the captured
clean build. The [canonical after receipt](results/hosted/canonical-after.json)
checks **all 39 static files**, the real root before/after and the explicit index,
without cache-busting parameters. The module observed in all four public browser
sessions has the exact clean-build hash given above. The
[deployment record](results/hosted/deployment.json) identifies the source/build
and branch observation separately; remote CI execution is not inferred.

The [public after replay](results/hosted/after/receipt.json) completes all
**76 comparisons and downloaded full-report checks** against the fresh CLI.
Both previously hidden native statuses are retained. There are no page errors,
horizontal page overflow or HTTP request attempts after disconnection in any of
the four sessions. The eight native failure-example downloads are retained
alongside the receipt; the remaining controlled cases retain their full-result
comparison outcome and actual download hash/size. All four public after
screenshots were opened, as were the four public before views and four local
views. [Retained image inventory](results/hosted/images.json).

| Public after | Desktop | 375 px |
| --- | --- | --- |
| Chromium | [Outer 503 retained](../../docs/assets/envelope-identity-hosted/after-chromium-1440.png) | [Outer 503 retained](../../docs/assets/envelope-identity-hosted/after-chromium-375.png) |
| Firefox | [Outer 503 retained](../../docs/assets/envelope-identity-hosted/after-firefox-1440.png) | [Outer 503 retained](../../docs/assets/envelope-identity-hosted/after-firefox-375.png) |

This publishes the existing runtime correction. Subsequent README, replay and
evidence changes do not change the artifact. The [fresh registry observation](results/hosted/registry.json) remains at
0.3.4; this manual static deployment is not a crate release or tag.

## Repeat the focused check

From an installed source checkout, create a new temporary parent directory and
build the native reports and browser artifact:

```sh
mkdir -p /tmp/logdelta-native
cargo build --locked --release
python3 studies/envelope-identity/replay.py target/release/logdelta /tmp/logdelta-native/native-replay after > /tmp/logdelta-native/native-replay.json
cd web && npm ci && npm run build && cd ..
node studies/envelope-identity/replay-wasm.mjs web/src/generated/logdelta.wasm /tmp/logdelta-native/native-replay
node studies/envelope-identity/replay-hosted.mjs https://antonsoo.github.io/logdelta/ /tmp/logdelta-native/public after /tmp/logdelta-native/native-replay
```

The browser helper resolves Playwright from `web/package.json`. It reads the
fresh replay manifest beside the supplied replay directory. The `before` phase
checks only the two retained failures and requires the older artifact; it is not
expected to pass once the corrected page is published.

The original [content-recognition boundaries](README.md#boundaries) still
apply. A successful deployment does not authenticate a log's origin, certify
production incident detection or publish a new crate. The unchanged release
number alone cannot identify this engine; use the captured module hash and
canonical-byte receipt.
