// Exercise the binary C ABI with GNU-produced files, independently checked by gunzip.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const [modulePath, binary, output] = process.argv.slice(2);
assert(modulePath && binary && output, 'Use verify.mjs MODULE NATIVE_BINARY OUTPUT');
const module = await readFile(resolve(modulePath));
const { instance } = await WebAssembly.instantiate(module, {});
const e = instance.exports;
assert.equal(typeof e.ld_gunzip, 'function');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const max = 25 * 1024 * 1024;
function call(operation, bytes) {
  const pointer = e.ld_alloc(bytes.length);
  new Uint8Array(e.memory.buffer, pointer, bytes.length).set(bytes);
  const code = e[operation](pointer, bytes.length);
  const result = Buffer.from(new Uint8Array(e.memory.buffer, e.ld_result_ptr(), e.ld_result_len()));
  return { code, bytes: result };
}
const manifest = JSON.parse(await readFile(new URL('captures/manifest.json', import.meta.url), 'utf8'));
const cases = [];
for (const capture of manifest.files) {
  const path = fileURLToPath(new URL(`captures/${capture.file}`, import.meta.url));
  const bytes = await readFile(path);
  assert.equal(sha256(bytes), capture.sha256);
  const gnu = spawnSync('gzip', ['--decompress', '--stdout', path], { maxBuffer: 60 * 1024 * 1024 });
  assert.equal(gnu.status, capture.gnu_gunzip_exit);
  const wasm = call('ld_gunzip', bytes);
  const accepted = capture.gnu_gunzip_exit === 0 && capture.plain_bytes <= max;
  assert.equal(wasm.code, accepted ? 0 : 1, `${capture.file}: ${wasm.bytes.subarray(0, 200)}`);
  if (accepted) {
    assert.equal(wasm.bytes.length, capture.plain_bytes);
    assert.equal(sha256(wasm.bytes), capture.plain_sha256);
    assert(wasm.bytes.equals(gnu.stdout), `${capture.file}: all decompressed bytes must match GNU gunzip`);
  } else {
    const error = JSON.parse(wasm.bytes).error;
    assert.equal(typeof error, 'string');
    assert(error.length);
  }
  cases.push({ file: capture.file, accepted, output_bytes: accepted ? wasm.bytes.length : null,
    output_sha256: accepted ? sha256(wasm.bytes) : null,
    error: accepted ? null : JSON.parse(wasm.bytes).error, complete_bytes_equal_gnu_gunzip: accepted });
}
const tooLarge = call('ld_gunzip', Buffer.alloc(max + 1));
assert.equal(tooLarge.code, 1);
assert.match(JSON.parse(tooLarge.bytes).error, /25 MiB/);
const native = JSON.parse(await readFile(new URL('results/native.json', import.meta.url), 'utf8'));
const observedBytes = await readFile(new URL('../../examples/http-rates/http-rate-observed.json', import.meta.url));
const observed = JSON.parse(observedBytes).runs;
const clientCounts = ['good', 'good-2', 'failed'].map(label => {
  const responses = observed[`http-rate-${label}.log`].responses;
  const counts = new Map();
  for (const response of responses) {
    const key = JSON.stringify([response.route, response.status]);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  assert.equal(responses.length, 2000);
  return counts;
});
for (const value of native.result.watched_fields[0].values) {
  const key = JSON.stringify([JSON.parse(value.group_values_json[0]), JSON.parse(value.value_json)]);
  assert.deepEqual(value.baseline_counts, clientCounts.slice(0, 2).map(counts => counts.get(key) ?? 0));
  assert.equal(value.target_count, clientCounts[2].get(key) ?? 0);
}
const normalize = value => JSON.parse(JSON.stringify(value, (_, n) => typeof n === 'number' && !Number.isInteger(n) ? Number(n.toPrecision(12)) : n));
const runs = [];
const rejectedNative = [];
for (const file of ['bad-second-crc.gz', 'bad-last-size.gz', 'truncated-last-member.gz', 'trailing-junk.gz']) {
  const cli = spawnSync(resolve(binary), ['diff', 'good.single.gz', '--target', file, '--json'], {
    cwd: fileURLToPath(new URL('captures/', import.meta.url)), encoding: 'utf8', maxBuffer: 1024 * 1024,
  });
  assert.equal(cli.status, 2, cli.stderr);
  assert.equal(cli.stdout, '');
  rejectedNative.push({ file, exit: cli.status, stderr: cli.stderr, no_partial_report: true });
}
for (const kind of ['single', 'multi']) {
  const paths = ['good', 'good-2', 'failed'].map(label => fileURLToPath(new URL(`captures/${label}.${kind}.gz`, import.meta.url)));
  const request = { baselines: [], target: '', context: 0, masks: [], watch_fields: ['/http/status'], watch_by: ['/route'], watch_rate_change: 5 };
  const text = [];
  for (const path of paths) {
    const decoded = call('ld_gunzip', await readFile(path)); assert.equal(decoded.code, 0);
    text.push(decoded.bytes.toString('utf8'));
  }
  for (const [index, plain] of text.entries()) {
    const counts = new Map();
    for (const line of plain.split('\n')) {
      if (!line.startsWith('{')) continue;
      const record = JSON.parse(line);
      if (record.event !== 'request_complete') continue;
      const key = JSON.stringify([record.route, record.http.status]);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    assert.deepEqual([...counts].sort(), [...clientCounts[index]].sort());
  }
  request.baselines = text.slice(0, 2); request.target = text[2];
  const wasm = call('ld_diff', Buffer.from(JSON.stringify(request)));
  assert.equal(wasm.code, 0);
  const cli = spawnSync(resolve(binary), ['diff', ...paths.slice(0, 2), '--target', paths[2], '--watch-field', '/http/status', '--watch-by', '/route', '--watch-rate-change', '5', '--json'], { encoding: 'utf8', maxBuffer: 1024 * 1024 });
  assert.equal(cli.status, 1, cli.stderr);
  assert.deepEqual(normalize(JSON.parse(wasm.bytes)), normalize(native.result));
  assert.deepEqual(JSON.parse(cli.stdout), native.result);
  runs.push({ kind, native_exit: cli.status, complete_wasm_and_native_reports_match_frozen_native_result: true,
    baseline_totals: native.result.baseline_totals, target_total: native.result.target_total });
}
await writeFile(resolve(output), JSON.stringify({ scope: 'Real CLI binary, compiled WASM C ABI, retained GNU gzip members; no browser transport mocked here',
  wasm_sha256: sha256(module), module_bytes: module.length, cases, oversized_compressed_input_rejected: true,
  independent_client_ledger_sha256: sha256(observedBytes), captured_http_responses_checked: 6000,
  original_client_status_counts_preserved_after_each_gzip_representation: true, rejected_native: rejectedNative, runs }, null, 2) + '\n');
console.log(JSON.stringify({ captures: cases.length, reports: runs.length, wasm_sha256: sha256(module) }));
