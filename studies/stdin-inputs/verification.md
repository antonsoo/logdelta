# Archived and installed verification

Verified source `0f67251` on 2026-10-14. The baseline release binary was built
from an independent archive of `9f20fb0`; all captures use synthetic log lines
and actual subprocess pipes. No registry release or deployment is included.

| Workflow | Result |
|---|---|
| Baseline CLI pipe replay | 16 cases; nine disagreements |
| Fresh archived release build | All 16 agree |
| Independently installed packaged crate, Rust 1.91.0 | All 16 agree |
| Independently installed packaged crate, Rust 1.85.0 | All 16 agree |
| Seven supported control streams | Complete exit/stdout/stderr unchanged from baseline |
| Source/archive/packaged runtime parity | All 21 source files byte-identical |
| Archived Rust 1.91.0 checks | Format, Clippy with warnings denied, 222 native checks, release build and verified `cargo package` passed |
| Archived Rust 1.85.0 | 222 native checks and one documentation example passed |
| Packaged Rust 1.85.0 stdin boundary | All five focused checks passed |
| Fresh web checks | Rust format/Clippy, three WASM checks, lint/types, 83 JavaScript checks and build passed |
| WASM build conformance | All 20 isolated build cases passed |
| Fresh Chromium/Firefox browser workflows | All 128 passed |

The first attempt to run the entire development suite inside the extracted
crate failed three integration tests: they require `examples/large`, which the
package deliberately excludes. This is retained in the local raw log. The full
source archive was then checked with Rust 1.85.0, and the packaged crate's five
stdin checks passed separately. A full packaged development-suite pass is not
claimed. Both independently installed binaries passed the complete CLI replay.

Fresh native commands were `cargo fmt --all -- --check`,
`cargo clippy --locked --all-targets --all-features -- -D warnings`,
`cargo test --locked --all-targets --all-features`, `cargo build --locked --release`
and `cargo package --locked`. The resulting crate was extracted and installed
with `cargo install --locked --path ...` in independent target/install directories,
once with default Rust 1.91.0 and once with `cargo +1.85.0`.

The archived web ran its configured Rust checks, `npm ci`, lint, type checking,
unit checks, `npm run verify:wasm`, build and both-engine Playwright suite. These
are regression checks of the existing browser; repeated `-` is a native input
contract and has no browser equivalent.

The packaged `logdelta 0.3.4` crate is 222,112 bytes, SHA-256
`5755863057d833a01af69d2184aae5c8bc7695ea17bae809b85835f9568649b3`.
`Cargo.toml.orig` matches the archive. The native binary hashes differ across
build locations/toolchains and are recorded separately; source parity does not
claim identical native binaries.

[Artifact receipt](results/artifact-verification.json),
[fresh archive replay](results/after.json),
[installed Rust 1.91 replay](results/installed-rust-1.91.json) and
[installed Rust 1.85 replay](results/installed-rust-1.85.json) retain the checked
results. The [terminal image](../../docs/assets/stdin-inputs.png) was rendered
from the [unedited installed-demo stdout](../../docs/assets/stdin-inputs.ansi)
and visually inspected. Full command logs remain in the local work directory.
