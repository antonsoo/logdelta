import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFile } from "node:fs/promises";
import { TextEncoder } from "node:util";

async function ready(page) {
  await page.goto("./");
  await expect(page.getByRole("status")).toContainText("Comparison complete");
}
async function example(page) {
  await ready(page);
  await page.locator('[data-example="http"]').click();
  await expect(page.locator(".kind-field")).toHaveCount(2);
}
async function run(page, incomplete = false) {
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await expect(page.getByRole("status")).toContainText(incomplete ? "Field comparison incomplete" : "Comparison complete");
}
async function report(page, button = "Download report") {
  const promise = page.waitForEvent("download");
  await page.getByRole("button", { name: button, exact: true }).click();
  const file = await promise;
  return JSON.parse(await readFile(await file.path(), "utf8"));
}
test.beforeEach(async ({ page, baseURL }) => {
  const errors = [], external = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => { if (new URL(r.url()).origin !== new URL(baseURL).origin) external.push(r.url()); });
  await page.exposeFunction("fieldDiagnostics", () => ({ errors, external }));
  await page.addInitScript(() => {
    window.fieldViolations = [];
    document.addEventListener("securitypolicyviolation", (e) => window.fieldViolations.push(e.violatedDirective));
    window.addEventListener("unhandledrejection", (e) => window.fieldViolations.push(String(e.reason)));
  });
});
test.afterEach(async ({ page }) => {
  expect(await page.evaluate(() => window.fieldDiagnostics())).toEqual({ errors: [], external: [] });
  expect(await page.evaluate(() => window.fieldViolations)).toEqual([]);
});

test("real WASM exposes the masked incident with exact counts, sources and coherent exports", async ({ page }) => {
  await example(page);
  await expect(page.locator("#watch-fields")).toHaveValue("/http/status\n/exit_code");
  await expect(page.locator(".summary-line")).toContainText("2 findings");
  await expect(page.locator(".kind-field").first()).toContainText("503");
  await expect(page.locator(".kind-field").first()).toContainText("2 in the target");
  await page.locator(".field-evidence > summary").first().click();
  await expect(page.locator(".field-values").first().getByRole("row", { name: "200 20 20 18 Seen in baseline" })).toBeVisible();
  await expect(page.locator(".field-values").first().getByRole("row", { name: "503 0 0 2 New value" })).toBeVisible();
  await page.locator(".field-sources > summary").first().click();
  await expect(page.locator(".field-sources").first()).toContainText("Target, line 8");
  const exported = await report(page);
  expect(exported.settings.watch_fields).toEqual(["/http/status", "/exit_code"]);
  expect(exported.result).toEqual(await report(page, "Download JSON"));
  expect(exported.result.findings).toEqual([]);
  expect(exported.result.value_findings).toEqual([]);
  expect(exported.result.watched_fields[0].values[1]).toMatchObject({ value_json: "503", baseline_counts: [0, 0], target_count: 2, is_new: true, first_target: { line_no: 8 } });
  await page.locator("#watch-fields").fill("");
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  expect((await report(page)).settings.watch_fields).toEqual(["/http/status", "/exit_code"]);
  await run(page);
  await expect(page.locator(".summary-line")).toContainText("0 findings");
  expect((await report(page)).settings.watch_fields).toEqual([]);
});

test("grouping the captured HTTP requests identifies checkout while maintenance stays known", async ({ page }) => {
  await ready(page);
  await page.locator('[data-example="http-routes"]').click();
  await expect(page.locator(".kind-field")).toHaveCount(1);
  await expect(page.locator("#watch-by")).toHaveValue("/route");
  await expect(page.locator(".kind-field .field-group")).toHaveText('/route = "/checkout"');
  await expect(page.locator(".kind-field .field-known")).toHaveText("Baseline values in this group: 200. Compared before masking.");
  await expect(page.locator(".kind-field .is-hit")).toContainText('"status":503');
  await page.locator(".field-evidence > summary").click();
  const table = page.locator(".field-values");
  await expect(table.getByRole("row", { name: '"/checkout" 200 20 20 18 Seen in baseline' })).toBeVisible();
  await expect(table.getByRole("row", { name: '"/checkout" 503 0 0 2 New value' })).toBeVisible();
  await expect(table.getByRole("row", { name: '"/maintenance" 503 20 20 20 Seen in baseline' })).toBeVisible();
  const grouped = await report(page);
  expect(grouped.schema_version).toBe(2);
  expect(grouped.settings.watch_by).toEqual(["/route"]);
  expect(grouped.result).toEqual(await report(page, "Download JSON"));
  expect(grouped.result.watched_fields[0].values.find((v) => v.is_new)).toMatchObject({ group_values_json: ['"/checkout"'], group_seen_in_baseline: true, value_json: "503", baseline_counts: [0, 0], target_count: 2, first_target: { line_no: 14 } });
  await page.locator("#watch-by").fill("");
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  expect(await report(page)).toEqual(grouped);
  await run(page);
  await expect(page.locator(".summary-line")).toContainText("0 findings");
  const pooled = await report(page);
  expect(pooled.schema_version).toBe(1);
  expect(pooled.settings.watch_by).toBeUndefined();
  expect(pooled.result.watched_fields[0].group_by).toBeUndefined();
  await page.locator("#watch-by").fill("/route");
  await run(page);
  await expect(page.locator(".kind-field")).toHaveCount(1);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await expect(page.locator("#watch-by")).toHaveValue("");
});

test("missing group keys fail visibly and invalid group settings retain the completed evidence", async ({ page }) => {
  await ready(page);
  await page.locator('[data-example="http-routes"]').click();
  await expect(page.locator(".kind-field")).toHaveCount(1);
  const before = await report(page);
  await page.locator("#watch-by").fill("route");
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await expect(page.locator("#watch-by")).toBeFocused();
  await expect(page.locator("#watch-by")).toHaveAttribute("aria-invalid", "true");
  expect(await report(page)).toEqual(before);
  await page.locator("#watch-by").fill("/route");
  await page.locator("#watch-fields").fill("");
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("at least one watched field");
  await expect(page.locator("#watch-by")).toBeFocused();
  await page.locator("#watch-fields").fill("/http/status");
  await page.locator("#target").fill('{"route":"/checkout","http":{"status":200}}\n{"http":{"status":503}}');
  await run(page, true);
  await expect(page.locator(".field-coverage")).toContainText("1 missing group key");
  await expect(page.locator(".field-problem")).toContainText("Target, line 2");
  await expect(page.locator("#report-content")).not.toContainText("No findings under");
  const incomplete = (await report(page)).result.watched_fields[0];
  expect(incomplete.complete).toBe(false);
  expect(incomplete.values.every((v) => v.is_new === null && v.group_seen_in_baseline === undefined)).toBe(true);
});

test("composite groups and unseen routes retain exact keys without implying an observed regression", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await page.locator("#baseline").fill('{"service":9007199254740992,"route":"/checkout","v":200}');
  await page.locator("#target").fill('{"service":9007199254740992,"route":"/checkout","v":503}\n{"service":9007199254740993,"route":"<img src=x onerror=alert(1)>","v":200}');
  await page.locator("#watch-fields").fill("/v");
  await page.locator("#watch-by").fill("/service\n/route");
  await run(page);
  await expect(page.locator(".kind-field")).toHaveCount(2);
  const newGroup = page.locator(".kind-field").filter({ has: page.locator(".kind", { hasText: "New group" }) });
  await expect(newGroup).toContainText("No baseline observation of this field in this group");
  await expect(newGroup.locator(".field-group")).toContainText("9007199254740993");
  await expect(page.locator("#report-content img")).toHaveCount(0);
  const exported = await report(page);
  expect(exported.settings.watch_by).toEqual(["/service", "/route"]);
  expect(exported.result.watched_fields[0].values.find((v) => v.group_seen_in_baseline === false).group_values_json).toEqual(["9007199254740993", '"<img src=x onerror=alert(1)>"']);
});

test("a real engine response that ignored grouping is rejected and the last report survives", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.Worker = class extends NativeWorker {
      postMessage(message) {
        if (window.omitGrouping) { message = window.structuredClone(message); delete message.request.watch_by; }
        return super.postMessage(message);
      }
    };
  });
  await ready(page);
  await page.locator('[data-example="http-routes"]').click();
  await expect(page.locator(".kind-field")).toHaveCount(1);
  const before = await report(page);
  await page.evaluate(() => { window.omitGrouping = true; });
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("did not return the requested field evidence");
  expect(await report(page)).toEqual(before);
  await page.evaluate(() => { window.omitGrouping = false; });
  await run(page);
  await expect(page.locator(".kind-field")).toHaveCount(1);
});

test("group cardinality overflow remains incomplete in the real WASM report", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await page.locator("#baseline").fill('{"route":0,"status":200}');
  await page.locator("#target").fill(Array.from({ length: 257 }, (_, i) => JSON.stringify({ route: i, status: 200 })).join("\n"));
  await page.locator("#watch-fields").fill("/status");
  await page.locator("#watch-by").fill("/route");
  await run(page, true);
  const field = (await report(page)).result.watched_fields[0];
  expect(field.values).toHaveLength(256);
  expect(field.target).toMatchObject({ matched: 257, untracked: 1, first_problem: { line_no: 257 } });
  await expect(page.locator(".field-coverage")).toContainText("1 untracked");
  await expect(page.locator(".kind-field")).toHaveCount(0);
});

test("unobserved and ambiguous fields cannot look clean; corrected selectors recover", async ({ page }) => {
  await example(page);
  for (const pointer of ["/typo", "/http"]) {
    await page.locator("#watch-fields").fill(pointer);
    await run(page, true);
    await expect(page.locator(".field-evidence")).toHaveAttribute("open", "");
    await expect(page.locator("#report-content")).not.toContainText("No findings under");
    expect((await report(page)).result.watched_fields[0].complete).toBe(false);
  }
  await page.locator("#watch-fields").fill("/http/status");
  await page.locator("#target").fill('{"http":{"status":503,"status":200}}\n{"http":{"status":200}}');
  await run(page, true);
  await expect(page.locator(".field-coverage")).toContainText("1 ambiguous");
  await expect(page.locator(".field-problem")).toContainText("Target, line 1");
  await page.locator("#target").fill('{"http":{"status":503}}');
  await run(page);
  await expect(page.locator(".kind-field")).toHaveCount(1);
  await page.locator("#watch-fields").fill("http.status");
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await expect(page.locator("#watch-fields")).toBeFocused();
  await expect(page.locator("#watch-fields")).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByRole("status")).toContainText("JSON Pointers");
});

test("omitted baseline editors retain their captured labels in field counts and source evidence", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await page.locator("#baseline").fill('{"v":200}');
  await page.getByRole("button", { name: "Add a baseline run" }).click();
  await page.getByRole("button", { name: "Add a baseline run" }).click();
  await page.locator("#baseline").fill('{"v":204}');
  await page.locator("#target").fill('{"v":503}');
  await page.locator("#watch-fields").fill("/v");
  await run(page);
  await page.locator(".field-evidence > summary").click();
  await expect(page.locator(".field-values").getByRole("columnheader", { name: "Baseline 3" })).toBeVisible();
  await expect(page.locator(".field-values").getByRole("columnheader", { name: "Baseline 2" })).toHaveCount(0);
  await page.locator(".field-sources > summary").click();
  await expect(page.locator(".field-sources")).toContainText("Baseline 3, line 1");
  const downloaded = await report(page);
  expect(downloaded.sources.omitted_baselines).toEqual([2]);
  expect(downloaded.result.watched_fields[0].values.find((v) => v.value_json === "204").first_baseline.baseline_index).toBe(1);
  await page.getByRole("button", { name: "Add a baseline run" }).click();
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  expect(await report(page)).toEqual(downloaded);
});

test("numeric precision, JSON types and HTML-like values survive actual downloads", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await page.locator("#baseline").fill('{"v":9007199254740992}\n{"v":null}');
  await page.locator("#target").fill('{"v":9007199254740993}\n{"v":"9007199254740992"}\n{"v":1e400}\n{"v":"<img src=x onerror=alert(1)>"}');
  await page.locator("#watch-fields").fill("/v");
  await run(page);
  await expect(page.locator(".kind-field")).toHaveCount(4);
  await expect(page.locator("#report-content img")).toHaveCount(0);
  const fields = (await report(page)).result.watched_fields;
  expect(fields[0].values.filter((v) => v.is_new).map((v) => v.value_json)).toEqual(['"9007199254740992"', '"<img src=x onerror=alert(1)>"', "1e400", "9007199254740993"]);
  expect(fields[0].values.find((v) => v.value_json === "9007199254740993").first_target.raw).toBe('{"v":9007199254740993}');
});

test("cardinality overflow retains bounded evidence and incomplete coverage", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  await page.locator("#baseline").fill('{"v":0}');
  await page.locator("#target").fill(Array.from({ length: 100 }, (_, i) => JSON.stringify({ v: i })).join("\n"));
  await page.locator("#watch-fields").fill("/v");
  await run(page, true);
  await expect(page.locator(".field-coverage")).toContainText("36 untracked");
  await expect(page.locator(".field-values tbody tr")).toHaveCount(64);
  const field = (await report(page)).result.watched_fields[0];
  expect(field.target).toMatchObject({ matched: 100, untracked: 36, first_problem: { line_no: 65 } });
  expect(field.values.every((v) => v.is_new === null)).toBe(true);
});

test("large surrounding records have bounded context and visible clipping in the actual report", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
  const line = (v) => JSON.stringify({ v, padding: "x".repeat(12000) });
  await page.locator("#baseline").fill(line(0));
  await page.locator("#target").fill([line(0), line(1), line(0)].join("\n"));
  await page.locator("#watch-fields").fill("/v");
  await run(page);
  await expect(page.locator(".kind-field")).toContainText("Surrounding context clipped");
  const value = (await report(page)).result.watched_fields[0].values.find((v) => v.is_new);
  expect(value).toMatchObject({ value_json: "1", context_truncated: true, first_target: { line_no: 2 } });
  expect([...value.context.before, ...value.context.after].reduce((bytes, row) => bytes + new TextEncoder().encode(row[1]).length, 0)).toBeLessThanOrEqual(8192);
});

test("editing a watch cancels a held real WASM reply and preserves the previous report", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.pendingFields = [];
    window.Worker = class extends NativeWorker {
      set onmessage(handler) {
        super.onmessage = handler ? (event) => {
          if (window.holdFields) window.pendingFields.push(() => handler.call(this, event));
          else handler.call(this, event);
        } : null;
      }
    };
  });
  await example(page);
  const before = await report(page);
  await page.evaluate(() => { window.holdFields = true; });
  await page.getByRole("button", { name: "Compare runs", exact: true }).click();
  await page.waitForFunction(() => window.pendingFields.length > 0);
  await page.locator("#watch-fields").fill("/exit_code");
  await page.evaluate(() => { window.holdFields = false; window.pendingFields.forEach((send) => send()); });
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  expect(await report(page)).toEqual(before);
  await run(page);
  expect((await report(page)).result.watched_fields.map((f) => f.pointer)).toEqual(["/exit_code"]);
});

for (const theme of ["light", "dark"]) {
  test(`grouped evidence is keyboard accessible at 320px in ${theme} mode`, async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 780 });
    await page.emulateMedia({ colorScheme: theme });
    await ready(page);
    await page.locator('[data-example="http-routes"]').click();
    await expect(page.locator(".kind-field")).toHaveCount(1);
    await page.locator("#watch-fields").focus();
    await page.keyboard.press("Tab");
    await expect(page.locator("#watch-by")).toBeFocused();
    await page.locator(".field-evidence > summary").focus();
    await page.keyboard.press("Enter");
    await page.keyboard.press("Tab");
    await expect(page.locator(".field-table-scroll")).toBeFocused();
    await page.keyboard.press("End");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
  test(`field evidence is keyboard accessible at 320px in ${theme} mode`, async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 780 });
    await page.emulateMedia({ colorScheme: theme });
    await example(page);
    const summary = page.locator(".field-evidence > summary").first();
    await summary.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(".field-evidence").first()).toHaveAttribute("open", "");
    await expect(summary).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(page.locator(".field-table-scroll").first()).toBeFocused();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await page.locator("#watch-fields").fill("/typo");
    await run(page, true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
}
