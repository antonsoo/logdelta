// Compiles real engines in isolated source trees. The flag probe is added only
// to disposable copies; neither the application nor caller configuration changes.
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { access, appendFile, cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { constants } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { TextDecoder, TextEncoder } from "node:util";

const project = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const directory = await mkdtemp(join(tmpdir(), "logdelta-wasm-verify-"));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const results = [];
const baseEnv = { ...process.env, CARGO_TERM_COLOR: "never", CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? "2" };
for (const key of ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]) delete baseEnv[key];
const marker = Buffer.from("synthetic stale default-target artifact");
const stagedPath = (root) => join(root, "web/src/generated/logdelta.wasm");
const configPath = (root) => join(root, "web/wasm/.cargo/config.toml");
const defaultArtifact = (root) => join(root, "web/wasm/target/wasm32-unknown-unknown/release/logdelta_wasm.wasm");

async function copySource(name) {
  const root = join(directory, name);
  for (const path of ["Cargo.toml", "Cargo.lock", "src", "examples", "web/package.json", "web/scripts/build-wasm.mjs", "web/scripts/canonical-metadata.rs", "web/wasm/Cargo.toml", "web/wasm/Cargo.lock", "web/wasm/build.rs", "web/wasm/src"]) {
    const destination = join(root, path);
    await mkdir(dirname(destination), { recursive: true });
    await cp(join(project, path), destination, { recursive: true });
  }
  await mkdir(dirname(configPath(root)), { recursive: true });
  return root;
}
function build(root, env) {
  return spawnSync(process.execPath, ["scripts/build-wasm.mjs"], {
    cwd: join(root, "web"), env, encoding: "utf8", maxBuffer: 16 * 1024 * 1024,
  });
}
async function compileFixture(name, source) {
  const path = join(directory, `${name}${process.platform === "win32" ? ".exe" : ""}`);
  const input = join(directory, `${name}.rs`);
  await writeFile(input, source);
  const result = spawnSync("rustc", [input, "--crate-name", "logdelta_build_probe", "--edition=2021", "-D", "warnings", "-o", path], { env: baseEnv, encoding: "utf8" });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return path;
}
async function executable(name) {
  for (const path of baseEnv.PATH.split(delimiter)) {
    const candidate = join(path, `${name}${process.platform === "win32" ? ".exe" : ""}`);
    try { await access(candidate, constants.X_OK); return candidate; } catch { /* Try the next PATH entry. */ }
  }
  throw new Error(`Could not find the real ${name} executable for failure injection.`);
}
async function retainedAfterFailure(root, name, env, error) {
  const retained = await readFile(stagedPath(root));
  const failed = build(root, env);
  assert.ifError(failed.error);
  assert.notEqual(failed.status, 0, `${name}: build must fail`);
  assert.match(failed.stderr, error, `${name}: failed for the wrong reason`);
  assert((await readFile(stagedPath(root))).equals(retained), `${name}: changed the previous staged engine`);
  results.push({ name, sha256: digest(retained) });
  console.log(`PASS ${name}`);
}
async function engine(root, name, env, probe) {
  const result = build(root, env);
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${name}: ${result.stderr}`);
  const bytes = await readFile(stagedPath(root));
  assert(!bytes.equals(marker), `${name}: staged a stale default artifact`);
  assert(!bytes.includes(Buffer.from(root)), `${name}: leaked the checkout prefix`);
  assert(!bytes.includes(Buffer.from("/registry/src/")), `${name}: leaked a Cargo source cache path`);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  if (probe !== undefined) assert.equal(instance.exports.ld_flag_probe(), probe, `${name}: caller flags changed or disappeared`);
  const request = new TextEncoder().encode(JSON.stringify({ baselines: ["started\ncompleted\n"], target: "started\ncatastrophic failure\n", context: 2, masks: [] }));
  const e = instance.exports;
  const pointer = e.ld_alloc(request.length);
  new Uint8Array(e.memory.buffer, pointer, request.length).set(request);
  assert.equal(e.ld_diff(pointer, request.length), 0);
  const diff = JSON.parse(new TextDecoder().decode(new Uint8Array(e.memory.buffer, e.ld_result_ptr(), e.ld_result_len())));
  assert.deepEqual(diff.baseline_totals, [2]);
  assert(diff.findings.length > 0);
  results.push({ name, sha256: digest(bytes), bytes: bytes.length, ...(probe === undefined ? {} : { probe }) });
  console.log(`PASS ${name}`);
  return bytes;
}

try {
  const first = await copySource("first-checkout");
  const second = await copySource("different checkout 'with spaces' é");
  const firstEnv = { ...baseEnv, CARGO_TARGET_DIR: join(first, "web/wasm/target") };
  const secondTarget = join(directory, "custom output with spaces");
  const secondEnv = { ...baseEnv, CARGO_TARGET_DIR: secondTarget };
  const original = await engine(first, "standard target layout", firstEnv);
  await mkdir(dirname(defaultArtifact(second)), { recursive: true });
  await writeFile(defaultArtifact(second), marker);
  const other = await engine(second, "different checkout and CARGO_TARGET_DIR", secondEnv);
  assert.equal(digest(other), digest(original), "same source/toolchain must rebuild byte-identically across checkout paths");
  assert((await readFile(defaultArtifact(second))).equals(marker), "custom build must not use the stale default cache");

  // Only this disposable engine gains an observable cfg-dependent export.
  await appendFile(join(second, "web/wasm/src/lib.rs"), `
#[allow(unexpected_cfgs)]
#[no_mangle]
pub extern "C" fn ld_flag_probe() -> u32 {
    (cfg!(logdelta_flag_probe = "plain") as u32)
        | ((cfg!(logdelta_flag_probe = "encoded value with spaces") as u32) << 1)
        | ((cfg!(logdelta_flag_probe = "target") as u32) << 2)
        | ((cfg!(logdelta_flag_probe = "build") as u32) << 3)
        | ((cfg!(logdelta_flag_probe = "cfg") as u32) << 4)
}
`);
  const flag = (value) => `--cfg=logdelta_flag_probe="${value}"`;
  const array = (value) => JSON.stringify([flag(value)]);
  const configuredTarget = join(directory, "Cargo-configured target");
  await writeFile(configPath(second), `[build]\ntarget-dir=${JSON.stringify(configuredTarget)}\n`);
  await engine(second, "Cargo-configured target directory", baseEnv, 0);

  const cases = [
    ["RUSTFLAGS", { RUSTFLAGS: flag("plain") }, "", 1],
    ["encoded flags with spaces and priority", { RUSTFLAGS: "--invalid-lower-priority-flag", CARGO_ENCODED_RUSTFLAGS: `--cfg\x1flogdelta_flag_probe="encoded value with spaces"` }, "", 2],
    ["target environment flags", { CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS: flag("target") }, "", 4],
    ["build environment flags", { CARGO_BUILD_RUSTFLAGS: flag("build") }, "", 8],
    ["build config array", {}, `[build]\nrustflags=${array("build")}\n`, 8],
    ["build config string", {}, `[build]\nrustflags=${JSON.stringify(flag("build"))}\n`, 8],
    ["target config flags", {}, `[target.wasm32-unknown-unknown]\nrustflags=${array("target")}\n`, 4],
    ["matching cfg flags", {}, `[target.'cfg(target_arch = "wasm32")']\nrustflags=${array("cfg")}\n`, 16],
    ["joined target and cfg flags with build fallback excluded", {}, `[build]\nrustflags=${array("build")}\n[target.wasm32-unknown-unknown]\nrustflags=${array("target")}\n[target.'cfg(target_arch = "wasm32")']\nrustflags=${array("cfg")}\n`, 20],
    ["environment flags override config", { RUSTFLAGS: flag("plain") }, `[build]\nrustflags=${array("build")}\n[target.wasm32-unknown-unknown]\nrustflags=${array("target")}\n`, 1],
    ["caller crate metadata", { RUSTFLAGS: `${flag("plain")} -Cmetadata=caller-kept` }, "", 1],
  ];
  for (const [name, env, config, probe] of cases) {
    await writeFile(configPath(second), config);
    await engine(second, name, { ...secondEnv, ...env }, probe);
  }
  // Real outer/workspace wrappers must still see and execute the compiler call.
  const wrapper = await compileFixture("outer wrapper with spaces", `
use std::{env, fs::OpenOptions, io::Write, process::{self, Command}};
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let line = std::iter::once(env::current_exe().unwrap().into_os_string()).chain(args.iter().cloned())
        .map(|value| value.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\\x1f");
    let mut file = OpenOptions::new().create(true).append(true).open(env::var_os("LOGDELTA_WRAPPER_TRACE").unwrap()).unwrap();
    writeln!(file, "{line}").unwrap();
    process::exit(Command::new(&args[0]).args(&args[1..]).status().unwrap().code().unwrap_or(1));
}
`);
  const workspaceWrapper = join(directory, `workspace wrapper${process.platform === "win32" ? ".exe" : ""}`);
  await cp(wrapper, workspaceWrapper);
  const trace = join(directory, "wrapper calls.txt");
  for (const configured of [false, true]) {
    await writeFile(trace, "");
    await writeFile(configPath(second), configured ? `[build]\nrustc-wrapper=${JSON.stringify(wrapper)}\nrustc-workspace-wrapper=${JSON.stringify(workspaceWrapper)}\n` : "");
    await engine(second, `${configured ? "configured" : "environment"} compiler wrappers retain caller metadata`, {
      ...secondEnv, LOGDELTA_WRAPPER_TRACE: trace, RUSTFLAGS: `${flag("plain")} -Cmetadata=caller-kept`,
      ...(configured ? {} : { RUSTC_WRAPPER: wrapper, RUSTC_WORKSPACE_WRAPPER: workspaceWrapper }),
    }, 1);
    const calls = (await readFile(trace, "utf8")).trim().split("\n").map((line) => line.split("\x1f"));
    const linked = calls.filter((args) => args.includes("logdelta_wasm") && args.some((arg) => arg.startsWith("--emit=") && arg.includes("link")));
    assert(linked.some((args) => args[0] === wrapper && args[1] === workspaceWrapper), "outer wrapper lost the workspace wrapper");
    assert(linked.some((args) => args[0] === workspaceWrapper), "workspace wrapper did not execute");
    assert(linked.every((args) => args.includes("-Cmetadata=caller-kept") && args.some((arg) => arg.startsWith("metadata=logdelta_"))), "caller or stable crate metadata was lost");
  }
  await writeFile(configPath(second), "");
  await retainedAfterFailure(second, "failed compilation retains staged engine", { ...secondEnv, RUSTFLAGS: "--invalid-logdelta-flag" }, /invalid-logdelta-flag/);

  // Cargo protocol failures must never fall back to the stale default artifact.
  // The proxy delegates the real check/build and only injects the named failure.
  const realCargo = await executable("cargo");
  const proxy = await compileFixture("cargo proxy", `
use std::{env, io::{self, Write}, process::{self, Command}};
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let mode = env::var("LOGDELTA_CARGO_FAILURE").unwrap();
    if args[0] == "metadata" && mode == "metadata" {
        eprintln!("synthetic Cargo metadata failure");
        process::exit(9);
    }
    let output = Command::new(env::var_os("LOGDELTA_REAL_CARGO").unwrap()).args(&args).output().unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    if args[0] == "build" && mode == "artifact" && output.status.success() {
        for line in String::from_utf8(output.stdout).unwrap().lines() {
            if !(line.contains(r#""reason":"compiler-artifact""#) && line.contains(r#""name":"logdelta_wasm""#)) {
                println!("{line}");
            }
        }
    } else { io::stdout().write_all(&output.stdout).unwrap(); }
    process::exit(output.status.code().unwrap_or(1));
}
`);
  const proxyDir = join(directory, "proxy-bin");
  await mkdir(proxyDir);
  await cp(proxy, join(proxyDir, `cargo${process.platform === "win32" ? ".exe" : ""}`));
  const proxyEnv = { ...secondEnv, PATH: `${proxyDir}${delimiter}${baseEnv.PATH}`, LOGDELTA_REAL_CARGO: realCargo };
  await retainedAfterFailure(second, "failed metadata retains staged engine", { ...proxyEnv, LOGDELTA_CARGO_FAILURE: "metadata" }, /Could not read the engine's Cargo metadata/);
  await retainedAfterFailure(second, "missing artifact retains staged engine", { ...proxyEnv, LOGDELTA_CARGO_FAILURE: "artifact" }, /exactly one compiled engine artifact/);
  console.log(JSON.stringify({ cases: results.length, reproducibleSha256: digest(original), results }, null, 2));
} finally {
  await rm(directory, { recursive: true, force: true });
}
