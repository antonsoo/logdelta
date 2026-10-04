# Logdelta browser-engine verification - 2026-10-04

This records the earlier cache-fix deployment. The later
[WASM build verification](verification-wasm-2026-10-04.md) addresses its build-path
limitation and records the replacement hosted artifact; the evidence below is historical.

A browser update could reuse an older engine cached under the fixed
`logdelta.wasm` URL. The worker now imports a fingerprinted module and records
SHA-256 of the exact buffer it instantiates in portable reports. The native
Rust algorithm, CLI and raw diff JSON remain at version 0.3.4.

## Source and clean build

- Application source: `8787ae6794cccca350b3d5b2e3a8f27b86a07ac8`.
- Fresh source archive, Node **24.21.0**, npm **11.19.0**, Rust **1.91.0**.
- `npm ci`: 134 packages installed, 135 audited, zero reported vulnerabilities.
- Lint, TypeScript checks, production build: passed.
- **55 unit tests / 5 files**: passed.
- Root Rust formatting, Clippy with all targets/features and warnings denied,
  **128 library + 33 integration tests**, and library-only build: passed.
- WASM wrapper formatting, wasm32 Clippy and its native ABI test: passed.
- Locked native build/test on the declared Rust **1.85.0** minimum: passed,
  including the documentation example.
- The browser build uses the committed WASM dependency lockfile (`--locked`).

## Production browsers

The clean local production suite passed **62 tests** across Chromium and
Firefox, including two isolated cache fixtures. Each cache fixture builds the
real application twice, changing only a harmless WebAssembly custom section.
The Rust algorithm and ABI remain identical. It then serves both builds from
one origin with a cacheable legacy module URL:

1. Load the first production build and deliberately warm `logdelta.wasm`.
2. Switch to the second production build without clearing the browser cache.
3. Confirm the legacy URL still returns the first module without a new server
   request, while the worker requests the second fingerprinted asset.
4. Compare both reports' checksums to the actual module bytes and confirm the
   findings remain identical.

This demonstrates a real HTTP cache boundary in both browsers; request
routing is not used in the cache fixtures because it disables browser caching.
Separate workflows reject HTTP 503, invalid binaries and valid modules with
incompatible exports, retain loaded inputs, and recover on retry. Report tests
compare the checksum to the served module body and block the legacy URL.

| Other workflow coverage | Checks |
| --- | --- |
| Import and worker ownership | Delayed imports, newer edits, removal/reset, cancellation, queued stale replies, retry |
| Evidence and downloads | Captured source order/settings, retained old reports after edits, intentional empty inputs, complete raw and portable exports |
| Bounded processing | Gzip/UTF-16 recovery, finding pagination, bounded expansion with complete exports |
| Keyboard and layout | Native tabs, disclosure/filter focus, eight baseline slots, 320 px and 1360 px, light/dark |
| Accessibility | Eight Axe WCAG 2.1 A/AA and best-practice scans per run; zero violations or horizontal overflow |
| Privacy and runtime | Zero page errors, unhandled rejections, CSP violations or off-origin requests in workflow diagnostics; offline processing and blocked-storage recovery |

## Hosted integrity

Published the fresh production build with a normal, appended `gh-pages`
commit `be0728e322f7188e54bdd0f146113ae46aa847e0`, preserving its branch history. All **31 built files** fetched from
[the deployed origin](https://antonsoo.github.io/logdelta/) matched the clean
build by SHA-256. The page retains its production CSP, including
`connect-src 'self'` and the WASM script directive. The
[timestamped manifest](verification-assets-2026-10-04.json) records every hash.

| Executed engine asset | Value |
| --- | --- |
| Path | `assets/logdelta-yBvBleeb.wasm` |
| Bytes | 1,184,319 |
| SHA-256 | `aa32993744e91a4459cfb893a78b23dc00d226639e27f5ceadda0bbf06ffa618` |

The hosted production suite passed **60 workflows**, with only the two local
cache fixtures intentionally skipped. Its eight Axe scans found zero
violations. Reviewed hosted Chromium and Firefox screenshots at 1360 px in
light mode and 320 px in dark mode; all four visits exported the expected
module checksum and recorded zero console/page errors, CSP violations,
off-origin requests or document overflow.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo build --lib --no-default-features --locked
cargo +1.85.0 build --locked
cargo +1.85.0 test --locked
cargo fmt --manifest-path web/wasm/Cargo.toml -- --check
cargo clippy --manifest-path web/wasm/Cargo.toml --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --manifest-path web/wasm/Cargo.toml --locked
cd web
npm ci
npm run lint
npm run typecheck
npm test
npm run test:browser
npm run verify:hosted
LOGDELTA_BASE_URL=https://antonsoo.github.io/logdelta/ npx playwright test
```

## Limits

The checksum identifies the executed artifact, not a signature, audit, source
revision or semantic version. Different compilers and build paths can produce
different bytes from the same source; the fresh-checkout artifact above is the
one published and verified. Comparison timing still excludes module loading,
hashing and initialization. Cancel/reset may need site assets again; an old
open tab may need a reload after deployment. No crate publication, binary
release or version tag was performed.
