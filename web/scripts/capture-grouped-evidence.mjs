// Verify actual downloaded reports against the native CLI and capture the same UI.
// Run after cargo build --release, npm run build, and starting the production preview.
import { chromium, firefox } from "@playwright/test";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const base = process.argv[2] ?? "http://127.0.0.1:4197/logdelta/";
await mkdir(`${root}/docs/assets/grouped-fields`, { recursive: true });
const native = spawnSync(`${root}/target/release/logdelta`, ["diff", "examples/http-routes-good.log", "examples/http-routes-good-2.log", "--target", "examples/http-routes-failed.log", "--watch-field", "/http/status", "--watch-by", "/route", "--json", "-C", "2"], { cwd: root, encoding: "utf8" });
assert.equal(native.status, 1, native.stderr);
const reference = JSON.parse(native.stdout);
const evidence = [];
for (const [name, browserType] of [["chromium", chromium], ["firefox", firefox]]) {
  const browser = await browserType.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, colorScheme: "light" });
    const page = await context.newPage();
    const errors = [], external = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("request", request => { if (new URL(request.url()).origin !== new URL(base).origin) external.push(request.url()); });
    await page.goto(base);
    await page.getByRole("status").filter({ hasText: "Comparison complete" }).waitFor();
    await page.locator('[data-example="http-routes"]').click();
    await page.locator(".kind-field .field-group").waitFor();
    const getReport = async () => {
      const pending = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download report", exact: true }).click();
      return JSON.parse(await readFile(await (await pending).path(), "utf8"));
    };
    const report = await getReport();
    await page.evaluate(() => globalThis.document.fonts.ready);
    assert.deepEqual(report.result, reference);
    assert.deepEqual(report.settings.watch_by, ["/route"]);
    await page.locator(".field-evidence > summary").click();
    await page.locator("#report-content").screenshot({ path: `${root}/docs/assets/grouped-fields/${name}-evidence.png` });
    const entry = { browser: name, version: browser.version(), native_result_equal: true, engine_sha256: report.engine.wasm_sha256, screenshots: [`docs/assets/grouped-fields/${name}-evidence.png`] };
    if (name === "chromium") {
      await page.locator(".field-evidence > summary").click();
      await page.setViewportSize({ width: 375, height: 1000 });
      await page.locator("#theme-toggle").click();
      await page.locator("#report-content").screenshot({ path: `${root}/docs/assets/grouped-fields/mobile-dark.png` });
      entry.screenshots.push("docs/assets/grouped-fields/mobile-dark.png");
      await page.setViewportSize({ width: 1440, height: 1050 });
      await page.locator("#theme-toggle").click();
      await page.locator(".watch-input").screenshot({ path: `${root}/docs/assets/grouped-fields/controls.png` });
      entry.screenshots.push("docs/assets/grouped-fields/controls.png");
      await page.setViewportSize({ width: 375, height: 1000 });
      await page.locator("#target").fill('{"route":"/checkout","http":{"status":200}}\n{"http":{"status":503}}');
      await page.getByRole("button", { name: "Compare runs", exact: true }).click();
      await page.getByRole("status").filter({ hasText: "Field comparison incomplete" }).waitFor();
      const incomplete = await getReport();
      assert.equal(incomplete.result.watched_fields[0].target.group_missing, 1);
      await page.locator("#report-content").screenshot({ path: `${root}/docs/assets/grouped-fields/mobile-incomplete.png` });
      entry.screenshots.push("docs/assets/grouped-fields/mobile-incomplete.png");
    }
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    entry.errors = errors;
    entry.external_requests = external;
    entry.screenshot_sha256 = {};
    for (const path of entry.screenshots) entry.screenshot_sha256[path] = createHash("sha256").update(await readFile(`${root}/${path}`)).digest("hex");
    evidence.push(entry);
  } finally { await browser.close(); }
}
await writeFile(`${root}/docs/verification-grouped-browser.json`, JSON.stringify({ checked_at: new Date().toISOString(), scope: "Local production build; controlled loopback HTTP fixture", native_binary_sha256: createHash("sha256").update(await readFile(`${root}/target/release/logdelta`)).digest("hex"), evidence }, null, 2) + "\n");
console.log(JSON.stringify(evidence, null, 2));
