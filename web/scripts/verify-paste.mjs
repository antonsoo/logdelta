/* global document, navigator */
// Distinguish actual clipboard paste from Playwright fill()/Input.insertText.
// node scripts/verify-paste.mjs <URL> <output.json>
import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { TextEncoder } from "node:util";
import { chromium } from "@playwright/test";
import { writeFile } from "node:fs/promises";
const [url = "http://127.0.0.1:4341/logdelta/", output = "../docs/field-rate-evidence/browser/paste.json"] = process.argv.slice(2);
const keys = Array.from({ length: 64 }, (_, i) => i === 0 ? '<img src=x onerror="alert(1)">' : `/route/${String(i).padStart(3, "0")}`);
const text = keys.flatMap((route) => Array.from({ length: 100 }, (_, i) => JSON.stringify({ route, status: i === 99 ? 503 : 200 }))).join("\n");
const browser = await chromium.launch();
const context = await browser.newContext({ permissions: ["clipboard-read", "clipboard-write"] });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.goto(url);
await page.locator("#workspace-status").filter({ hasText: "Comparison complete" }).waitFor();
await page.locator("#new-comparison").click();
await page.evaluate(async (value) => navigator.clipboard.writeText(value), text);
await page.locator("#baseline").focus();
const operations = [];
for (const [shortcut, expected] of [["Control+V", text], ["Control+Z", ""], ["Control+Shift+Z", text]]) {
  const started = performance.now();
  await page.keyboard.press(shortcut);
  await page.waitForFunction((value) => document.querySelector("#baseline").value === value, expected);
  assert.equal(await page.locator("#baseline").inputValue(), expected);
  operations.push({ shortcut, milliseconds: performance.now() - started, exactContents: true, lines: expected ? 6400 : 0 });
}
assert.equal(await page.locator("#baseline-count").innerText(), "6,400 lines");
assert.deepEqual(errors, []);
const result = { checkedAt: new Date().toISOString(), browser: browser.version(), url, utf8Bytes: new TextEncoder().encode(text).length, operations, runtimeErrors: errors };
await writeFile(output, JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify(result));
await browser.close();
