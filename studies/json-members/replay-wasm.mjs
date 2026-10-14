// Run the actual built C-ABI module, comparing public API reports at stated precision.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
const [modulePath, nativePath, outputPath] = process.argv.slice(2);
assert(modulePath && nativePath && outputPath, 'Expected ENGINE.wasm NATIVE_CAPTURE.json OUTPUT.json');
const cases = JSON.parse(await readFile(new URL('./cases.json', import.meta.url), 'utf8'));
const native = JSON.parse(await readFile(nativePath, 'utf8'));
const bytes = await readFile(modulePath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const engine = instance.exports;
const stable = value => JSON.parse(JSON.stringify(value, (_, value) => typeof value === 'number' && !Number.isInteger(value) ? Number(value.toPrecision(12)) : value));
const records = [];
for (const item of cases) {
  const request = { baselines: [(item.baseline + '\n').repeat(12)], target: (item.target + '\n').repeat(12), context: 0, masks: [] };
  const encoded = new TextEncoder().encode(JSON.stringify(request));
  const pointer = engine.ld_alloc(encoded.length);
  new Uint8Array(engine.memory.buffer, pointer, encoded.length).set(encoded);
  assert.equal(engine.ld_diff(pointer, encoded.length), 0, item.id);
  const output = new Uint8Array(engine.memory.buffer, engine.ld_result_ptr(), engine.ld_result_len());
  const report = JSON.parse(new TextDecoder().decode(output));
  const expected = native.records.find(row => row.id === item.id && row.kind === 'public-api').diff;
  assert.deepEqual(stable(report), stable(expected), item.id);
  records.push({ id: item.id, nativeMatches: true, report });
}
await writeFile(outputPath, JSON.stringify({ node: process.version, moduleBytes: bytes.length,
  moduleSha256: createHash('sha256').update(bytes).digest('hex'), workflows: records.length,
  comparison: 'Full public API reports; integers/text/source evidence exact, floating-point scores at 12 significant digits.', records }, null, 2) + '\n');
console.log(`${records.length} WASM public API reports match native`);
