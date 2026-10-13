import { test, expect } from "@playwright/test";
import { readFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";

const study = new URL("../../../studies/container-payloads/", import.meta.url);
const recorded = JSON.parse(await readFile(new URL("results/after.json", study), "utf8"));
const input = (name) => readFile(new URL(`inputs/${name}.log`, study), "utf8");
const stable = (value) => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "number" && !Number.isInteger(v) ? Number(v.toPrecision(12)) : v));

for (const width of [1440, 375]) {
  test(`decoded container watches retain source evidence and rates offline at ${width}px`, async ({ page, context }, testInfo) => {
    await page.setViewportSize({ width, height: 1000 });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto("./");
    await expect(page.getByRole("status")).toContainText("Comparison complete");
    await page.evaluate(() => document.fonts.ready);
    await context.setOffline(true);
    await page.locator("#new-comparison").click();
    // Match the CLI replay's zero context; first-source records remain complete.
    await page.locator(".options > summary").click();
    await page.locator("#context").fill("0");
    await page.locator(".options > summary").click();
    const download = async () => {
      const promise = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download report", exact: true }).click();
      return JSON.parse(await readFile(await (await promise).path(), "utf8"));
    };
    for (const [kind, caseName, selector] of [
      ["novelty", "grouped-sgr-compact", ".kind-field"],
      ["rate", "rate-sgr", ".kind-rate"],
    ]) {
      await page.locator("#baseline").fill(await input(`${kind}-baseline`));
      await page.locator("#target").fill(await input(`${kind}-target`));
      await page.locator("#watch-fields").fill("/status");
      await page.locator("#watch-by").fill("/route");
      await page.locator("#watch-rates").setChecked(kind === "rate");
      await page.locator("#run").click();
      await expect(page.getByRole("status")).toContainText("Comparison complete");
      await expect(page.locator(selector)).toHaveCount(1);
      const exported = await download();
      const expected = recorded.cases.find((item) => item.case === caseName).watch;
      expect(stable(exported.result.watched_fields[0])).toEqual(stable(expected));
      expect(exported.result.findings).toEqual([]);
      expect(exported.result.value_findings).toEqual([]);
      if (kind === "novelty") {
        await page.locator(".field-evidence > summary").click();
        await expect(page.locator(".field-values")).toContainText("503");
        await page.locator(".field-sources > summary").click();
        await expect(page.locator(".field-sources")).toContainText("Target, line 3");
        if (process.env.LOGDELTA_CONTAINER_SHOTS) {
          await mkdir(process.env.LOGDELTA_CONTAINER_SHOTS, { recursive: true });
          await page.locator("#report-content").screenshot({ path: join(process.env.LOGDELTA_CONTAINER_SHOTS, `${testInfo.project.name}-${width}.png`) });
        }
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    }
    expect(errors).toEqual([]);
  });
}
