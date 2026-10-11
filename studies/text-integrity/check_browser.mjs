// Compare actual file imports with production WASM, offline after worker load.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { gzipSync } from 'node:zlib';

const require = createRequire(new URL('../../web/package.json', import.meta.url));
const { chromium, firefox } = require('playwright');
const { expect } = require('@playwright/test');
const AxeBuilder = require('@axe-core/playwright').default;
const [baseURL, phase, output] = process.argv.slice(2);
assert(['before', 'after'].includes(phase));
await mkdir(output, { recursive: true });
const literal = Buffer.from('{"route":"caf\uFFFD"}\n');
const malformed = {
  'UTF-8': Buffer.concat([Buffer.from('{"route":"caf'), Buffer.from([255]), Buffer.from('"}\n')]),
  'UTF-16LE': Buffer.concat([Buffer.from([255, 254]), Buffer.from('{"route":"caf\ud800"}\n', 'utf16le')]),
  'UTF-16BE': Buffer.concat([Buffer.from([255, 254]), Buffer.from('{"route":"caf\ud800"}\n', 'utf16le')]).swap16(),
};
const results = [];
for (const [engine, browserType] of Object.entries(phase === 'before' ? { chromium } : { chromium, firefox })) {
  const browser = await browserType.launch();
  try {
    for (const width of phase === 'before' ? [1440] : [1440, 375]) {
      const context = await browser.newContext({ viewport: { width, height: 1000 }, colorScheme: width === 1440 ? 'dark' : 'light' });
      try {
        const page = await context.newPage();
        const errors = [], external = [];
        page.on('pageerror', e => errors.push(e.message));
        page.on('request', r => { if (new URL(r.url()).origin !== new URL(baseURL).origin) external.push(r.url()); });
        await page.addInitScript(() => {
          window.textCsp = [];
          addEventListener('securitypolicyviolation', e => window.textCsp.push(e.violatedDirective));
        });
        await page.goto(baseURL);
        await expect(page.getByRole('status')).toContainText('Comparison complete');
        await page.evaluate(() => document.fonts.ready);
        const downloadReport = async () => {
          const promise = page.waitForEvent('download');
          await page.getByRole('button', { name: 'Download report', exact: true }).click();
          return JSON.parse(await readFile(await (await promise).path(), 'utf8'));
        };
        const open = async (side, name, buffer) => {
          await page.locator(`input[data-file-for="${side}"]`).setInputFiles({ name, mimeType: 'application/octet-stream', buffer });
          await expect(page.getByRole('status')).toContainText(`Opened ${name}`);
        };
        const compare = async () => page.getByRole('button', { name: 'Compare runs', exact: true }).click();
        for (const [encoding, bytes] of Object.entries(phase === 'before' ? { 'UTF-8': malformed['UTF-8'] } : malformed)) {
          await context.setOffline(false);
          await page.getByRole('button', { name: 'New comparison', exact: true }).click();
          await open('baseline', 'valid.log', literal);
          await open('target', 'valid-start.log', literal);
          await compare();
          await expect(page.getByRole('status')).toContainText('Comparison complete');
          // This study checks reuse of an already-loaded engine. New comparison
          // destroys the worker and needs separate offline lifecycle coverage.
          await context.setOffline(true);
          await open('target', 'damaged.gz', gzipSync(bytes));
          await compare();
          await expect(page.getByRole('status')).toContainText('Comparison complete');
          const ordinary = await downloadReport();
          if (phase === 'after') assert.equal(ordinary.sources.target.decoding_loss, encoding);
          if (phase === 'after') await page.locator('#target').fill('{"route":"caf\uFFFD"}\n# edited comment\n');
          await page.locator('#watch-fields').fill('/route');
          await compare();
          let misleading;
          if (phase === 'before') {
            await expect(page.getByRole('status')).toContainText('Comparison complete');
            const exact = await downloadReport();
            assert.equal(exact.result.watched_fields[0].complete, true);
            assert(exact.result.watched_fields[0].values.every(v => !v.is_new));
            assert.deepEqual(exact.result.findings, []);
            assert.deepEqual(exact.result.value_findings, []);
            misleading = exact;
          } else {
            await expect(page.getByRole('status')).toContainText(`contained invalid ${encoding}`);
            await expect(page.getByRole('status')).toContainText('Exact field watches require valid text');
            await expect(page.locator('#report-status')).toContainText('Inputs changed');
          }
          const status = await page.getByRole('status').textContent();
          await page.getByRole('status').scrollIntoViewIfNeeded();
          let screenshot = null, screenshotSha256 = null;
          if (encoding === 'UTF-8') {
            screenshot = `${engine}-${width}-${phase}.png`;
            await page.screenshot({ path: resolve(output, screenshot) });
            screenshotSha256 = createHash('sha256').update(await readFile(resolve(output, screenshot))).digest('hex');
          }
          const accessibility = (await new AxeBuilder({ page }).analyze()).violations.map(v => v.id);
          assert.deepEqual(accessibility, []);
          assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
          let recovered;
          if (phase === 'after') {
            assert.deepEqual(await downloadReport(), ordinary);
            await open('target', 'valid-again.log', literal);
            await compare();
            await expect(page.getByRole('status')).toContainText('Comparison complete');
            recovered = await downloadReport();
            assert.equal(recovered.sources.target.decoding_loss, undefined);
            assert.equal(recovered.result.watched_fields[0].complete, true);
            assert.equal(recovered.result.watched_fields[0].values[0].value_json, '"caf\uFFFD"');
            await open('baseline', 'damaged-baseline.gz', gzipSync(bytes));
            await compare();
            await expect(page.getByRole('status')).toContainText(`Baseline 1 (damaged-baseline.gz) contained invalid ${encoding}`);
            assert.deepEqual(await downloadReport(), recovered);
          }
          results.push({ engine, browserVersion: browser.version(), width, encoding, phase, ordinary, misleadingExactWatch: misleading,
            exactWatchBlocked: phase === 'after', restoredLiteralWatch: recovered, screenshot, screenshotSha256,
            offlineAfterWorkerLoad: true, status, accessibility });
        }
        assert.deepEqual(errors, []);
        assert.deepEqual(external, []);
        assert.deepEqual(await page.evaluate(() => window.textCsp), []);
      } finally { await context.close(); }
    }
  } finally { await browser.close(); }
}
await writeFile(resolve(output, 'browser.json'), JSON.stringify(results, null, 2) + '\n');
