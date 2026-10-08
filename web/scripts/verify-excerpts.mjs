// Run against a local production preview after collecting the Kubernetes study logs.
// node scripts/verify-excerpts.mjs [http://127.0.0.1:4194/logdelta/]
/* global Event, document, innerWidth */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { chromium, firefox, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

const root = fileURLToPath(new URL("../..", import.meta.url));
const baseURL = process.argv[2] ?? "http://127.0.0.1:4194/logdelta/";
const job = "ci-kubernetes-integration-master";
const passing = "2076525647693352960";
const failing = "2076510297404739584";
const source = (build) => resolve(root, "studies/kubernetes-ci/cache", job, `${build}.log`);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const command = spawnSync(resolve(root, "target/release/logdelta"), ["diff", source(passing), source(failing), "--json"], { encoding: "utf8" });
assert.equal(command.status, 1, command.stderr);
const native = JSON.parse(command.stdout);
function compareEvidence(actual, expected, path = "result", scoreDifferences = []) {
  if (path.endsWith(".score")) {
    // Native libm and WASM can round logarithms a few ULPs apart. Counts,
    // source records, ordering, membership and every other field remain exact.
    assert.equal(typeof actual, "number", path);
    assert.equal(Number.isFinite(actual), true, path);
    const difference = Math.abs(actual - expected);
    assert.ok(difference <= 1e-12 * Math.max(1, Math.abs(expected)), path);
    if (difference) scoreDifferences.push({ path, difference });
  } else if (expected !== null && typeof expected === "object") {
    assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), path);
    for (const key of Object.keys(expected)) compareEvidence(actual[key], expected[key], `${path}.${key}`, scoreDifferences);
  } else assert.equal(actual, expected, path);
  return scoreDifferences;
}
const records = [];
for (const [name, browserType] of Object.entries({ chromium, firefox })) {
  const browser = await browserType.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1080 }, colorScheme: "light" });
    const page = await context.newPage();
    const errors = [];
    const external = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (new URL(request.url()).origin !== new URL(baseURL).origin) external.push(request.url());
    });
    await page.goto(baseURL);
    await expect(page.getByRole("status")).toContainText("Comparison complete");
    await page.getByRole("button", { name: "New comparison", exact: true }).click();
    await page.locator('input[data-file-for="baseline"]').setInputFiles(source(passing));
    await expect(page.getByRole("status")).toContainText(`Opened ${passing}.log`);
    await page.locator('input[data-file-for="target"]').setInputFiles(source(failing));
    await expect(page.getByRole("status")).toContainText(`Opened ${failing}.log`);
    await page.locator("#context").evaluate((el) => {
      el.value = "0";
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await page.getByRole("button", { name: "Compare runs", exact: true }).click();
    await expect(page.getByRole("status")).toContainText("Comparison complete");
    const block = page.locator(".is-block.kind-new").first();
    const assertion = block.locator(".log-line").filter({ hasText: "versioning_test.go:250: context deadline exceeded" });
    await expect(assertion).toHaveCount(1);
    await expect(assertion.locator(".gutter")).toHaveText("118");
    const preview = await block.locator(".log-line .gutter").allTextContents();
    assert.equal(preview.length, 36);
    const downloadEvent = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download report", exact: true }).click();
    const download = await downloadEvent;
    const report = JSON.parse(await readFile(await download.path(), "utf8"));
    const scoreDifferences = compareEvidence(report.result, native);
    const screenshots = [];
    for (const [view, width, height, colorScheme] of [["desktop", 1440, 1080, "light"], ["mobile", 375, 812, "dark"]]) {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme });
      await assertion.scrollIntoViewIfNeeded();
      const audit = await new AxeBuilder({ page }).analyze();
      assert.deepEqual(audit.violations, []);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      const path = `assets/excerpts-${name}-${view}.png`;
      if (view === "desktop") await block.screenshot({ path: resolve(root, "docs", path) });
      else await page.screenshot({ path: resolve(root, "docs", path) });
      screenshots.push({ path, width, height, colorScheme, axeViolations: audit.violations.length });
    }
    const expand = block.getByRole("button", { name: "Show all 38", exact: true });
    await expand.focus();
    await page.keyboard.press("Enter");
    await expect(block.locator(".log-line")).toHaveCount(38);
    await expect(block.getByRole("button", { name: "Show less", exact: true })).toBeFocused();
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    records.push({ browser: name, version: browser.version(), previewLines: preview.map(Number), assertionLine: 118, expandedLines: 38, nativeDownloadEvidenceEqual: true, scoreDifferences, screenshots, errors, external });
  } finally {
    await browser.close();
  }
}
const evidence = {
  measuredAt: new Date().toISOString(),
  job, passing, failing,
  inputSha256: { passing: sha256(await readFile(source(passing))), failing: sha256(await readFile(source(failing))) },
  nativeBinarySha256: sha256(await readFile(resolve(root, "target/release/logdelta"))),
  records,
};
await writeFile(resolve(root, "docs/verification-excerpts-browser.json"), `${JSON.stringify(evidence, null, 2)}\n`);
console.log(JSON.stringify(evidence, null, 2));
