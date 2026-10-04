# Logdelta WASM build verification - 2026-10-04

This follows the [earlier browser-cache verification](verification-2026-10-04.md).
It fixes two build problems without changing the native 0.3.4 engine, CLI or
browser ABI. The [build guide](wasm-build.md) explains the configuration and limits.

## Reproduced failures

From source `21f1031`, an isolated build with `CARGO_TARGET_DIR` outside the
checkout compiled the new module in that directory but staged a synthetic
stale marker from the old hardcoded default path. The build exited zero.
With a real older binary in that path, a successful build could ship the old engine.
The builder now requires Cargo's successful artifact record and stages its exact bytes.

Identical sources in separate checkouts originally embedded local paths and
produced different module hashes. Remapping source paths alone still left
different function ordering. The scoped wrapper now canonicalizes Cargo's WASM
crate metadata while retaining caller flags and compiler-wrapper chaining.

The first clean build comparison also exposed warm Cargo-cache reuse: updating
an ordinary compiler wrapper did not invalidate the cached module. The wrapper's
source fingerprint now appears in Cargo-tracked flags. A real fixture updates
the wrapper over a valid existing module without cleaning the target cache.

## Source and build evidence

- Implementation commits: `2df0fba` and `0b93f60`.
- Final application source: `0b93f60bcf8d197abed9ea1e8fc1125d9f76aaed`.
- Fresh source archive: Node **24.21.0**, npm **11.19.0**, Rust **1.91.0**.
- `npm ci`: 134 packages installed, 135 audited, zero reported vulnerabilities.
- Lint, TypeScript, **55 unit tests / 5 files**, production build: passed.
- Existing warm checkout and fresh archive: all **31 production files** matched
  by complete SHA-256, including the module, worker and application assets.
- Root Rust formatting/Clippy, **128 library + 33 integration + 1 documentation
  test**, and library-only build: passed.
- WASM wrapper formatting/Clippy/native ABI test and standalone compiler-wrapper
  formatting/Clippy: passed.
- The build uses the committed dependency lockfile. Rust **1.85.0** also compiled
  the complete browser pipeline; its module was instantiated and ran a real diff.

| Final production engine | Value |
| --- | --- |
| Asset | `assets/logdelta-BtpnaYob.wasm` |
| Bytes | 1,180,056 |
| SHA-256 | `a55f40bf1deef600db6687267ad9800d46cb72d54ffc3a8c2f378b382473ce54` |

## Real compiler matrix

`npm run verify:wasm` passed **20 real-compiler cases**. Its
[machine-readable record](verification-wasm-matrix-2026-10-04.json) includes the
locked dependency hashes, module hashes, sizes and observed probe values.
Both isolated final modules matched the production engine checksum above.

| Cases | Evidence |
| --- | --- |
| Warm cache and standard layout | Replacing a transparent older compiler wrapper rebuilds a valid cached module without cleaning |
| Custom and Cargo-configured targets | Cargo's selected artifact is staged; the synthetic stale default artifact remains unused |
| Eleven caller-flag cases | Plain/encoded environment flags, target/build environment settings, config arrays/strings, matching cfg, joined target/cfg, precedence and caller crate metadata |
| Two compiler-wrapper cases | Environment and config outer/workspace wrappers still execute; their outputs match the same caller-metadata build |
| Three failure cases | Invalid compiler flags, failed metadata and a withheld successful artifact record return failure and preserve the previous staged module |

All successful cases instantiate the real module, run a diff and reject embedded
checkout/cache prefixes. Only disposable copies gain the cfg probe. SHA-256
strings are compared rather than printing large binary-buffer diffs on failure.

## Production browsers

The final clean build passed **62 Chromium/Firefox workflows**, including the
two real production-build cache fixtures. Eight Axe WCAG 2.1 A/AA and
best-practice scans found zero violations. Import ownership, cancellation,
worker/module recovery, full downloads, report checksums, bounded evidence,
keyboard navigation, offline processing, blocked storage, 320 px/1360 px
light/dark layouts, CSP and off-origin network diagnostics remained green.

## Hosted verification

The final site was published by appended `gh-pages` commit
`77974f9b97c6e77ae8d4ac4237b92e5bfcb37bfa`, preserving the branch's history.
All **31 hosted files** matched the fresh production build by SHA-256. The
[timestamped hosted manifest](verification-wasm-assets-2026-10-04.json) records
every asset. The hosted production suite passed **60 workflows**, with only the
two local cache fixtures intentionally skipped. Its eight Axe scans found zero
violations.

Reviewed four hosted Chromium/Firefox visits at 1360 px/light and 320 px/dark.
Each downloaded a report with the exact production module checksum and nine
raw template findings, matching the native pytest comparison. The reviewed
screens show three grouped cards (ten findings before grouping, including the
separate new-value finding). All four recorded zero console/page errors,
CSP violations, off-origin requests or document overflow. The log panels retain
their own horizontal scrolling at narrow widths. The
[visit record](verification-wasm-visual-2026-10-04.json),
[desktop screenshot](assets/wasm-hosted-desktop-2026-10-04.png) and
[mobile screenshot](assets/wasm-hosted-mobile-2026-10-04.png) preserve this evidence.

No crate publication, binary release or version tag is part of this change.

## Reproduce

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo test --doc --locked
cargo build --lib --no-default-features --locked
cd web
rustfmt --check scripts/canonical-metadata.rs
cargo fmt --manifest-path wasm/Cargo.toml -- --check
cargo clippy --manifest-path wasm/Cargo.toml --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --manifest-path wasm/Cargo.toml --locked
npm ci
npm run lint
npm run typecheck
npm test
npm run verify:wasm
npm run test:browser
npm run verify:hosted
LOGDELTA_BASE_URL=https://antonsoo.github.io/logdelta/ npx playwright test
```

## Limits

Reproducibility was checked on Linux with the same Rust/host toolchain, sysroot,
source, locks, target and effective flags. Different compilers, intentional
flags or custom wrapper behavior can produce different bytes. The declared
minimum compiler produces a working module with its own checksum. The report
checksum identifies executed bytes; it is not a source attestation or signature.
An already open tab still needs a reload to load a newly deployed application.
