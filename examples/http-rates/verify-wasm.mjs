// Compare the actual compiled WASM engine with the native CLI, including unknown rate coverage.
// node examples/http-rates/verify-wasm.mjs <binary> <engine.wasm> <output-directory>
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const [binaryArg, wasmArg, outputArg] = process.argv.slice(2);
if (!binaryArg || !wasmArg || !outputArg) throw new Error('Usage: verify-wasm.mjs <binary> <engine.wasm> <output-directory>');
const binary = resolve(binaryArg), wasmPath = resolve(wasmArg), output = resolve(outputArg);
mkdirSync(output, { recursive: true });
const here = dirname(fileURLToPath(import.meta.url));
const files = ['good', 'good-2', 'failed'].map(name => join(here, `http-rate-${name}.log`));
const baselines = files.slice(0, 2).map(path => readFileSync(path, 'utf8'));
const target = readFileSync(files[2], 'utf8');
const bytes = readFileSync(wasmPath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const api = instance.exports;

function wasm(request) {
  const encoded = new TextEncoder().encode(JSON.stringify(request));
  const pointer = api.ld_alloc(encoded.length);
  new Uint8Array(api.memory.buffer, pointer, encoded.length).set(encoded);
  const code = api.ld_diff(pointer, encoded.length);
  const result = JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer, api.ld_result_ptr(), api.ld_result_len())));
  assert.equal(code, 0, JSON.stringify(result));
  return result;
}

const cases = [];
for (const [name, text, enabled] of [
  ['enabled', target, true], ['disabled', target, false], ['unchanged', baselines[0], true],
  ['missing-group', target + '{"route":"/new","http":{"status":200}}\n', true],
  ['invalid-record', target + '{not-json}\n', true]
]) {
  const path = join(output, `${name}.jsonl`);
  writeFileSync(path, text);
  let stdout, exit = 0;
  const args = ['diff', ...files.slice(0, 2), '--target', path, '--watch-field', '/http/status', '--watch-by', '/route', '--json'];
  if (enabled) args.push('--watch-rate-change', '5');
  try { stdout = execFileSync(binary, args, { encoding: 'utf8', timeout: 30_000 }); }
  catch (error) { assert.ok([1, 2].includes(error.status), error.stderr); stdout = error.stdout; exit = error.status; }
  const native = JSON.parse(stdout);
  const result = wasm({ baselines, target: text, watch_fields: ['/http/status'], watch_by: ['/route'],
    ...(enabled ? { watch_rate_change: 5 } : {}) });
  // Floating-point library implementations can differ in the last bit between native and WASM.
  const stable = value => JSON.parse(JSON.stringify(value, (_, entry) => typeof entry === 'number' && !Number.isInteger(entry) ? Number(entry.toPrecision(12)) : entry));
  assert.deepEqual(stable(result), stable(native), name);
  const rates = result.watched_fields[0].rate_comparison;
  assert.equal(exit, ['missing-group', 'invalid-record'].includes(name) ? 2 : name === 'enabled' ? 1 : 0);
  cases.push({ name, nativeExit: exit, rateComplete: rates?.complete ?? null,
    changedGroups: rates?.groups.filter(group => group.changes.length).length ?? 0 });
}
const evidence = { wasmSha256: createHash('sha256').update(bytes).digest('hex'), cases };
writeFileSync(join(output, 'wasm-verification.json'), JSON.stringify(evidence, null, 2) + '\n');
console.log(JSON.stringify(evidence));
