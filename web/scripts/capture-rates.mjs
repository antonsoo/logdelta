/* global document, innerWidth */
// Capture actual reports and downloads from a production preview or the hosted app.
// node scripts/capture-rates.mjs <URL> <screenshot-directory> <evidence-directory>
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { chromium, firefox } from "@playwright/test";

const [url = "http://127.0.0.1:4341/logdelta/", screenshots = "../docs/assets/browser-rates", output = "../docs/field-rate-evidence/browser"] = process.argv.slice(2);
await mkdir(screenshots, { recursive: true });
await mkdir(output, { recursive: true });
const checks = [];
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const stable = (value) => JSON.parse(JSON.stringify(value, (_, n) => typeof n === "number" && !Number.isInteger(n) ? Number(n.toPrecision(12)) : n));
const native = JSON.parse(await readFile(new URL("../../docs/field-rate-evidence/native.json", import.meta.url), "utf8"));
for (const [name, engine] of [["chromium", chromium], ["firefox", firefox]]) {
  const browser = await engine.launch();
  for (const width of [1440, 375]) for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width, height: 1000 }, colorScheme: theme });
    const errors = [], external = [], wasm = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => { if (new URL(request.url()).origin !== new URL(url).origin) external.push(request.url()); });
    page.on("response", (response) => { if (new URL(response.url()).pathname.endsWith(".wasm")) wasm.push(response); });
    await page.goto(url);
    await page.locator("#workspace-status").filter({ hasText: "Comparison complete" }).waitFor();
    await page.locator('[data-example="http-rates"]').click();
    await page.locator(".kind-rate").waitFor();
    await page.locator(".rate-source > summary").first().click();
    await page.locator(".rate-source-body .is-hit").waitFor();
    assert.match(await page.locator(".rate-source-body .is-hit").innerText(), /"status":503/);
    await page.evaluate(() => document.fonts.ready);
    const screenshot = `${name}-${width}-${theme}.png`;
    await page.locator("#results").screenshot({ path: `${screenshots}/${screenshot}` });
    const viewportOverflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    assert.equal(viewportOverflow, false);
    // Compare complete exports, using the same no-context setting as the native capture.
    await page.locator(".options > summary").click();
    await page.locator("#context").fill("0");
    await page.context().setOffline(true);
    await page.locator("#run").click();
    await page.locator("#workspace-status").filter({ hasText: "Comparison complete" }).waitFor();
    const [download] = await Promise.all([page.waitForEvent("download"), page.locator("#download-report").click()]);
    const bytes = await readFile(await download.path());
    const report = JSON.parse(bytes);
    assert.equal(report.schema_version, 3);
    assert.equal(report.settings.watch_rate_change, 5);
    assert.deepEqual(stable(report.result), stable(native));
    assert.equal(wasm.length, 1);
    const engineDigest = digest(await wasm[0].body());
    assert.equal(report.engine.wasm_sha256, engineDigest);
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    checks.push({ browser: name, width, theme, screenshot, screenshotSha256: digest(await readFile(`${screenshots}/${screenshot}`)), viewportOverflow, runtimeErrors: errors, externalRequests: external, offlineComparisonAndDownload: true, nativeResultMatch: true, engineSha256: engineDigest, engineTimeMs: Number((await page.locator(".summary-meta").innerText()).match(/Compared in ([\d.]+) ms/)?.[1] ?? NaN) });
    if (name === "chromium" && width === 1440 && theme === "light") await writeFile(`${output}/report.json`, bytes);
    await page.close();
  }
  await browser.close();
}
const evidence = { checkedAt: new Date().toISOString(), url, checks };
await writeFile(`${output}/verification.json`, JSON.stringify(evidence, null, 2) + "\n");
console.log(JSON.stringify(evidence));
