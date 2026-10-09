// Run against a local production preview after cargo build --release and npm run build.
import { chromium, firefox } from "@playwright/test";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import process from "node:process";
import { Buffer } from "node:buffer";

const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const url = process.argv[2] ?? "http://127.0.0.1:4197/logdelta/";
const scratch = await mkdtemp(join(process.env.LOGDELTA_VERIFY_SCRATCH ?? tmpdir(), "logdelta-ledger-"));
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const log = (value, padding = 3900) => Array.from({ length: 128 }, (_, i) => JSON.stringify({ route: `/api/endpoint/${String(i).padStart(3, "0")}/` + "x".repeat(padding), ...Object.fromEntries(Array.from({ length: 16 }, (_, n) => [`status${n}`, value])) }) + "\n").join("");
const watches = Array.from({ length: 16 }, (_, n) => `/status${n}`);
const good = log(200), target = log(503);
const evidence = [];
await mkdir(`${root}/docs/assets/field-ledger`, { recursive: true });
try {
  await writeFile(`${scratch}/good.log`, good);
  await writeFile(`${scratch}/target.log`, target);
  const native = spawnSync(`${root}/target/release/logdelta`, ["diff", `${scratch}/good.log`, "--target", `${scratch}/target.log`, "--json", "-C", "2", "--watch-by", "/route", ...watches.flatMap((w) => ["--watch-field", w])], { encoding: "utf8", maxBuffer: 128 * 1024 * 1024 });
  assert.equal(native.status, 1, native.stderr);
  const reference = JSON.parse(native.stdout);
  for (const [name, browserType] of [["chromium", chromium], ["firefox", firefox]]) {
    const browser = await browserType.launch();
    try {
      const page = await browser.newPage({ viewport: { width: 1440, height: 1100 }, colorScheme: "light" });
      const errors = [], external = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("request", (request) => { if (new URL(request.url()).origin !== new URL(url).origin) external.push(request.url()); });
      await page.goto(url);
      await page.getByRole("status").filter({ hasText: "Comparison complete" }).waitFor();
      await page.getByRole("button", { name: "New comparison", exact: true }).click();
      await page.locator("#baseline").fill(good);
      await page.locator("#target").fill(target);
      await page.locator("#watch-fields").fill(watches.join("\n"));
      await page.locator("#watch-by").fill("/route");
      await page.evaluate(() => {
        globalThis.ledgerTasks = [];
        if (globalThis.PerformanceObserver.supportedEntryTypes.includes("longtask")) new globalThis.PerformanceObserver((list) => globalThis.ledgerTasks.push(...list.getEntries().map((e) => e.duration))).observe({ type: "longtask" });
      });
      const start = globalThis.performance.now();
      await page.getByRole("button", { name: "Compare runs", exact: true }).click();
      await page.getByRole("status").filter({ hasText: "Comparison complete" }).waitFor();
      const measurement = { elapsed_ms: globalThis.performance.now() - start, ...await page.evaluate(() => ({ dom_elements: globalThis.document.querySelectorAll("*").length, report_html_bytes: new globalThis.TextEncoder().encode(globalThis.document.querySelector("#report-content").innerHTML).length, long_tasks_ms: globalThis.ledgerTasks, long_tasks_supported: globalThis.PerformanceObserver.supportedEntryTypes.includes("longtask"), heap_bytes: globalThis.performance.memory?.usedJSHeapSize ?? null })) };
      assert.equal(await page.locator(".field-values").count(), 0);
      const first = page.locator(".field-evidence").first();
      await first.locator(":scope > summary").click();
      assert.equal(await first.locator(".field-values tbody tr").count(), 32);
      await first.getByRole("searchbox").fill("/127/");
      assert.equal(await first.locator(".field-values tbody tr").count(), 2);
      await first.locator(".field-sources > summary").click();
      assert.match(await first.locator(".field-source-records").innerText(), /Target, line 128/);
      const pending = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download report", exact: true }).click();
      const report = JSON.parse(await readFile(await (await pending).path(), "utf8"));
      assert.deepEqual(report.result, reference);

      // Use readable route keys for screenshots of the same controls.
      await page.locator("#baseline").fill(log(200, 0));
      await page.locator("#target").fill(log(503, 0));
      await page.locator("#watch-fields").fill("/status0");
      await page.getByRole("button", { name: "Compare runs", exact: true }).click();
      await page.getByRole("status").filter({ hasText: "Comparison complete" }).waitFor();
      await first.locator(":scope > summary").click();
      await first.getByRole("searchbox").fill("/127/");
      await first.locator(".field-sources > summary").click();
      await page.evaluate(() => globalThis.document.fonts.ready);
      const screenshots = [`docs/assets/field-ledger/${name}-search.png`];
      await first.screenshot({ path: `${root}/${screenshots[0]}` });
      if (name === "chromium") {
        await page.setViewportSize({ width: 375, height: 1000 });
        await page.locator("#theme-toggle").click();
        screenshots.push("docs/assets/field-ledger/mobile-dark.png");
        await first.screenshot({ path: `${root}/${screenshots[1]}` });
        assert.equal(await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth), true);
      }
      assert.deepEqual(errors, []);
      assert.deepEqual(external, []);
      evidence.push({ browser: name, version: browser.version(), measurement, native_result_equal_after_filtering: true, engine_sha256: report.engine.wasm_sha256, screenshots: Object.fromEntries(await Promise.all(screenshots.map(async (path) => [path, hash(await readFile(`${root}/${path}`))]))), errors, external_requests: external });
    } finally { await browser.close(); }
  }
  const result = { checked_at: new Date().toISOString(), scenario: "Controlled 128 groups, 16 watches, 3900 padding characters in each group key; not a captured production log", input_bytes: Buffer.byteLength(good + target), input_sha256: { baseline: hash(good), target: hash(target) }, native_report_bytes: Buffer.byteLength(native.stdout), native_binary_sha256: hash(await readFile(`${root}/target/release/logdelta`)), evidence };
  await writeFile(`${root}/docs/verification-field-ledger.json`, JSON.stringify(result, null, 2) + "\n");
  console.log(JSON.stringify(result, null, 2));
} finally { await rm(scratch, { recursive: true, force: true }); }
