// Open the two actual production builds, then stop serving them before comparisons.
import assert from 'node:assert/strict';
import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { extname, resolve, sep } from 'node:path';
const require = createRequire(new URL('../../web/package.json', import.meta.url));
const { chromium, firefox, expect } = require('@playwright/test');
const AxeBuilder = require('@axe-core/playwright').default;
const [beforeArg, afterArg, outputArg] = process.argv.slice(2);
assert(beforeArg && afterArg && outputArg, 'Expected BEFORE_DIST AFTER_DIST OUTPUT_DIRECTORY');
const output = resolve(outputArg); await mkdir(output, { recursive: true });
const cases = JSON.parse(await readFile(new URL('./cases.json', import.meta.url), 'utf8'));
const expected = JSON.parse(await readFile(new URL('./results/wasm.json', import.meta.url), 'utf8'));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const stable = value => JSON.parse(JSON.stringify(value, (_, value) => typeof value === 'number' && !Number.isInteger(value) ? Number(value.toPrecision(12)) : value));
const views = [], records = [];

for (const [phase, distArg] of [['before', beforeArg], ['after', afterArg]]) {
  const dist = resolve(distArg); await mkdir(resolve(output, phase), { recursive: true });
  for (const [engine, type] of Object.entries({ chromium, firefox })) {
    const browser = await type.launch();
    try {
      for (const width of [1440, 375]) {
        const served = {}, loaded = {}, pending = [], errors = [], external = [], late = [];
        let offline = false;
        const server = createServer(async (request, response) => {
          const name = new URL(request.url, 'http://localhost').pathname.replace(/^\/logdelta\//, '') || 'index.html';
          const path = resolve(dist, name);
          if (!path.startsWith(dist + sep)) { response.writeHead(403); response.end(); return; }
          try {
            const bytes = await readFile(path); served[name] = { bytes: bytes.length, sha256: sha(bytes) };
            response.writeHead(200, { 'Content-Type': ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.woff2': 'font/woff2' })[extname(path)] ?? 'application/octet-stream', 'Cache-Control': 'no-store' });
            response.end(bytes);
          } catch { response.writeHead(404); response.end(); }
        });
        await new Promise(done => server.listen(0, '127.0.0.1', done));
        const origin = `http://127.0.0.1:${server.address().port}`;
        const context = await browser.newContext({ viewport: { width, height: 1000 } });
        try {
          const page = await context.newPage();
          await page.addInitScript(() => {
            window.policyViolations = [];
            document.addEventListener('securitypolicyviolation', event => window.policyViolations.push(event.violatedDirective));
          });
          page.on('pageerror', error => errors.push(error.message));
          page.on('request', request => {
            if (!/^https?:/.test(request.url())) return;
            if (new URL(request.url()).origin !== origin) external.push(request.url());
            if (offline) late.push(request.url());
          });
          page.on('response', response => {
            if (!response.url().startsWith(origin)) return;
            pending.push((async () => {
              const name = new URL(response.url()).pathname.replace(/^\/logdelta\//, '') || 'index.html';
              const bytes = await response.body(); assert.equal(sha(bytes), sha(await readFile(resolve(dist, name))));
              loaded[name] = { bytes: bytes.length, sha256: sha(bytes) };
            })());
          });
          await page.goto(`${origin}/logdelta/`);
          await expect(page.getByRole('status')).toContainText('Comparison complete');
          await page.evaluate(async () => { await document.fonts.ready; await Promise.all([...document.fonts].map(font => font.load())); });
          await Promise.all(pending);
          await context.setOffline(true); offline = true;
          server.closeAllConnections(); await new Promise(done => server.close(done));
          await page.locator('#new-comparison').click();
          await page.locator('.options > summary').click(); await page.locator('#context').fill('0');
          await page.locator('.options > summary').click();
          for (const item of cases) {
            for (const [role, text] of [['baseline', item.baseline], ['target', item.target]]) {
              const name = `${item.id}-${role}.log`;
              await page.locator(`input[data-file-for="${role}"]`).setInputFiles({
                name, mimeType: 'text/plain', buffer: Buffer.from((text + '\n').repeat(12)),
              });
              await expect(page.locator(`#${role}-source`)).toContainText(name);
              await expect(page.getByRole('status')).toContainText('Opened');
              assert.equal(await page.locator(`#${role}`).inputValue(), (text + '\n').repeat(12));
            }
            await page.locator('#run').click();
            await expect(page.getByRole('status')).toContainText('Comparison complete');
            const event = page.waitForEvent('download');
            await page.getByRole('button', { name: 'Download report', exact: true }).click();
            const download = await event;
            const exported = JSON.parse(await readFile(await download.path(), 'utf8'));
            const report = exported.result;
            const agrees = item.control ? report.findings.length === 0 : report.findings.some(finding => finding.kind === 'new');
            if (phase === 'after') {
              assert(agrees, item.id);
              assert.deepEqual(stable(report), stable(expected.records.find(record => record.id === item.id).report), item.id);
            }
            if (item.id === 'message') {
              if (phase === 'after') await expect(page.locator('#report-content')).toContainText('corruption');
              await page.screenshot({ path: resolve(output, phase, `${engine}-${width}.png`), fullPage: true });
            }
            assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
            records.push({ phase, engine, width, id: item.id, control: !!item.control, agrees,
              report, downloadedReportMatchesNative: phase === 'after' });
          }
          const violations = (await new AxeBuilder({ page }).analyze()).violations.map(row => row.id);
          assert.deepEqual(violations, []); assert.deepEqual(errors, []); assert.deepEqual(external, []); assert.deepEqual(late, []);
          const policy = await page.evaluate(() => window.policyViolations); assert.deepEqual(policy, []);
          await Promise.all(pending);
          views.push({ phase, engine, browser: browser.version(), width, offlineAfterInitialLoad: true,
            served, loaded, pageErrors: errors, externalRequests: external, lateRequests: late,
            policyViolations: policy, axeViolations: violations, overflow: false });
        } finally { server.closeAllConnections(); server.close(); await context.close(); }
      }
    } finally { await browser.close(); }
  }
}
const result = { node: process.version, workflows: records.length,
  beforeDisagreements: records.filter(record => record.phase === 'before' && !record.agrees).length,
  afterDisagreements: records.filter(record => record.phase === 'after' && !record.agrees).length,
  precision: 'Integers/text/source evidence exact, floating-point scores at 12 significant digits.', records, views };
await writeFile(resolve(output, 'observations.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify({ workflows: result.workflows, beforeDisagreements: result.beforeDisagreements,
  afterDisagreements: result.afterDisagreements, views: views.length }));
