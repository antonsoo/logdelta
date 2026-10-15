import { test, expect } from "@playwright/test";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";

const study = new URL("../../../studies/envelope-identity/", import.meta.url);
const recorded = JSON.parse(await readFile(new URL("results/after.json", study), "utf8"));

for (const width of [1440, 375]) {
  test(`application fields survive Docker recognition offline at ${width}px`, async ({ page, context }, testInfo) => {
    await page.setViewportSize({ width, height: 1000 });
    const errors = [];
    const requests = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto("./");
    await expect(page.getByRole("status")).toContainText("Comparison complete");
    await page.evaluate(() => document.fonts.ready);
    await context.setOffline(true);
    page.on("request", (request) => {
      if (["http:", "https:"].includes(new URL(request.url()).protocol)) requests.push(request.url());
    });
    await page.locator("#new-comparison").click();
    await page.locator(".options > summary").click();
    await page.locator("#context").fill("0");
    await page.locator(".options > summary").click();
    const observations = [];
    for (const [kind, name, grouped] of [
      ["native", "native-grouped", true],
      ["native-shadow", "native-shadow-pooled", false],
    ]) {
      await page.locator("#baseline").fill(await readFile(new URL(`inputs/${kind}-baseline.log`, study), "utf8"));
      await page.locator("#target").fill(await readFile(new URL(`inputs/${kind}-target.log`, study), "utf8"));
      await page.locator("#watch-fields").fill("/http/status");
      await page.locator("#watch-by").fill(grouped ? "/route" : "");
      await page.locator("#run").click();
      await expect(page.getByRole("status")).toContainText("Comparison complete");
      await expect(page.locator(".kind-field")).toHaveCount(1);
      const pending = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download report", exact: true }).click();
      const exported = JSON.parse(await readFile(await (await pending).path(), "utf8"));
      expect(exported.result.watched_fields[0]).toEqual(recorded.cases.find((entry) => entry.case === name).watch);
      observations.push({ case: name, watch: exported.result.watched_fields[0], matchesNative: true });
      await page.locator(".field-evidence > summary").click();
      await expect(page.locator(".field-values")).toContainText("503");
      await page.locator(".field-sources > summary").click();
      await expect(page.locator(".field-sources")).toContainText("Target, line 2");
      await expect(page.locator(".field-sources")).toContainText('"log"');
      if (kind === "native" && process.env.LOGDELTA_ENVELOPE_SHOTS) {
        await mkdir(process.env.LOGDELTA_ENVELOPE_SHOTS, { recursive: true });
        await page.locator("#report-content").screenshot({ path: join(process.env.LOGDELTA_ENVELOPE_SHOTS, `${testInfo.project.name}-${width}.png`) });
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    }
    expect(errors).toEqual([]);
    expect(requests).toEqual([]);
    if (process.env.LOGDELTA_ENVELOPE_SHOTS) {
      await writeFile(join(process.env.LOGDELTA_ENVELOPE_SHOTS, `${testInfo.project.name}-${width}.json`), JSON.stringify({
        engine: testInfo.project.name, width, offlineBeforeFirstComparison: true,
        networkRequests: requests, pageErrors: errors, workflows: observations,
      }, null, 2) + "\n");
    }
  });
}
