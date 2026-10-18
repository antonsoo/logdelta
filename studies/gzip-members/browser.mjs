// Compare native single/member gzip captures in the actual app, offline after warm-up.
import { chromium, firefox, expect } from '../../web/node_modules/@playwright/test/index.mjs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { allMulti, controlGzipReplies, guardImports, ownership } from './browser-checks.mjs';

const [url, destination, phase = 'after'] = process.argv.slice(2);
assert(url && destination && ['before', 'after'].includes(phase), 'Use browser.mjs URL OUTPUT before|after');
const output = resolve(destination), captures = fileURLToPath(new URL('captures/', import.meta.url));
await mkdir(output, { recursive: true });
const native = JSON.parse(await readFile(new URL('results/native.json', import.meta.url), 'utf8')).result;
const stable = value => JSON.parse(JSON.stringify(value, (_, n) => typeof n === 'number' && !Number.isInteger(n) ? Number(n.toPrecision(12)) : n));
const records = [];
for (const [name, engine] of Object.entries({ chromium, firefox })) {
  const browser = await engine.launch({ headless: true });
  for (const width of [1440, 375]) {
    const context = await browser.newContext({ viewport: { width, height: 1000 }, acceptDownloads: true });
    const page = await context.newPage(), errors = [], requests = [], downloads = [];
    await page.addInitScript(() => {
      window.gzipDiagnostics = { csp: [], rejections: [] };
      document.addEventListener('securitypolicyviolation', event => window.gzipDiagnostics.csp.push(event.violatedDirective));
      window.addEventListener('unhandledrejection', event => window.gzipDiagnostics.rejections.push(String(event.reason)));
    });
    if (phase === 'after') await controlGzipReplies(page);
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(url);
    await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
    await page.evaluate(() => document.fonts.ready);
    await context.setOffline(true);
    page.on('request', request => { if (/^https?:/.test(request.url())) requests.push(request.url()); });
    await page.locator('#new-comparison').click();
    const open = async (editor, file, expected = 'opened') => {
      await page.locator(`input[data-file-for="${editor}"]`).setInputFiles(join(captures, file));
      await expect(page.locator('#workspace-status')).toContainText(expected === 'opened' ? `Opened ${file}.` : 'Previous input kept');
    };
    const download = async (suffix, button = 'Download report') => {
      const pending = page.waitForEvent('download');
      await page.getByRole('button', { name: button, exact: true }).click();
      const path = join(output, `${name}-${width}-${suffix}.json`);
      await (await pending).saveAs(path);
      const bytes = await readFile(path);
      downloads.push({ file: `${name}-${width}-${suffix}.json`, kind: button, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
      return JSON.parse(bytes);
    };
    await open('baseline', 'good.single.gz');
    await page.locator('[data-add-baseline]').click();
    await open('baseline', 'good-2.single.gz');
    await open('target', 'good.single.gz');
    const options = page.locator('.options');
    if (await options.getAttribute('open') === null) await options.locator('summary').first().click();
    await page.locator('#context').fill('0');
    await page.locator('#watch-fields').fill('/http/status');
    await page.locator('#watch-by').fill('/route');
    await page.locator('#watch-rates').check();
    await page.locator('#watch-rate-change').fill('5');
    await page.locator('#run').click();
    await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
    const previous = await download('previous');
    const previousInput = await page.locator('#target').inputValue();
    await open('target', 'failed.multi.gz', phase === 'before' ? 'rejected' : 'opened');
    const importStatus = await page.locator('#workspace-status').textContent();
    let guards, concurrency;
    if (phase === 'before') {
      assert.equal(await page.locator('#target').inputValue(), previousInput);
      await page.screenshot({ path: join(output, `${name}-${width}.png`), fullPage: true });
      assert.deepEqual(await download('after-rejected-import'), previous);
    } else {
      const plaintext = await readFile(new URL('../../examples/http-rates/http-rate-failed.log', import.meta.url), 'utf8');
      assert.equal(await page.locator('#target').inputValue(), plaintext);
      await page.locator('#run').click();
      await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
      const applied = await download('multi');
      assert.deepEqual(stable(applied.result), stable(native));
      assert.equal(applied.result.target_total, 2002);
      await expect(page.locator('.rate-headline')).toContainText('20% in target');
      await expect(page.locator('.rate-values tbody tr').first()).toHaveText('50310 / 1,000 (1%)12 / 1,000 (1.2%)200 / 1,000 (20%)+18.8 pp287.023');
      await page.screenshot({ path: join(output, `${name}-${width}.png`), fullPage: true });
      guards = await guardImports(page, download, applied);
      const multiReport = await allMulti(page, download, native);
      concurrency = await ownership(page, download, multiReport);
    }
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
    assert.deepEqual(errors, []); assert.deepEqual(requests, []);
    const diagnostics = await page.evaluate(() => window.gzipDiagnostics);
    assert.deepEqual(diagnostics, { csp: [], rejections: [] });
    const gzip = await readFile(join(captures, 'failed.multi.gz'));
    records.push({ browser: name, version: browser.version(), width, phase,
      capture_sha256: createHash('sha256').update(gzip).digest('hex'),
      member_file: 'failed.multi.gz', opened: phase === 'after', previous_input_preserved_on_failure: phase === 'before',
      complete_native_result_matches: phase === 'after', offline: true, errors, requests,
      guards, concurrency, downloads, diagnostics,
      import_status: importStatus, status: await page.locator('#workspace-status').textContent() });
    await context.close();
  }
  await browser.close();
}
await writeFile(join(output, 'browser.json'), JSON.stringify({ workflows: records }, null, 2) + '\n');
console.log(JSON.stringify({ workflows: records.length, phase }));
