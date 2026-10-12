// Real production workers; hold completed replies only to make cancellation order deterministic.
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
const results = [];
const good = '{"route":"/private-demo","status":200}\n';
const bad = '{"route":"/private-demo","status":500}\n';
for (const [engine, browserType] of Object.entries({ chromium, firefox })) {
  const browser = await browserType.launch();
  try {
    for (const width of [1440, 375]) {
      const context = await browser.newContext({ viewport: { width, height: 1000 }, colorScheme: width === 1440 ? 'dark' : 'light' });
      try {
        const page = await context.newPage();
        const errors = [], requests = [], external = [];
        let offline = false, offlineBlobRequests = 0;
        page.on('pageerror', e => errors.push(e.message));
        page.on('request', r => {
          if (new URL(r.url()).origin !== new URL(baseURL).origin) external.push(r.url());
          if (offline && r.url().startsWith('blob:')) offlineBlobRequests++;
          if (offline && /^https?:/.test(r.url())) requests.push({ url: r.url().replace(new URL(baseURL).origin, 'ORIGIN'), method: r.method() });
        });
        await page.addInitScript(() => {
          window.offlineCsp = []; window.offlineRejections = []; window.workerCounts = { created: 0, terminated: 0 };
          window.held = []; window.hold = false;
          addEventListener('securitypolicyviolation', e => window.offlineCsp.push(e.violatedDirective));
          addEventListener('unhandledrejection', e => window.offlineRejections.push(String(e.reason)));
          const NativeWorker = Worker;
          window.Worker = class extends NativeWorker {
            constructor(...args) { super(...args); window.workerCounts.created++; }
            terminate() { window.workerCounts.terminated++; super.terminate(); }
            set onmessage(handler) {
              super.onmessage = handler ? event => {
                if (window.hold) window.held.push(() => handler.call(this, event));
                else handler.call(this, event);
              } : null;
            }
          };
        });
        const status = page.getByRole('status');
        const compare = () => page.getByRole('button', { name: 'Compare runs', exact: true }).click();
        const exported = async () => {
          const pending = page.waitForEvent('download');
          await page.getByRole('button', { name: 'Download report', exact: true }).click();
          return JSON.parse(await readFile(await (await pending).path(), 'utf8'));
        };
        const localInputs = async () => {
          for (const [side, text] of [['baseline', good], ['target', bad]]) {
            await page.locator(`input[data-file-for="${side}"]`).setInputFiles({ name: `${side}.gz`, mimeType: 'application/gzip', buffer: gzipSync(Buffer.from(text)) });
            await expect(status).toContainText(`Opened ${side}.gz`);
          }
          await page.locator('#watch-fields').fill('/status');
          await page.locator('#watch-by').fill('/route');
        };
        await page.goto(baseURL);
        await expect(status).toContainText('Comparison complete');
        await page.evaluate(() => document.fonts.ready);
        const initial = await exported();
        offline = true;
        await context.setOffline(true);
        await page.getByRole('button', { name: 'New comparison', exact: true }).click();
        await localInputs();
        await compare();
        const reports = [], transitions = [];
        let initialAttemptStatus;
        if (phase === 'before') {
          await expect(status).toContainText(/Comparison (failed|complete)/);
          initialAttemptStatus = await status.textContent();
          if ((await status.textContent()).includes('complete')) reports.push(await exported());
          else assert(requests.length > 0);
        } else {
          await expect(status).toContainText('Comparison complete');
          reports.push(await exported());
          // Cancelling active requests must still terminate the worker. Releasing
          // their held real replies afterward must not replace the current report.
          for (const action of ['Cancel', 'edit', 'New comparison', 'Cancel']) {
            const previous = reports.at(-1);
            await page.evaluate(() => { window.hold = true; window.held = []; });
            await compare();
            await page.waitForFunction(() => window.held.length === 1);
            if (action === 'edit') await page.locator('#target').fill(bad + bad);
            else await page.getByRole('button', { name: action, exact: true }).click();
            await page.evaluate(() => { window.hold = false; window.held.forEach(release => release()); });
            if (action === 'New comparison') {
              await expect(page.locator('.summary')).toHaveCount(0);
              await localInputs();
            } else assert.deepEqual(await exported(), previous);
            await compare();
            await expect(status).toContainText('Comparison complete');
            const report = await exported();
            assert.equal(report.engine.wasm_sha256, initial.engine.wasm_sha256);
            assert.equal(report.result.watched_fields[0].complete, true);
            assert(report.result.watched_fields[0].values.some(value => value.is_new));
            reports.push(report);
            transitions.push(action);
          }
          assert.equal(requests.length, 0, 'offline restarts must not need another network request');
          assert.deepEqual(reports[0].result, reports.at(-1).result);
          await compare();
          await expect(status).toContainText('Comparison complete');
        }
        const finalStatus = await status.textContent();
        await status.scrollIntoViewIfNeeded();
        const screenshot = `${engine}-${width}-${phase}.png`;
        await page.screenshot({ path: resolve(output, screenshot) });
        const accessibility = (await new AxeBuilder({ page }).analyze()).violations.map(v => v.id);
        assert.deepEqual(accessibility, []);
        assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
        const stored = await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage }, url: location.href }));
        assert(!JSON.stringify(stored).includes('private-demo'));
        const workers = await page.evaluate(() => window.workerCounts);
        if (phase === 'after') { assert.equal(workers.created, 6); assert.equal(workers.terminated, 5); }
        assert.deepEqual(errors, []); assert.deepEqual(external, []);
        assert.deepEqual(await page.evaluate(() => window.offlineCsp), []);
        assert.deepEqual(await page.evaluate(() => window.offlineRejections), []);
        results.push({ engine, version: browser.version(), width, phase, initial, reports, transitions,
          finalStatus, initialAttemptStatus, workers, offlineRequests: requests, offlineBlobRequests, accessibility, screenshot,
          screenshotSha256: createHash('sha256').update(await readFile(resolve(output, screenshot))).digest('hex') });
      } finally { await context.close(); }
    }
  } finally { await browser.close(); }
}
await writeFile(resolve(output, 'browser.json'), JSON.stringify({ synthetic: true, inputs: { good, bad }, results }, null, 2) + '\n');
