import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { cp, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import { test, expect } from "@playwright/test";
import { build } from "vite";

const web = fileURLToPath(new URL("../../", import.meta.url));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const mime = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".woff2": "font/woff2", ".log": "text/plain" };

test("warm legacy cache cannot supply the engine of a subsequent production build", async ({ page }) => {
  test.skip(!!process.env.LOGDELTA_BASE_URL, "Local two-build/cache fixture; deployed workflows are in workspace.spec.mjs.");
  test.setTimeout(120_000);
  const directory = await mkdtemp(join(tmpdir(), "logdelta-cache-"));
  const original = await readFile(join(web, "src/generated/logdelta.wasm"));
  // A harmless, valid custom section changes the real module's bytes without
  // changing the Rust algorithm or ABI. Both variants are production Vite builds.
  const name = Buffer.from("cache-fixture-second-build");
  const second = Buffer.concat([original, Buffer.from([0, name.length + 1, name.length]), name]);
  expect(WebAssembly.validate(second)).toBe(true);
  const expected = [digest(original), digest(second)];
  const snapshots = [];
  const modules = [];
  let phase = 0;
  let legacyRequests = 0;
  let server;
  const errors = [];
  const external = [];
  try {
    for (const [index, bytes] of [original, second].entries()) {
      const root = join(directory, String(index));
      await mkdir(root);
      for (const path of ["src", "public", "index.html", "package.json", "vite.config.ts", "vite.csp.ts"]) await cp(join(web, path), join(root, path), { recursive: true });
      await symlink(join(web, "node_modules"), join(root, "node_modules"), "dir");
      await writeFile(join(root, "src/generated/logdelta.wasm"), bytes);
      await build({ root, logLevel: "silent" });
      snapshots.push(join(root, "dist"));
    }
    server = createServer(async (request, response) => {
      try {
        const relative = decodeURIComponent(new URL(request.url, "http://localhost").pathname).replace(/^\/logdelta\//, "") || "index.html";
        const legacy = relative === "logdelta.wasm";
        const path = resolve(snapshots[phase], relative);
        if (!path.startsWith(`${snapshots[phase]}${sep}`)) { response.writeHead(400); response.end(); return; }
        const bytes = legacy ? [original, second][phase] : await readFile(path);
        if (legacy) legacyRequests++;
        else if (relative.endsWith(".wasm")) modules.push({ phase, path: relative, sha256: digest(bytes) });
        response.writeHead(200, { "content-type": mime[extname(relative)] ?? "application/octet-stream", "cache-control": relative.endsWith(".wasm") ? "public, max-age=3600, immutable" : "no-store" });
        response.end(bytes);
      } catch { response.writeHead(404); response.end("Not found"); }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const site = `http://127.0.0.1:${server.address().port}/logdelta/`;
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => { if (new URL(request.url()).origin !== new URL(site).origin) external.push(request.url()); });
    await page.addInitScript(() => {
      window.cacheCspViolations = [];
      document.addEventListener("securitypolicyviolation", (event) => window.cacheCspViolations.push(event.violatedDirective));
    });
    const compareResult = async () => {
      await expect(page.getByRole("status")).toContainText("Comparison complete");
      const pending = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download report", exact: true }).click();
      const download = await pending;
      return JSON.parse(await readFile(await download.path(), "utf8"));
    };
    const legacyDigest = () => page.evaluate(async () => {
      const bytes = await (await fetch("/logdelta/logdelta.wasm")).arrayBuffer();
      return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (byte) => byte.toString(16).padStart(2, "0")).join("");
    });
    await page.goto(site);
    const firstReport = await compareResult();
    expect(firstReport.engine.wasm_sha256).toBe(expected[0]);
    expect(await legacyDigest()).toBe(expected[0]);
    expect(legacyRequests).toBe(1);
    expect(await page.evaluate(() => window.cacheCspViolations)).toEqual([]);

    phase = 1;
    await page.goto(`${site}?second-build`);
    const secondReport = await compareResult();
    expect(secondReport.result).toEqual(firstReport.result);
    expect(secondReport.engine.wasm_sha256).toBe(expected[1]);
    // This proves a real browser cache still serves the old fixed-name bytes.
    // The new production worker succeeds because its URL is a different key.
    expect(await legacyDigest()).toBe(expected[0]);
    expect(legacyRequests).toBe(1);
    expect(modules.map((module) => module.sha256)).toEqual(expected);
    expect(modules[0].path).not.toBe(modules[1].path);
    expect(await page.evaluate(() => window.cacheCspViolations)).toEqual([]);
    expect(errors).toEqual([]);
    expect(external).toEqual([]);
  } finally {
    await page.goto("about:blank");
    if (server) await new Promise((resolve) => { server.close(resolve); server.closeAllConnections(); });
    await rm(directory, { recursive: true, force: true });
  }
});
