import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFile } from "node:fs/promises";
import { Buffer } from "node:buffer";
import { gzipSync } from "node:zlib";
import { createHash } from "node:crypto";

const baseline = (page) => page.locator("#baseline");
const target = (page) => page.locator("#target");
const compare = (page) => page.getByRole("button", { name: "Compare runs", exact: true });
const status = (page) => page.getByRole("status");
const file = (name, text) => ({ name, mimeType: "text/plain", buffer: Buffer.from(text) });

async function ready(page) {
  await page.goto("./");
  await expect(status(page)).toContainText("Comparison complete");
}
async function blank(page) {
  await ready(page);
  await page.getByRole("button", { name: "New comparison", exact: true }).click();
}
async function inputs(page, good = "started\nfinished\n", bad = "started\ncatastrophic failure\n") {
  await baseline(page).fill(good);
  await target(page).fill(bad);
}
async function completed(page) {
  await compare(page).click();
  await expect(status(page)).toContainText("Comparison complete");
}
async function exported(page, button = "Download report") {
  const event = page.waitForEvent("download");
  await page.getByRole("button", { name: button, exact: true }).click();
  const download = await event;
  return JSON.parse(await readFile(await download.path(), "utf8"));
}

// Pause real file streams without substituting their contents. Cancellation still closes them.
async function delayFiles(page) {
  await page.addInitScript(() => {
    const original = File.prototype.stream;
    window.imports = {};
    File.prototype.stream = function () {
      const stream = original.call(this);
      if (!this.name.startsWith("slow-")) return stream;
      const name = this.name;
      let cancelled = false;
      return new ReadableStream({
        start(controller) {
          window.imports[name] = async () => {
            if (cancelled) return;
            const reader = stream.getReader();
            try {
              for (;;) {
                const { done, value } = await reader.read();
                if (cancelled) return;
                if (done) { controller.close(); return; }
                controller.enqueue(value);
              }
            } finally { reader.releaseLock(); }
          };
        },
        cancel() { cancelled = true; return stream.cancel(); },
      });
    };
  });
}
async function openDelayed(page, which, name, text) {
  await page.locator(`input[data-file-for="${which}"]`).setInputFiles(file(name, text));
  await page.waitForFunction((key) => !!window.imports[key], name);
}
async function releaseFile(page, name) { await page.evaluate((key) => window.imports[key](), name); }

// Hold the actual WASM result at the worker boundary, so races have a deterministic order.
async function controlWorkers(page) {
  await page.addInitScript(() => {
    const NativeWorker = Worker;
    window.heldReplies = [];
    window.Worker = class extends NativeWorker {
      constructor(...args) {
        if (window.failNextWorker) { window.failNextWorker = false; throw new Error("worker startup blocked"); }
        super(...args);
      }
      set onmessage(handler) {
        super.onmessage = handler ? (event) => {
          if (window.holdReplies) window.heldReplies.push(() => handler.call(this, event));
          else handler.call(this, event);
        } : null;
      }
    };
  });
}

test.beforeEach(async ({ page, baseURL }) => {
  const errors = [];
  const external = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => { if (new URL(r.url()).origin !== new URL(baseURL).origin) external.push(r.url()); });
  await page.exposeFunction("testDiagnostics", () => ({ errors, external }));
  await page.addInitScript(() => {
    window.violations = [];
    window.rejections = [];
    document.addEventListener("securitypolicyviolation", (e) => window.violations.push(e.violatedDirective));
    window.addEventListener("unhandledrejection", (e) => window.rejections.push(String(e.reason)));
  });
});
test.afterEach(async ({ page }) => {
  expect(await page.evaluate(() => window.violations)).toEqual([]);
  expect(await page.evaluate(() => window.rejections)).toEqual([]);
  expect(await page.evaluate(() => window.testDiagnostics())).toEqual({ errors: [], external: [] });
});

test("real WASM examples and both JSON exports agree on counts and source order", async ({ page }) => {
  await ready(page);
  for (const example of ["pytest", "k8s", "large"]) {
    await page.locator(`[data-example="${example}"]`).click();
    await expect(status(page)).toContainText("Comparison complete");
    const report = await exported(page);
    const raw = await exported(page, "Download JSON");
    expect(report.result).toEqual(raw);
    expect(report.format).toBe("logdelta-report");
    expect(report.schema_version).toBe(1);
    expect(report.sources.baselines.map((s) => s.lines)).toEqual(raw.baseline_totals);
    expect(report.sources.target.lines).toBe(raw.target_total);
    expect(raw.findings.length).toBeGreaterThan(0);
    await expect(page.locator("#report-status")).toContainText("matches the loaded logs");
  }
});

test("delayed baseline import stays with its original tab", async ({ page }) => {
  await delayFiles(page);
  await blank(page);
  await openDelayed(page, "baseline", "slow-first.log", "belongs to first baseline\n");
  await page.getByRole("button", { name: "Add a baseline run" }).click();
  await baseline(page).fill("second baseline\n");
  await releaseFile(page, "slow-first.log");
  await expect(baseline(page)).toHaveValue("second baseline\n");
  await page.getByRole("tab", { name: /^Baseline 1:/ }).click();
  await expect(baseline(page)).toHaveValue("belongs to first baseline\n");
  await expect(page.locator("#baseline-source")).toHaveText("slow-first.log");
});

test("newer file wins in the same source and independent target imports survive", async ({ page }) => {
  await delayFiles(page);
  await blank(page);
  await openDelayed(page, "baseline", "slow-old.log", "obsolete baseline\n");
  await openDelayed(page, "target", "slow-target.log", "independent target\n");
  await page.locator('input[data-file-for="baseline"]').setInputFiles(file("new.log", "newest baseline\n"));
  await expect(baseline(page)).toHaveValue("newest baseline\n");
  await releaseFile(page, "slow-old.log");
  await releaseFile(page, "slow-target.log");
  await expect(target(page)).toHaveValue("independent target\n");
  await expect(baseline(page)).toHaveValue("newest baseline\n");
});

test("editing, clearing, removing and resetting invalidate pending file reads", async ({ page }) => {
  await delayFiles(page);
  await blank(page);
  for (const action of ["edit", "clear", "remove", "reset"]) {
    if (action === "remove") await page.getByRole("button", { name: "Add a baseline run" }).click();
    const name = `slow-${action}.log`;
    await openDelayed(page, "baseline", name, "must never arrive\n");
    if (action === "edit") await baseline(page).fill("manual edit");
    if (action === "clear") await page.locator('[data-clear="baseline"]').click();
    if (action === "remove") await page.locator("[data-remove-baseline]").click();
    if (action === "reset") await page.getByRole("button", { name: "New comparison", exact: true }).click();
    await releaseFile(page, name);
    await expect(baseline(page)).not.toHaveValue("must never arrive\n");
    await expect(compare(page)).toBeEnabled();
  }
});

test("cancel stops imports without clearing loaded logs or the report", async ({ page }) => {
  await delayFiles(page);
  await ready(page);
  const original = await baseline(page).inputValue();
  const report = await exported(page);
  await openDelayed(page, "baseline", "slow-cancel.log", "discarded\n");
  await expect(page.getByRole("button", { name: "Loading logs…", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await releaseFile(page, "slow-cancel.log");
  await expect(baseline(page)).toHaveValue(original);
  await expect(compare(page)).toBeFocused();
  expect(await exported(page)).toEqual(report);
});

test("a late example cannot replace manual edits or a newer example", async ({ page }) => {
  await ready(page);
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  let requested = false;
  await page.route("**/examples/k8s-service-incident.log", async (route) => {
    requested = true;
    await gate;
    await route.fulfill({ status: 200, body: "obsolete example\n" }).catch(() => {});
  });
  await page.locator('[data-example="k8s"]').click();
  await expect.poll(() => requested).toBe(true);
  await target(page).fill("my manual target\n");
  release();
  await expect(target(page)).toHaveValue("my manual target\n");
  await page.locator('[data-example="large"]').click();
  await expect(status(page)).toContainText("Comparison complete");
  expect((await exported(page)).sources.baselines).toHaveLength(3);
});

test("example fetch failures keep current logs and allow retry", async ({ page }) => {
  await ready(page);
  const original = await target(page).inputValue();
  await page.route("**/examples/k8s-service-incident.log", (route) => route.fulfill({ status: 503, body: "unavailable" }));
  await page.locator('[data-example="k8s"]').click();
  await expect(status(page)).toContainText("HTTP 503");
  await expect(target(page)).toHaveValue(original);
  await expect(compare(page)).toBeEnabled();
  await page.unroute("**/examples/k8s-service-incident.log");
  await page.locator('[data-example="k8s"]').click();
  await expect(status(page)).toContainText("Comparison complete");
});

test("editing during analysis cancels it; old grouped evidence and exports stay coherent", async ({ page }) => {
  await controlWorkers(page);
  await ready(page);
  const oldReport = await exported(page);
  const oldLines = await page.locator(".log-text").allTextContents();
  await page.evaluate(() => { window.holdReplies = true; });
  await compare(page).click();
  await page.waitForFunction(() => window.heldReplies.length === 1);
  await target(page).fill("this text was never compared\n");
  await expect(page.locator("#report-status")).toContainText("Inputs changed");
  await page.evaluate(() => { window.holdReplies = false; window.heldReplies.forEach((release) => release()); });
  expect(await page.locator(".log-text").allTextContents()).toEqual(oldLines);
  expect(await exported(page)).toEqual(oldReport);
  await completed(page);
  expect((await exported(page)).sources.target.lines).toBe(1);
  await expect(page.locator("#report-status")).toContainText("matches the loaded logs");
});

test("cancel and reset discard pending worker replies and recover with a fresh worker", async ({ page }) => {
  await controlWorkers(page);
  await ready(page);
  for (const action of ["Cancel", "New comparison"]) {
    await page.evaluate(() => { window.holdReplies = true; window.heldReplies = []; });
    await compare(page).click();
    await page.waitForFunction(() => window.heldReplies.length === 1);
    await page.getByRole("button", { name: action, exact: true }).click();
    await page.evaluate(() => { window.holdReplies = false; window.heldReplies.forEach((release) => release()); });
    if (action === "New comparison") {
      await expect(page.locator(".summary")).toHaveCount(0);
      await expect(baseline(page)).toBeFocused();
      await inputs(page);
    }
    await completed(page);
  }
});

test("worker startup failure is actionable and a retry works", async ({ page }) => {
  await controlWorkers(page);
  await blank(page);
  await inputs(page);
  await page.evaluate(() => { window.failNextWorker = true; });
  await compare(page).click();
  await expect(status(page)).toContainText("worker startup blocked");
  await expect(compare(page)).toBeEnabled();
  await completed(page);
});

test("WASM fetch failure is recoverable without reloading the page", async ({ page }) => {
  await blank(page);
  await inputs(page);
  await page.route("**/assets/logdelta-*.wasm", (route) => route.fulfill({ status: 503, body: "unavailable" }));
  await compare(page).click();
  await expect(status(page)).toContainText("Comparison failed");
  await page.unroute("**/assets/logdelta-*.wasm");
  await completed(page);
});

test("report identifies the actual executed module and never requests the legacy cache key", async ({ page }) => {
  let legacyRequests = 0;
  await page.route("**/logdelta.wasm", (route) => { legacyRequests++; return route.abort(); });
  const downloaded = page.waitForResponse((response) => /\/assets\/logdelta-[^/]+\.wasm$/.test(new URL(response.url()).pathname));
  await ready(page);
  const response = await downloaded;
  expect(response.status()).toBe(200);
  const expected = createHash("sha256").update(await response.body()).digest("hex");
  const report = await exported(page);
  expect(report.engine.wasm_sha256).toBe(expected);
  expect(legacyRequests).toBe(0);
  await blank(page);
  await inputs(page);
  await completed(page);
  expect((await exported(page)).engine.wasm_sha256).toBe(expected);
});

for (const [name, bytes] of [["invalid", Buffer.from("not WebAssembly")], ["incompatible", Buffer.from([0, 97, 115, 109, 1, 0, 0, 0])]]) {
  test(`${name} engine module preserves inputs and a subsequent valid load recovers`, async ({ page }) => {
    await blank(page);
    await inputs(page);
    await page.route("**/assets/logdelta-*.wasm", (route) => route.fulfill({ status: 200, contentType: "application/wasm", body: bytes }));
    await compare(page).click();
    await expect(status(page)).toContainText("Comparison failed");
    await expect(target(page)).toHaveValue("started\ncatastrophic failure\n");
    await expect(compare(page)).toBeEnabled();
    await page.unroute("**/assets/logdelta-*.wasm");
    await completed(page);
    expect((await exported(page)).engine.wasm_sha256).toMatch(/^[0-9a-f]{64}$/);
  });
}

test("intentional empty targets report gone lines; unused baseline slots are identified", async ({ page }) => {
  await blank(page);
  await baseline(page).fill("job started\nfinished successfully\n");
  await page.getByRole("button", { name: "Add a baseline run" }).click();
  await page.locator('input[data-file-for="target"]').setInputFiles(file("empty.log", ""));
  await expect(page.locator("#target-count")).toHaveText("0 lines");
  await completed(page);
  const report = await exported(page);
  expect(report.result.target_total).toBe(0);
  expect(report.result.findings.every((f) => f.kind === "gone")).toBe(true);
  expect(report.result.findings.length).toBeGreaterThan(0);
  expect(report.sources.omitted_baselines).toEqual([2]);
  await page.locator("#source-details summary").click();
  await expect(page.locator("#source-details")).toContainText("Unused baseline editors omitted: 2");
  await page.locator('[data-empty="baseline"]').click();
  await completed(page);
  expect((await exported(page)).result.baseline_totals).toEqual([2, 0]);
});

test("invalid options keep prior evidence, and a corrected mask is exported exactly", async ({ page }) => {
  await ready(page);
  const old = await exported(page);
  await page.locator(".options summary").click();
  await page.locator("#context").fill("1.5");
  await compare(page).click();
  await expect(status(page)).toContainText("whole number");
  await expect(page.locator("#context")).toBeFocused();
  expect(await exported(page)).toEqual(old);
  await page.locator("#context").fill("0");
  await page.locator("#masks").fill("(");
  await compare(page).click();
  await expect(status(page)).toContainText("Comparison failed");
  expect(await exported(page)).toEqual(old);
  await page.locator("#masks").fill(" ORDER=ord_[a-z]+ \n\n");
  await completed(page);
  expect((await exported(page)).settings).toEqual({ context: 0, masks: ["ORDER=ord_[a-z]+"], watch_fields: [] });
});

test("gzip, UTF-16 and failed file reads preserve existing input and recover", async ({ page }) => {
  await blank(page);
  const body = Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from("job started\r\njob finished\r\n", "utf16le")]);
  await page.locator('input[data-file-for="baseline"]').setInputFiles({ name: "compressed.data", mimeType: "application/octet-stream", buffer: gzipSync(body) });
  await expect(baseline(page)).toHaveValue("job started\njob finished\n");
  const before = await baseline(page).inputValue();
  for (const buffer of [Buffer.from([1, 0, 3]), Buffer.from([0x1f, 0x8b, 0, 0]), gzipSync(Buffer.alloc(25 * 1024 * 1024 + 1, 120))]) {
    await page.locator('input[data-file-for="baseline"]').setInputFiles({ name: "broken.log", mimeType: "application/octet-stream", buffer });
    await expect(status(page)).toContainText("Previous input kept");
    await expect(baseline(page)).toHaveValue(before);
  }
  await target(page).fill("job failed\n");
  await completed(page);
});

test("source names and log text render as text and downloads retain evidence", async ({ page }) => {
  await blank(page);
  const hostile = '<img src=x onerror="window.injected=true">';
  await page.locator('input[data-file-for="baseline"]').setInputFiles(file(`${hostile}.log`, "good\n"));
  await target(page).fill(`${hostile}\n`);
  await completed(page);
  await page.locator("#source-details summary").click();
  await expect(page.locator("#source-details")).toContainText(hostile);
  await expect(page.locator("#results img")).toHaveCount(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  expect(JSON.stringify((await exported(page)).result)).toContain("onerror");
});

test("keyboard tabs, focus, filters and disclosure state survive updates", async ({ page }) => {
  await page.goto("./");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to log editors" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(status(page)).toContainText("Comparison complete");
  await page.locator('[data-example="large"]').click();
  await expect(status(page)).toContainText("Comparison complete");
  await page.getByRole("tab", { name: /^Baseline 1:/ }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: /^Baseline 2:/ })).toBeFocused();
  await page.keyboard.press("End");
  await expect(page.getByRole("tab", { name: /^Baseline 3:/ })).toBeFocused();
  await page.keyboard.press("Home");
  await expect(page.getByRole("tab", { name: /^Baseline 1:/ })).toBeFocused();
  await page.locator("#source-details summary").click();
  await page.locator('[data-filter="new"]').click();
  await expect(page.locator('[data-filter="new"]')).toBeFocused();
  await expect(page.locator("#source-details")).toHaveAttribute("open", "");
  await page.locator('[data-filter="all"]').click();
  await expect(page.locator("#source-details")).toHaveAttribute("open", "");
  await page.locator("[data-remove-baseline]").click();
  await expect(page.getByRole("tab", { selected: true })).toBeFocused();
});

test("a loaded engine compares and exports offline without persisting log contents", async ({ page, context }) => {
  await ready(page);
  await context.setOffline(true);
  await inputs(page, "private baseline phrase\n", "private target phrase\n");
  await completed(page);
  expect((await exported(page)).sources.target).toMatchObject({ origin: "edited", name: "examples/pytest-fail.log (edited)" });
  const stored = await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage }, url: location.href }));
  expect(JSON.stringify(stored)).not.toContain("private");
  await context.setOffline(false);
});

for (const colorScheme of ["light", "dark"]) {
  for (const width of [320, 1360]) {
    test(`accessible ${colorScheme} workspace at ${width}px with all baseline tabs`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await ready(page);
      for (let i = 1; i < 8; i++) await page.getByRole("button", { name: "Add a baseline run" }).click();
      await expect(page.getByRole("button", { name: "Add a baseline run" })).toBeDisabled();
      await page.locator(".options summary").click();
      await page.locator("#source-details summary").click();
      const audit = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "best-practice"]).analyze();
      expect(audit.violations).toEqual([]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
  }
}

test("large finding sets are paginated while exports retain every finding", async ({ page }) => {
  await blank(page);
  const normal = "checkpoint steady\n";
  const novel = Array.from({ length: 120 }, (_, i) => `novel${String.fromCharCode(97 + Math.floor(i / 26), 97 + i % 26)}\n${normal.repeat(3)}`).join("");
  await inputs(page, normal.repeat(4), novel);
  await completed(page);
  await page.locator('[data-filter="new"]').click();
  await expect(page.locator(".finding")).toHaveCount(50);
  await expect(page.locator("#findings-page")).toHaveText("Showing 1–50 of 120 findings");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.locator("#findings-page")).toHaveText("Showing 51–100 of 120 findings");
  await expect(page.locator("#findings-page")).toBeFocused();
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.locator(".finding")).toHaveCount(20);
  await expect(page.getByRole("button", { name: "Next", exact: true })).toBeDisabled();
  expect((await exported(page)).result.findings.filter((f) => f.kind === "new")).toHaveLength(120);
  await page.locator('[data-filter="all"]').click();
  await expect(page.locator("#findings-page")).toContainText("Showing 1–50");
});

test("a failed download keeps the report and can be retried", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    const original = URL.createObjectURL;
    URL.createObjectURL = (...args) => {
      URL.createObjectURL = original;
      throw new Error(`download unavailable (${args.length})`);
    };
  });
  await page.getByRole("button", { name: "Download report", exact: true }).click();
  await expect(status(page)).toContainText("Couldn't download the report");
  expect((await exported(page)).result.target_total).toBeGreaterThan(0);
});

test("a lost connection after reset keeps logs and recovers when site assets return", async ({ page, context }) => {
  await blank(page);
  await context.setOffline(true);
  await inputs(page);
  await compare(page).click();
  // Browsers may retain the worker assets in cache. Either outcome must be recoverable.
  await expect(status(page)).toContainText(/Comparison complete|Comparison failed/);
  await expect(target(page)).toHaveValue("started\ncatastrophic failure\n");
  await context.setOffline(false);
  await completed(page);
});

test("blocked storage does not prevent loading, comparing or changing theme", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "localStorage", { get() { throw new Error("storage denied"); } });
  });
  await ready(page);
  await page.getByRole("button", { name: "Switch color theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", /dark|light/);
  await completed(page);
});

test("finishing an import preserves focus on the add-baseline control", async ({ page }) => {
  await delayFiles(page);
  await blank(page);
  await openDelayed(page, "baseline", "slow-focus.log", "loaded\n");
  const add = page.getByRole("button", { name: "Add a baseline run" });
  await add.focus();
  await releaseFile(page, "slow-focus.log");
  await expect(status(page)).toContainText("Opened slow-focus.log");
  await expect(add).toBeFocused();
});

test("block expansion is bounded, retains disclosure state, and exports every template", async ({ page }) => {
  await blank(page);
  const text = Array.from({ length: 2100 }, (_, i) => `novel${String.fromCharCode(97 + Math.floor(i / 676), 97 + Math.floor(i / 26) % 26, 97 + i % 26)}\n`).join("");
  await inputs(page, "checkpoint steady\n", text);
  await completed(page);
  const block = page.locator(".is-block.kind-new");
  await expect(block.locator(".log-line")).toHaveCount(36);
  await expect(block.locator(".block-templates li")).toHaveCount(12);
  await block.locator(".block-details summary").click();
  await expect(block.locator(".block-templates li")).toHaveCount(2000);
  await block.getByRole("button", { name: "Show first 2,000", exact: true }).click();
  await expect(block.locator(".log-line")).toHaveCount(2000);
  await expect(block).toContainText("100 more lines not shown");
  await expect(block.getByRole("button", { name: "Show less", exact: true })).toBeFocused();
  await expect(block.locator(".block-details")).toHaveAttribute("open", "");
  await page.locator('[data-filter="gone"]').click();
  await page.locator('[data-filter="all"]').click();
  await expect(block.locator(".block-details")).toHaveAttribute("open", "");
  await expect(block.locator(".log-line")).toHaveCount(2000);
  expect((await exported(page)).result.findings.filter((f) => f.kind === "new")).toHaveLength(2100);
});
