// Inspect actual cmark-gfm output in offline browsers; no app or hosted-site changes.
// node browser.mjs BEFORE_CAPTURE AFTER_CAPTURE OUTPUT_DIRECTORY
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { chromium, firefox } = createRequire(join(root, 'web/package.json'))('@playwright/test');
const [before, after, out] = process.argv.slice(2).map(path => resolve(path));
await mkdir(out, { recursive: true });
const sha = text => createHash('sha256').update(text).digest('hex');
const styles = `body{font:16px/1.5 system-ui,sans-serif;color:#172033;background:#fff;margin:28px auto;padding:0 20px;max-width:1050px;overflow-wrap:anywhere}
table{border-collapse:collapse;display:block;overflow:auto;margin:20px 0}td,th{border:1px solid #cbd2dc;padding:8px 12px;text-align:left}th{background:#f0f3f7;white-space:nowrap}code{font:13px/1.5 ui-monospace,monospace;background:#eff2f6;border-radius:3px;padding:2px 4px}pre{overflow:auto;background:#eff2f6;padding:12px}h3{font-size:24px}h4{font-size:18px}`;
const results = [];
for (const [engine, browserType] of [['chromium', chromium], ['firefox', firefox]]) {
  const browser = await browserType.launch();
  try {
    for (const width of [1440, 375]) {
      const context = await browser.newContext({ viewport: { width, height: 950 }, offline: true });
      await context.route('**/*', route => route.abort());
      const page = await context.newPage();
      let attempted = [];
      page.on('request', request => attempted.push(request.url()));
      for (const [phase, dir] of [['before', before], ['after', after]]) {
        const receipt = JSON.parse(await readFile(join(dir, 'receipt.json'), 'utf8'));
        const cases = phase === 'after' ? receipt.results : receipt.results.filter(item => ['flat-image', 'field-image'].includes(item.name));
        for (const item of cases) {
          const html = await readFile(join(dir, item.name, 'report.html'), 'utf8');
          assert.equal(sha(html), item.rendered_html.sha256);
          attempted = [];
          await page.setContent(`<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Actual Logdelta Markdown report</title><style>${styles}</style></head><body>${html}</body></html>`);
          const actual = await page.evaluate(() => ({
            images: document.querySelectorAll('img').length,
            links: document.querySelectorAll('a').length,
            code_children: document.querySelectorAll('code *').length,
            code_texts: [...document.querySelectorAll('code')].map(code => code.textContent),
            overflow: document.documentElement.scrollWidth > innerWidth + 1
          }));
          assert.equal(actual.images, item.image_elements);
          assert.equal(actual.links, item.link_elements);
          assert.equal(sha(JSON.stringify(actual.code_texts)), item.code_text_sha256);
          if (phase === 'after') {
            assert.equal(actual.images + actual.links + actual.code_children, 0);
            assert.equal(attempted.length, 0);
            assert.equal(actual.overflow, false);
          } else assert.ok(attempted.length > 0, 'Baseline image markup must attempt a fetch; offline routing prevents network access.');
          if (engine === 'chromium' && width === 1440 && item.name === 'field-image') {
            await page.screenshot({ path: join(out, `${phase}.png`), fullPage: true });
          }
          results.push({ engine, width, phase, case: item.name, images: actual.images, links: actual.links,
            code_children: actual.code_children, attempted_requests: [...attempted], offline: true, overflow: actual.overflow,
            html_sha256: sha(html), code_text_sha256: sha(JSON.stringify(actual.code_texts)) });
        }
      }
      await context.close();
    }
  } finally { await browser.close(); }
}
const receipt = { scope: 'Actual reference-rendered CLI reports, offline Chromium/Firefox, desktop and 375px; generated local presentation stylesheet, not a hosted GitHub page.',
  cases: results.length, results };
await writeFile(join(out, 'receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
console.log(JSON.stringify({ cases: results.length, after: results.filter(r => r.phase === 'after').length,
  before_attempted_requests: results.filter(r => r.phase === 'before').reduce((sum, r) => sum + r.attempted_requests.length, 0),
  after_attempted_requests: results.filter(r => r.phase === 'after').reduce((sum, r) => sum + r.attempted_requests.length, 0) }));
