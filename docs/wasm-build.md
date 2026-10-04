# Building the browser engine

From `web/`, `npm run wasm` compiles the locked Rust engine and stages it for
Vite. `npm run build` then produces the complete site in `web/dist/`.

```sh
rustup target add wasm32-unknown-unknown
cd web
npm ci
npm run build
npm run verify:wasm
```

The JavaScript build tools use argument arrays, so checkout and target paths
can contain spaces, quotes and Unicode. Node 24 and Rust 1.85 or newer are
supported. The root engine and browser package versions must agree.

## Cargo owns configuration and artifact selection

The old script copied a guessed filename under `web/wasm/target/`. When
`CARGO_TARGET_DIR` or Cargo configuration chose another directory, that path
could contain an older module and the build could silently stage it.

The builder now reads Cargo's JSON `compiler-artifact` message for the wrapper's
`cdylib`, requires exactly one WASM artifact and reads those bytes. It also
requires successful compilation. Compilation, metadata or artifact-selection
failure leaves the previously staged `src/generated/logdelta.wasm` intact and
returns a nonzero exit status; there is no fallback to the default cache.

Compiler configuration is resolved by Cargo, including its documented
[flag precedence](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags).
A small build script records the effective `CARGO_ENCODED_RUSTFLAGS`, compiler
and outer wrapper from Cargo's
[build-script environment](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts).
The subsequent build retains those arguments, including arguments with spaces,
and appends the source-path remappings. Target-triple and matching `cfg` flags,
build-config arrays/strings, and environment overrides retain Cargo's behavior.

```text
Cargo check --------> effective flags + compiler/wrapper context
Cargo metadata -----> package source/version identities
                           |
                           v
Cargo build -> scoped metadata wrapper -> prior outer/workspace wrappers -> rustc
     |
     v
compiler-artifact filenames -> exact WASM bytes -> Vite fingerprint -> worker
```

Only temporary verification copies gain a cfg-dependent test export. The
application's Rust algorithm and ABI are unchanged.

## Rebuilding from another checkout

Absolute checkout paths and Cargo dependency-cache paths used in source/panic
locations are [remapped](https://doc.rust-lang.org/rustc/remap-source-paths.html)
to `logdelta/...` and `logdelta-deps/<name>-<version>/...`. This also keeps local
paths out of the distributed module.

Path remapping alone was insufficient: Cargo's path-dependent crate metadata
changed function ordering and therefore the module checksum. A scoped native
compiler wrapper canonicalizes only Cargo's `-C metadata` for WASM crates,
using package origin/name/version, crate name and sorted cfg values. Caller
metadata arguments are verified and retained. Host build scripts, proc macros
and compiler probes retain their ordinary metadata. Existing outer and
workspace compiler wrappers still execute with the remaining arguments.

`npm run verify:wasm` compiles real engines from two isolated checkouts and
compares their complete SHA-256 values. One checkout contains spaces, a quote
and Unicode, uses a separate target directory and contains a synthetic stale
artifact in the old default location. The command also instantiates the engines
and runs a real diff, observes each flag source through a disposable cfg probe,
checks both configured and environment compiler wrappers, and injects compiler,
metadata and missing-artifact failures to check preservation of the staged module.
All verification directories are removed afterward; caller configuration is
not edited.

Reproducibility here means the same source, locks, Rust/host toolchain, target
and effective flags produce the same bytes across checkout paths. Different
compilers, sysroots, intentional compiler flags or custom wrapper behavior can
change the module. The checksum in `engine.wasm_sha256` identifies the exact
bytes executed by a worker; it is not a source attestation or signature.
