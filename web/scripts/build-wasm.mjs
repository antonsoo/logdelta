// Compiles web/wasm (the logdelta library behind a C ABI) for a fingerprinted
// Vite import, and stages repository examples in public/. Both are generated output.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const web = dirname(dirname(fileURLToPath(import.meta.url)));
// The browser report names the engine version. Do not publish misleading provenance.
const version = JSON.parse(readFileSync(join(web, "package.json"), "utf8")).version;
const crateVersion = readFileSync(join(web, "..", "Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (version !== crateVersion) throw new Error(`web/package.json version ${version} does not match the Rust engine ${crateVersion}`);
const crate = join(web, "wasm");
execFileSync("cargo", ["build", "--locked", "--release", "--target", "wasm32-unknown-unknown"], { cwd: crate, stdio: "inherit" });

const publicDir = join(web, "public");
mkdirSync(join(publicDir, "examples", "large"), { recursive: true });
const wasm = join(crate, "target", "wasm32-unknown-unknown", "release", "logdelta_wasm.wasm");
// Imported by the worker so Vite fingerprints the engine and changes the
// worker dependency URL whenever the binary changes. Never reuse a public
// fixed-name cache key for versioned engine code.
const generatedDir = join(web, "src", "generated");
mkdirSync(generatedDir, { recursive: true });
copyFileSync(wasm, join(generatedDir, "logdelta.wasm"));
rmSync(join(publicDir, "logdelta.wasm"), { force: true });

const examples = join(web, "..", "examples");
for (const dir of ["", "large"]) {
  for (const name of readdirSync(join(examples, dir))) {
    if (name.endsWith(".log")) copyFileSync(join(examples, dir, name), join(publicDir, "examples", dir, name));
  }
}
console.log(`staged engine (${(statSync(wasm).size / 1024).toFixed(0)} KiB) for hashed worker import and example logs in public/`);
