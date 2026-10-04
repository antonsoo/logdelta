// Compiles web/wasm (the logdelta library behind a C ABI) for a fingerprinted
// Vite import, and stages repository examples in public/. Both are generated output.
import { spawnSync } from "node:child_process";
import { Buffer } from "node:buffer";
import { createHash, randomUUID } from "node:crypto";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = dirname(dirname(fileURLToPath(import.meta.url)));
// The browser report names the engine version. Do not publish misleading provenance.
const version = JSON.parse(readFileSync(join(web, "package.json"), "utf8")).version;
const crateVersion = readFileSync(join(web, "..", "Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (version !== crateVersion) throw new Error(`web/package.json version ${version} does not match the Rust engine ${crateVersion}`);
const crate = join(web, "wasm");
const project = realpathSync(join(web, ".."));
const target = "wasm32-unknown-unknown";

function cargo(args, env = process.env) {
  const result = spawnSync("cargo", args, {
    cwd: crate, env, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  return result;
}
function messages(command, env) {
  const result = cargo([command, "--locked", "--release", "--target", target, "--message-format=json"], env);
  const records = result.stdout.split(/\r?\n/).filter((line) => line.trim()).map((line) => JSON.parse(line));
  for (const record of records) {
    if (record.reason === "compiler-message" && record.message?.rendered) process.stderr.write(record.message.rendered);
  }
  if (result.status !== 0 || !records.some((record) => record.reason === "build-finished" && record.success)) {
    throw new Error(`Cargo ${command} failed; the previous staged engine was retained.`);
  }
  return records;
}
const fromWrapper = (record) => record.reason === "compiler-artifact" && record.manifest_path && resolve(record.manifest_path) === join(crate, "Cargo.toml");

// A check runs the small build script with Cargo's own resolved flags. Keeping
// this separate from the remapped build preserves all supported flag sources,
// including encoded arguments with spaces and matching target/cfg config.
const captureEnv = { ...process.env, LOGDELTA_CAPTURE_TOKEN: randomUUID() };
const checked = messages("check", captureEnv);
const script = checked.find((record) => fromWrapper(record) && record.target.kind.includes("custom-build"));
const executed = checked.find((record) => record.reason === "build-script-executed" && record.package_id === script?.package_id);
if (!executed) throw new Error("Cargo did not identify the engine's flag-capture build script.");
const encoded = readFileSync(join(executed.out_dir, "logdelta-rustflags"), "utf8");
const flags = encoded ? encoded.split("\x1f") : [];
const compiler = readFileSync(join(executed.out_dir, "logdelta-RUSTC"), "utf8");
const previousWrapper = readFileSync(join(executed.out_dir, "logdelta-RUSTC_WRAPPER"), "utf8");
if (!compiler) throw new Error("Cargo did not identify its compiler.");

const metadataResult = cargo(["metadata", "--locked", "--format-version=1", "--filter-platform", target]);
if (metadataResult.status !== 0) throw new Error("Could not read the engine's Cargo metadata.");
const metadata = JSON.parse(metadataResult.stdout);
const identities = [];
// Give dependency sources stable aliases too; local Cargo cache paths must not
// become part of a public engine's panic strings. The project map comes last.
for (const pkg of metadata.packages) {
  const directory = dirname(pkg.manifest_path);
  if ([...directory].some((character) => character.charCodeAt(0) < 32 || character === "\x7f")) throw new Error("A source path contains unsupported control characters.");
  const origin = pkg.source ?? `local:${relative(project, directory).replaceAll("\\", "/")}`;
  identities.push(`${directory}\t${origin}#${pkg.name}@${pkg.version}`);
  if (pkg.source) flags.push(`--remap-path-prefix=${realpathSync(directory)}=logdelta-deps/${pkg.name}-${pkg.version}`);
}
flags.push(`--remap-path-prefix=${project}=logdelta`);
const wrapperSource = join(web, "scripts/canonical-metadata.rs");
// Cargo does not fingerprint ordinary RUSTC_WRAPPER changes. Track its source
// in the compiler flags so a warm cache cannot bypass updated canonicalization.
flags.push(`--cfg=logdelta_build_pipeline="${createHash("sha256").update(readFileSync(wrapperSource)).digest("hex")}"`);
// Cargo's path-dependent crate salts can reorder functions even after source
// names are remapped. Use stable package/version/source/cfg identities for its
// metadata only; the wrapper verifies and retains any caller metadata flags.
const wrapper = join(executed.out_dir, `logdelta-metadata-wrapper${process.platform === "win32" ? ".exe" : ""}`);
const compiledWrapper = spawnSync(compiler, [wrapperSource, "--crate-name", "logdelta_metadata_wrapper", "--edition=2021", "-D", "warnings", "-O", "-o", wrapper], { cwd: crate, stdio: "inherit" });
if (compiledWrapper.error) throw compiledWrapper.error;
if (compiledWrapper.status !== 0) throw new Error("Could not compile the scoped metadata wrapper.");
const built = messages("build", {
  ...captureEnv, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f"), RUSTC_WRAPPER: wrapper,
  LOGDELTA_PREVIOUS_RUSTC_WRAPPER: previousWrapper, LOGDELTA_CRATE_IDENTITIES: identities.join("\n"), LOGDELTA_ORIGINAL_RUSTFLAGS: encoded,
});
const artifacts = built.filter((record) => fromWrapper(record) && record.target.crate_types.includes("cdylib"))
  .flatMap((record) => record.filenames.filter((file) => file.endsWith(".wasm")));
if (artifacts.length !== 1) throw new Error("Cargo did not identify exactly one compiled engine artifact.");
// Follow Cargo's artifact record, never a guessed target directory that may
// contain an older binary. Stage the exact buffer inspected here.
const bytes = readFileSync(resolve(crate, artifacts[0]));
if (bytes.includes(Buffer.from(project))) throw new Error("The compiled engine still contains the absolute checkout path.");

const publicDir = join(web, "public");
mkdirSync(join(publicDir, "examples", "large"), { recursive: true });
// Imported by the worker so Vite fingerprints the engine and changes the
// worker dependency URL whenever the binary changes. Never reuse a public
// fixed-name cache key for versioned engine code.
const generatedDir = join(web, "src", "generated");
mkdirSync(generatedDir, { recursive: true });
writeFileSync(join(generatedDir, "logdelta.wasm"), bytes);
rmSync(join(publicDir, "logdelta.wasm"), { force: true });

const examples = join(web, "..", "examples");
for (const dir of ["", "large"]) {
  for (const name of readdirSync(join(examples, dir))) {
    if (name.endsWith(".log")) copyFileSync(join(examples, dir, name), join(publicDir, "examples", dir, name));
  }
}
console.log(`staged engine (${(bytes.length / 1024).toFixed(0)} KiB) for hashed worker import and example logs in public/`);
