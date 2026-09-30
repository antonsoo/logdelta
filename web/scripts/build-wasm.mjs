// Compiles web/wasm (the logdelta library behind a C ABI) for wasm32 and stages it, with the
// repository's example logs, into public/ for Vite to serve. Both are build output, not sources.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const web = dirname(dirname(fileURLToPath(import.meta.url)));
const crate = join(web, "wasm");
execFileSync("cargo", ["build", "--release", "--target", "wasm32-unknown-unknown"], { cwd: crate, stdio: "inherit" });

const publicDir = join(web, "public");
mkdirSync(join(publicDir, "examples", "large"), { recursive: true });
const wasm = join(crate, "target", "wasm32-unknown-unknown", "release", "logdelta_wasm.wasm");
copyFileSync(wasm, join(publicDir, "logdelta.wasm"));

const examples = join(web, "..", "examples");
for (const dir of ["", "large"]) {
  for (const name of readdirSync(join(examples, dir))) {
    if (name.endsWith(".log")) copyFileSync(join(examples, dir, name), join(publicDir, "examples", dir, name));
  }
}
console.log(`staged logdelta.wasm (${(statSync(wasm).size / 1024).toFixed(0)} KiB) and example logs in public/`);
