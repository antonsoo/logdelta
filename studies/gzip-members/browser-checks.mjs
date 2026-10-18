// Shared by the retained four-viewport replay and the production browser suite.
// The scheduler holds real worker replies; it never substitutes decoder output.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { expect } from '../../web/node_modules/@playwright/test/index.mjs';

const capture = name => fileURLToPath(new URL(`captures/${name}`, import.meta.url));
export async function controlGzipReplies(page) {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.heldGzipReplies = [];
    window.gzipTerminations = 0;
    window.Worker = class extends NativeWorker {
      postMessage(message, ...rest) {
        this.isGzip = 'gzip' in message;
        super.postMessage(message, ...rest);
      }
      set onmessage(handler) {
        super.onmessage = handler ? event => {
          if (this.isGzip && window.holdGzipReplies) window.heldGzipReplies.push(() => handler.call(this, event));
          else handler.call(this, event);
        } : null;
      }
      terminate() {
        if (this.isGzip) window.gzipTerminations++;
        super.terminate();
      }
    };
  });
}
export async function upload(page, side, name, rejected = false) {
  await page.locator(`input[data-file-for="${side}"]`).setInputFiles(capture(name));
  await expect(page.locator('#workspace-status')).toContainText(rejected ? 'Previous input kept' : `Opened ${name}`);
}
export async function configureRates(page) {
  const options = page.locator('.options');
  if (await options.getAttribute('open') === null) await options.locator('summary').first().click();
  await page.locator('#context').fill('0');
  await page.locator('#watch-fields').fill('/http/status');
  await page.locator('#watch-by').fill('/route');
  await page.locator('#watch-rates').check();
  await page.locator('#watch-rate-change').fill('5');
}
export async function guardImports(page, download, applied) {
  const before = await page.locator('#target').inputValue(), rejected = [];
  for (const name of ['bad-second-crc.gz', 'bad-last-size.gz', 'truncated-last-member.gz', 'trailing-junk.gz', 'expanded-over-limit.multi.gz', 'decoded-over-limit.multi.gz']) {
    await upload(page, 'target', name, true);
    const status = await page.locator('#workspace-status').textContent();
    assert.equal(await page.locator('#target').inputValue(), before);
    assert.deepEqual(await download(`rejected-${name}`), applied);
    rejected.push({ file: name, status, previous_input_and_complete_report_preserved: true });
  }
  const text = '{"message":"café 🔥", "status":503}\n';
  const encodings = [];
  for (const name of ['utf8-split.multi.gz', 'utf16-split-bom.multi.gz', 'utf16-split-unit.multi.gz']) {
    await upload(page, 'target', name);
    assert.equal(await page.locator('#target').inputValue(), text);
    await expect(page.locator('#target-source')).not.toContainText('exact field watches unavailable');
    encodings.push({ file: name, text_preserved: true, decoding_loss: false });
  }
  await upload(page, 'target', 'malformed-utf8.multi.gz');
  assert.equal(await page.locator('#target').inputValue(), 'a\uFFFD\n');
  await expect(page.locator('#target-source')).toContainText('exact field watches unavailable');
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Exact field watches require valid text');
  assert.deepEqual(await download('blocked-exact'), applied);
  await page.locator('#watch-fields').fill(''); await page.locator('#watch-by').fill(''); await page.locator('#watch-rates').uncheck();
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  const lossy = await download('lossy-exploration');
  assert.equal(lossy.sources.target.decoding_loss, 'UTF-8');
  await upload(page, 'target', 'literal-replacement.multi.gz');
  assert.equal(await page.locator('#target').inputValue(), 'a\uFFFD\n');
  await expect(page.locator('#target-source')).not.toContainText('exact field watches unavailable');
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  assert.equal((await download('literal-replacement')).sources.target.decoding_loss, undefined);
  return { rejected, encodings, malformed_utf8_retains_provenance_and_blocks_exact_watches: true, literal_replacement_is_valid: true };
}
export async function allMulti(page, download, expected) {
  await page.locator('#new-comparison').click();
  await upload(page, 'baseline', 'good.multi.gz');
  await page.locator('[data-add-baseline]').click();
  await upload(page, 'baseline', 'good-2.multi.gz');
  await upload(page, 'target', 'failed.multi.gz');
  await configureRates(page);
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  const report = await download('all-multi');
  const normalize = value => JSON.parse(JSON.stringify(value, (_, n) => typeof n === 'number' && !Number.isInteger(n) ? Number(n.toPrecision(12)) : n));
  assert.deepEqual(normalize(report.result), normalize(expected));
  assert.deepEqual(normalize(await download('all-multi-raw', 'Download JSON')), normalize(expected));
  assert.deepEqual(report.sources.baselines.map(s => s.name), ['good.multi.gz', 'good-2.multi.gz']);
  assert.equal(report.sources.target.name, 'failed.multi.gz');
  return report;
}
export async function ownership(page, download, applied) {
  const previous = await page.locator('#target').inputValue();
  await page.evaluate(() => { window.holdGzipReplies = true; window.heldGzipReplies = []; window.gzipTerminations = 0; });
  await page.locator('input[data-file-for="baseline"]').setInputFiles(capture('good.multi.gz'));
  await page.waitForFunction(() => window.heldGzipReplies.length === 1);
  await page.locator('input[data-file-for="target"]').setInputFiles(capture('failed.multi.gz'));
  await page.waitForFunction(() => window.heldGzipReplies.length === 2);
  await page.evaluate(() => window.heldGzipReplies[0]());
  await expect(page.locator('#baseline-source')).toHaveText('good.multi.gz');
  assert.equal(await page.locator('#baseline').inputValue(), await readFile(new URL('../../examples/http-rates/http-rate-good.log', import.meta.url), 'utf8'));
  await expect(page.locator('#target-source')).toContainText('Opening');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.evaluate(() => { window.holdGzipReplies = false; window.heldGzipReplies[1](); });
  assert.equal(await page.locator('#target').inputValue(), previous);
  assert.deepEqual(await download('cancelled-target'), applied);
  assert.equal(await page.evaluate(() => window.gzipTerminations), 2);

  await page.evaluate(() => { window.holdGzipReplies = true; window.heldGzipReplies = []; });
  await page.locator('input[data-file-for="target"]').setInputFiles(capture('good-2.multi.gz'));
  await page.waitForFunction(() => window.heldGzipReplies.length === 1);
  await page.locator('#target').fill('typed after import began\n');
  await page.evaluate(() => { window.holdGzipReplies = false; window.heldGzipReplies[0](); });
  assert.equal(await page.locator('#target').inputValue(), 'typed after import began\n');
  assert.deepEqual(await download('superseded-import'), applied);

  await page.evaluate(() => { window.holdGzipReplies = true; window.heldGzipReplies = []; });
  await page.locator('input[data-file-for="target"]').setInputFiles(capture('failed.multi.gz'));
  await page.waitForFunction(() => window.heldGzipReplies.length === 1);
  await page.locator('#new-comparison').click();
  await page.evaluate(() => { window.holdGzipReplies = false; window.heldGzipReplies[0](); });
  assert.equal(await page.locator('#target').inputValue(), '');
  await expect(page.locator('.summary')).toHaveCount(0);
  assert.equal(await page.evaluate(() => window.gzipTerminations), 4);
  await page.locator('#baseline').fill('job finished\n'); await page.locator('#target').fill('job failed\n');
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  assert.equal((await download('recovery')).engine.wasm_sha256, applied.engine.wasm_sha256);
  return { independent_baseline_and_target: true, cancelled_target_preserved: true, typing_and_reset_ignore_queued_replies: true,
    import_workers_terminated: 4, offline_comparison_recovery_uses_same_module: true };
}
