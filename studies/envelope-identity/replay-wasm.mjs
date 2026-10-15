// Compare the built engine with every full report from replay.py's after directory.
// node replay-wasm.mjs ENGINE.wasm CLI_REPLAY_DIRECTORY
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";

const [modulePath, directory] = process.argv.slice(2);
assert(modulePath && directory, "Expected ENGINE.wasm CLI_REPLAY_DIRECTORY");
const cases = JSON.parse(await readFile(new URL("results/after.json", import.meta.url), "utf8")).cases;
const bytes = await readFile(modulePath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const engine = instance.exports;
const stable = (value) => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "number" && !Number.isInteger(v) ? Number(v.toPrecision(12)) : v));
const results = [];
for (const entry of cases) {
  const getFlags = (flag) => entry.flags.flatMap((item, i) => item === flag ? [entry.flags[i + 1]] : []);
  const rate = getFlags("--watch-rate-change")[0];
  const request = {
    baselines: [await readFile(join(directory, entry.case, "baseline.log"), "utf8")],
    target: await readFile(join(directory, entry.case, "target.log"), "utf8"),
    context: 0, masks: [], watch_fields: getFlags("--watch-field"), watch_by: getFlags("--watch-by"),
    ...(rate === undefined ? {} : { watch_rate_change: Number(rate) }),
  };
  const encoded = new TextEncoder().encode(JSON.stringify(request));
  const pointer = engine.ld_alloc(encoded.length);
  new Uint8Array(engine.memory.buffer, pointer, encoded.length).set(encoded);
  assert.equal(engine.ld_diff(pointer, encoded.length), 0, entry.case);
  const output = new Uint8Array(engine.memory.buffer, engine.ld_result_ptr(), engine.ld_result_len());
  const actual = JSON.parse(new TextDecoder().decode(output));
  const expected = JSON.parse(await readFile(join(directory, entry.case, "report.json"), "utf8"));
  assert.deepEqual(stable(actual), stable(expected), entry.case);
  results.push({ case: entry.case, matches_native: true });
}
console.log(JSON.stringify({ engine_sha256: createHash("sha256").update(bytes).digest("hex"), engine_bytes: bytes.length,
  comparison: "Full reports; exact integers, strings and evidence; floating-point scores at 12 significant digits.", cases: results }, null, 2));
