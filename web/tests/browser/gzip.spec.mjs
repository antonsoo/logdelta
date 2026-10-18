import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { allMulti, controlGzipReplies, guardImports, ownership, upload } from '../../../studies/gzip-members/browser-checks.mjs';

const native = JSON.parse(await readFile(new URL('../../../studies/gzip-members/results/native.json', import.meta.url), 'utf8')).result;
async function report(page, button = 'Download report') {
  const pending = page.waitForEvent('download');
  await page.getByRole('button', { name: button, exact: true }).click();
  return JSON.parse(await readFile(await (await pending).path(), 'utf8'));
}

test('GNU multi-member files retain the complete HTTP report, validation gates and independent worker ownership offline', async ({ page, context }) => {
  test.setTimeout(120_000);
  const errors = [], requests = [], violations = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('gzipViolation', message => violations.push(message));
  await page.addInitScript(() => {
    document.addEventListener('securitypolicyviolation', event => window.gzipViolation(event.violatedDirective));
    window.addEventListener('unhandledrejection', event => window.gzipViolation(String(event.reason)));
  });
  await controlGzipReplies(page);
  await page.goto('./');
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  await page.evaluate(() => document.fonts.ready);
  await context.setOffline(true);
  page.on('request', request => { if (/^https?:/.test(request.url())) requests.push(request.url()); });
  const download = (_suffix, button) => report(page, button);
  const applied = await allMulti(page, download, native);
  await guardImports(page, download, applied);
  const restored = await allMulti(page, download, native);
  await ownership(page, download, restored);
  expect(errors).toEqual([]); expect(requests).toEqual([]); expect(violations).toEqual([]);
});

test('a gzip import can initialize the module after a failed initial comparison and then work offline', async ({ page, context }) => {
  await page.route('**/assets/logdelta-*.wasm', route => route.fulfill({ status: 503, body: 'unavailable' }));
  await page.goto('./');
  await expect(page.locator('#workspace-status')).toContainText('Comparison failed');
  await page.unroute('**/assets/logdelta-*.wasm');
  await page.locator('#new-comparison').click();
  const moduleResponse = page.waitForResponse(response => /\/assets\/logdelta-[^/]+\.wasm$/.test(new URL(response.url()).pathname));
  await upload(page, 'baseline', 'good.multi.gz');
  const moduleSha256 = createHash('sha256').update(await (await moduleResponse).body()).digest('hex');
  await page.evaluate(() => document.fonts.ready);
  await context.setOffline(true);
  const requests = [];
  page.on('request', request => { if (/^https?:/.test(request.url())) requests.push(request.url()); });
  await upload(page, 'target', 'failed.multi.gz');
  await page.locator('#run').click();
  await expect(page.locator('#workspace-status')).toContainText('Comparison complete');
  const applied = await report(page);
  expect(applied.result.baseline_totals).toEqual([2002]);
  expect(applied.result.target_total).toBe(2002);
  expect(applied.engine.wasm_sha256).toBe(moduleSha256);
  expect(await page.locator('#target').inputValue()).toBe(await readFile(new URL('../../../examples/http-rates/http-rate-failed.log', import.meta.url), 'utf8'));
  expect(requests).toEqual([]);
});
