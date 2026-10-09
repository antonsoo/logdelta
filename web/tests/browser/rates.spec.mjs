import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFile } from "node:fs/promises";
import { Buffer } from "node:buffer";

async function ready(page) {
  await page.goto("./");
  await expect(page.locator("#workspace-status")).toContainText("Comparison complete");
}
async function example(page) {
  await ready(page);
  await page.locator('[data-example="http-rates"]').click();
  await expect(page.locator(".kind-rate")).toHaveCount(1);
}
async function run(page, incomplete = false) {
  await page.locator("#run").click();
  await expect(page.locator("#workspace-status")).toContainText(incomplete ? "Field comparison incomplete" : "Comparison complete");
}
async function report(page, id = "download-report") {
  const [download] = await Promise.all([page.waitForEvent("download"), page.locator(`#${id}`).click()]);
  return JSON.parse(await readFile(await download.path(), "utf8"));
}
const stable = (v) => JSON.parse(JSON.stringify(v, (_, n) => typeof n === "number" && !Number.isInteger(n) ? Number(n.toPrecision(12)) : n));
const log = (counts, route = "/checkout") => counts.flatMap(([status, count]) => Array.from({ length: count }, () => JSON.stringify({ route, status }))).join("\n");
async function openLog(page, editor, text, name) {
  await page.locator(`input[data-file-for="${editor}"]`).setInputFiles({ name, mimeType: "text/plain", buffer: Buffer.from(text) });
  await expect(page.locator("#workspace-status")).toContainText(`Opened ${name}.`);
}
async function inputs(page, baselines, target, group = "/route") {
  await ready(page);
  await page.locator("#new-comparison").click();
  for (const [i, text] of baselines.entries()) {
    if (i) await page.locator("[data-add-baseline]").click();
    await openLog(page, "baseline", text, `baseline-${i + 1}.log`);
  }
  await openLog(page, "target", target, "target.log");
  await page.locator("#watch-fields").fill("/status");
  await page.locator("#watch-by").fill(group);
  await page.locator("#watch-rates").check();
}

test.beforeEach(async ({ page, baseURL }) => {
  const errors = [], external = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => { if (new URL(r.url()).origin !== new URL(baseURL).origin) external.push(r.url()); });
  await page.exposeFunction("rateDiagnostics", () => ({ errors, external }));
  await page.addInitScript(() => {
    window.rateViolations = [];
    document.addEventListener("securitypolicyviolation", (e) => window.rateViolations.push(e.violatedDirective));
    window.addEventListener("unhandledrejection", (e) => window.rateViolations.push(String(e.reason)));
  });
});
test.afterEach(async ({ page }) => {
  expect(await page.evaluate(() => window.rateDiagnostics())).toEqual({ errors: [], external: [] });
  expect(await page.evaluate(() => window.rateViolations)).toEqual([]);
});

test("captured HTTP rates match native evidence, while novelty stays quiet; edits keep the applied report", async ({ page, context }) => {
  await example(page);
  await expect(page.locator("#watch-rates")).toBeChecked();
  await expect(page.locator("#watch-rate-change")).toHaveValue("5");
  await expect(page.locator(".summary-line")).toContainText("1 finding");
  await expect(page.locator(".kind-field")).toHaveCount(0);
  await expect(page.locator(".kind-rate .field-group")).toHaveText('/route = "/checkout"');
  await expect(page.locator(".rate-headline")).toContainText("20% in target");
  await expect(page.locator(".rate-values tbody tr").first()).toHaveText("50310 / 1,000 (1%)12 / 1,000 (1.2%)200 / 1,000 (20%)+18.8 pp287.023");
  await expect(page.locator(".rate-source-body .log")).toHaveCount(0);
  await page.locator(".rate-source > summary").first().click();
  await expect(page.locator(".rate-source-body .is-hit")).toContainText('"status":503');
  await expect(page.locator(".rate-source-body .is-hit .gutter")).toHaveText("2");
  await page.locator(".rate-source > summary").first().click();
  await expect(page.locator(".rate-source-body .log")).toHaveCount(0);
  // Match the previously retained native capture with context off, including every ledger entry.
  await page.locator(".options > summary").click();
  await page.locator("#context").fill("0");
  await run(page);
  const applied = await report(page);
  const native = JSON.parse(await readFile(new URL("../../../docs/field-rate-evidence/native.json", import.meta.url), "utf8"));
  expect(stable(applied.result)).toEqual(stable(native));
  expect(applied.schema_version).toBe(3);
  expect(applied.settings.watch_rate_change).toBe(5);
  expect(applied.result).toEqual(await report(page, "download-json"));
  await context.setOffline(true);
  await page.locator("#watch-rates").uncheck();
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  expect(await report(page)).toEqual(applied);
  await run(page);
  await expect(page.locator(".summary-line")).toContainText("0 findings");
  expect((await report(page)).settings.watch_rate_change).toBeUndefined();
  await page.locator("#watch-rates").check();
  await page.locator("#watch-rate-change").fill("19");
  await run(page);
  await expect(page.locator(".kind-rate")).toHaveCount(0);
  await page.locator("#watch-rate-change").fill("18.8");
  await run(page);
  await expect(page.locator(".kind-rate")).toHaveCount(1);
});

test("missing groups are unknown while a known value falling to zero is measured", async ({ page }) => {
  await inputs(page, [log([[200, 800], [503, 200]]) + '\n' + log([[200, 10]], "/retired")], log([[200, 1000]]) + '\n' + log([[200, 10]], "/new"));
  await run(page, true);
  await expect(page.locator(".field-evidence")).toHaveAttribute("open", "");
  await expect(page.locator(".field-evidence")).toContainText("2 groups have no observations on one side");
  await expect(page.locator("#report-content")).not.toContainText("No findings under");
  const rows = page.locator(".field-evidence .field-values tbody tr");
  await expect(rows.filter({ hasText: '"/retired"' })).toContainText("Rate unavailable: no target observations");
  await expect(rows.filter({ hasText: '"/new"' })).toContainText("Rate unavailable: no baseline observations");
  await expect(page.locator(".rate-values")).toContainText("0 / 1,000 (0%)");
  const absent = page.locator(".rate-source").filter({ hasText: "absent from target" });
  await expect(absent).toContainText("503 · Baseline 1, line 801");
  await absent.locator("summary").click();
  await expect(absent.locator(".log .is-hit")).toContainText('"status":503');
  await expect(absent.locator(".is-context")).toHaveCount(0);
  const exported = await report(page);
  const field = exported.result.watched_fields[0];
  expect(field.complete).toBe(true);
  expect(field.rate_comparison.complete).toBe(false);
  expect(field.rate_comparison.groups.filter((g) => g.changes.length)).toHaveLength(1);
});

test("an unobserved individual baseline is not zero; source labels survive unused editors", async ({ page }) => {
  await ready(page);
  await page.locator("#new-comparison").click();
  await openLog(page, "baseline", log([[200, 1000]], "/maintenance"), "maintenance.log");
  await page.locator("[data-add-baseline]").click();
  await page.locator("[data-add-baseline]").click();
  await openLog(page, "baseline", log([[200, 800], [503, 200]]) + '\n' + log([[200, 1000]], "/maintenance"), "checkout-baseline.log");
  await openLog(page, "target", log([[200, 1000]]) + '\n' + log([[200, 1000]], "/maintenance"), "checkout-target.log");
  await page.locator("#watch-fields").fill("/status");
  await page.locator("#watch-by").fill("/route");
  await page.locator("#watch-rates").check();
  await run(page);
  await expect(page.locator(".rate-values")).toContainText("Unobserved");
  await expect(page.locator(".rate-values").getByRole("columnheader", { name: "Baseline 3" })).toBeVisible();
  await expect(page.locator(".rate-source").filter({ hasText: "absent from target" })).toContainText("Baseline 3, line 801");
  const exported = await report(page);
  expect(exported.sources.omitted_baselines).toEqual([2]);
  expect(exported.result.watched_fields[0].rate_comparison.complete).toBe(true);
  expect(exported.result.watched_fields[0].rate_comparison.groups[0].changes[0].baseline_rates[0]).toBeNull();
});

test("incomplete and truncated ledgers never display partial rate denominators", async ({ page }) => {
  await example(page);
  await page.locator("#target").fill('{"route":"/checkout","http":{"status":200}}\n{invalid}');
  await run(page, true);
  await expect(page.locator(".kind-rate")).toHaveCount(0);
  await expect(page.locator(".field-evidence")).toContainText("Rates unavailable: incomplete field coverage");
  await expect(page.locator(".rate-fraction")).toHaveCount(0);
  expect((await report(page)).result.watched_fields[0].rate_comparison.groups).toEqual([]);
  await page.locator("#target").fill(Array.from({ length: 257 }, (_, i) => JSON.stringify({ route: i, http: { status: 200 } })).join("\n"));
  await run(page, true);
  await expect(page.locator(".rate-fraction")).toHaveCount(0);
  await expect(page.locator(".field-coverage")).toContainText("untracked");
  expect((await report(page)).result.watched_fields[0].rate_comparison.groups).toEqual([]);
});

test("invalid settings preserve evidence and focus the correction; reset and examples clear rates", async ({ page }) => {
  await example(page);
  const before = await report(page);
  for (const value of ["", "0", "-1", "101"]) {
    await page.locator("#watch-rate-change").fill(value);
    await page.locator("#run").click();
    await expect(page.locator("#workspace-status")).toContainText("greater than 0 and at most 100");
    await expect(page.locator("#watch-rate-change")).toBeFocused();
    expect(await report(page)).toEqual(before);
  }
  await page.locator("#watch-rate-change").fill("5");
  await page.locator("#watch-fields").fill("");
  await page.locator("#watch-by").fill("");
  await page.locator("#run").click();
  await expect(page.locator("#workspace-status")).toContainText("at least one watched field");
  await expect(page.locator("#watch-fields")).toBeFocused();
  await page.locator('[data-example="http-routes"]').click();
  await expect(page.locator(".kind-field")).toHaveCount(1);
  await expect(page.locator("#watch-rates")).not.toBeChecked();
  await expect(page.locator("#watch-rate-change")).toBeDisabled();
  await page.locator('[data-example="http-rates"]').click();
  await expect(page.locator(".kind-rate")).toHaveCount(1);
  await page.locator("#new-comparison").click();
  await expect(page.locator("#watch-rates")).not.toBeChecked();
  await expect(page.locator("#rate-threshold")).toBeHidden();
  await expect(page.locator(".rate-source")).toHaveCount(0);
});

test("a real worker that ignores requested rates is rejected and can recover", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.Worker = class extends NativeWorker {
      postMessage(message) {
        if (window.omitRates) { message = window.structuredClone(message); delete message.request.watch_rate_change; }
        return super.postMessage(message);
      }
    };
  });
  await example(page);
  const before = await report(page);
  await page.evaluate(() => { window.omitRates = true; });
  await page.locator("#run").click();
  await expect(page.locator("#workspace-status")).toContainText("did not return the requested rate evidence");
  expect(await report(page)).toEqual(before);
  await page.evaluate(() => { window.omitRates = false; });
  await run(page);
  await expect(page.locator(".kind-rate")).toHaveCount(1);
});

for (const width of [375, 1440]) for (const theme of ["light", "dark"]) {
  test(`rate evidence uses keyboard scrolling and readable controls at ${width}px in ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1000 });
    await page.emulateMedia({ colorScheme: theme });
    await example(page);
    const toggle = page.locator("#watch-rates");
    await toggle.focus();
    await page.keyboard.press("Tab");
    await expect(page.locator("#watch-rate-change")).toBeFocused();
    const scroller = page.locator(".kind-rate .field-table-scroll");
    await page.locator('[data-filter="rate"]').focus();
    await page.keyboard.press("Tab");
    await expect(scroller).toBeFocused();
    if (width === 375) {
      await page.keyboard.press("ArrowRight");
      await expect.poll(() => scroller.evaluate((e) => e.scrollLeft)).toBeGreaterThan(0);
    }
    await page.keyboard.press("Tab");
    await expect(page.locator(".rate-source > summary").first()).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.locator(".rate-source-body .is-hit")).toContainText('"status":503');
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
}

test("many changed groups stay paged, searchable and complete in exports", async ({ page }) => {
  const keys = Array.from({ length: 64 }, (_, i) => i === 0 ? '<img src=x onerror="alert(1)">' : `/route/${String(i).padStart(3, "0")}`);
  await inputs(page, [keys.map((k) => log([[200, 99], [503, 1]], k)).join("\n")], keys.map((k) => log([[200, 80], [503, 20]], k)).join("\n"));
  await run(page);
  const applied = await report(page);
  expect(applied.result.watched_fields[0].rate_comparison.groups.filter((g) => g.changes.length)).toHaveLength(64);
  await expect(page.locator(".summary-line")).toContainText("64 findings");
  await expect(page.locator(".kind-rate")).toHaveCount(50);
  await expect(page.locator(".rate-source-body .log")).toHaveCount(0);
  await expect(page.locator("#report-content img")).toHaveCount(0);
  const field = page.locator(".field-evidence");
  await field.locator(":scope > summary").click();
  await expect(field.locator(".field-values tbody tr")).toHaveCount(32);
  await field.getByRole("searchbox").fill("/route/063");
  await expect(field.locator(".field-values tbody tr")).toHaveCount(2);
  await expect(field).toContainText("20 / 100 (20%)");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.locator(".kind-rate")).toHaveCount(14);
  await expect(field.getByRole("searchbox")).toHaveValue("/route/063");
  expect(await report(page)).toEqual(applied);
  expect(await report(page, "download-json")).toEqual(applied.result);
  await page.locator("#new-comparison").click();
  await expect(page.locator(".kind-rate, .field-evidence, .rate-source-body")).toHaveCount(0);
});

test("editing the rate threshold cancels a held real worker result", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.pendingRates = [];
    window.Worker = class extends NativeWorker {
      set onmessage(handler) {
        super.onmessage = handler ? (event) => {
          if (window.holdRates) window.pendingRates.push(() => handler.call(this, event));
          else handler.call(this, event);
        } : null;
      }
    };
  });
  await example(page);
  const before = await report(page);
  await page.evaluate(() => { window.holdRates = true; });
  await page.locator("#run").click();
  await page.waitForFunction(() => window.pendingRates.length > 0);
  await page.locator("#watch-rate-change").fill("25");
  await expect(page.locator("#workspace-status")).toContainText("Comparison cancelled because inputs changed");
  await page.evaluate(() => { window.holdRates = false; window.pendingRates.splice(0).forEach((deliver) => deliver()); });
  expect(await report(page)).toEqual(before);
  await run(page);
  await expect(page.locator(".kind-rate")).toHaveCount(0);
  expect((await report(page)).settings.watch_rate_change).toBe(25);
});
