// Exercise the actual page and downloaded reports, including after disconnection.
// node studies/envelope-identity/replay-hosted.mjs BASE OUTPUT before|after CLI_REPLAY_DIR
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { resolve } from "node:path";
const { chromium, firefox } = createRequire(
    new URL("../../web/package.json", import.meta.url),
)("@playwright/test");
const [base, output, phase, nativeDirectory] = process.argv.slice(2);
assert(
    base && output && ["before", "after"].includes(phase) && nativeDirectory,
    "Expected BASE OUTPUT before|after CLI_REPLAY_DIR",
);
const cases = JSON.parse(
    await readFile(
        resolve(nativeDirectory, "..", "native-replay.json"),
        "utf8",
    ),
).cases;
const before = JSON.parse(
    await readFile(new URL("results/before.json", import.meta.url), "utf8"),
).cases;
const selected =
    phase === "before"
        ? cases.filter((c) =>
              ["native-grouped", "native-shadow-pooled"].includes(c.case),
          )
        : cases;
const stable = (value) =>
    JSON.parse(
        JSON.stringify(value, (_, v) =>
            typeof v === "number" && !Number.isInteger(v)
                ? Number(v.toPrecision(12))
                : v,
        ),
    );
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
await mkdir(output, { recursive: true });
const sessions = [];
for (const [engine, kind] of [
    ["chromium", chromium],
    ["firefox", firefox],
]) {
    const browser = await kind.launch();
    try {
        for (const width of [1440, 375]) {
            const context = await browser.newContext({
                viewport: { width, height: 1000 },
            });
            const page = await context.newPage();
            const errors = [],
                network = [],
                observations = [],
                engineTasks = [];
            context.on("response", (response) => {
                if (new URL(response.url()).pathname.endsWith(".wasm"))
                    engineTasks.push(
                        (async () => {
                            const bytes = await response.body();
                            return {
                                url: response.url(),
                                httpStatus: response.status(),
                                bytes: bytes.length,
                                sha256: sha256(bytes),
                            };
                        })(),
                    );
            });
            page.on("pageerror", (error) => errors.push(error.message));
            await page.goto(base);
            await page
                .getByRole("status")
                .filter({ hasText: "Comparison complete" })
                .waitFor();
            await page.evaluate(() => document.fonts.ready);
            const engineAssets = await Promise.all(engineTasks);
            assert.equal(
                engineAssets.length,
                1,
                "Exactly one engine must be observed before disconnection",
            );
            assert.equal(engineAssets[0].httpStatus, 200);
            await context.setOffline(true);
            page.on("request", (request) => {
                if (
                    ["http:", "https:"].includes(
                        new URL(request.url()).protocol,
                    )
                )
                    network.push(request.url());
            });
            await page.locator("#new-comparison").click();
            await page.locator(".options > summary").click();
            await page.locator("#context").fill("0");
            await page.locator(".options > summary").click();
            for (const entry of selected) {
                const getFlags = (flag) =>
                    entry.flags.flatMap((item, i) =>
                        item === flag ? [entry.flags[i + 1]] : [],
                    );
                const baseline = await readFile(
                    resolve(nativeDirectory, entry.case, "baseline.log"),
                    "utf8",
                );
                const target = await readFile(
                    resolve(nativeDirectory, entry.case, "target.log"),
                    "utf8",
                );
                await page.locator("#baseline").fill(baseline);
                await page.locator("#target").fill(target);
                await page
                    .locator("#watch-fields")
                    .fill(getFlags("--watch-field").join("\n"));
                await page
                    .locator("#watch-by")
                    .fill(getFlags("--watch-by").join("\n"));
                const rate = getFlags("--watch-rate-change")[0];
                await page
                    .locator("#watch-rates")
                    .setChecked(rate !== undefined);
                if (rate !== undefined)
                    await page.locator("#watch-rate-change").fill(rate);
                await page.locator("#run").click();
                await page
                    .getByRole("status")
                    .filter({
                        hasText:
                            entry.kind === "incomplete"
                                ? "Field comparison incomplete"
                                : "Comparison complete",
                    })
                    .waitFor();
                const pending = page.waitForEvent("download");
                await page
                    .getByRole("button", {
                        name: "Download report",
                        exact: true,
                    })
                    .click();
                const bytes = await readFile(await (await pending).path());
                const actual = JSON.parse(bytes);
                const field = actual.result.watched_fields[0];
                const cards = await page.locator(".kind-field").count();
                if (phase === "after") {
                    const expected = JSON.parse(
                        await readFile(
                            resolve(nativeDirectory, entry.case, "report.json"),
                            "utf8",
                        ),
                    );
                    assert.deepEqual(
                        stable(actual.result),
                        stable(expected),
                        `${engine}/${width}/${entry.case}`,
                    );
                } else {
                    assert.deepEqual(
                        field,
                        before.find((c) => c.case === entry.case).watch,
                    );
                    assert.equal(field.complete, true);
                    assert.equal(
                        field.values.some((v) => v.value_json === "503"),
                        false,
                    );
                    assert.equal(cards, 0);
                }
                const retain = [
                    "native-grouped",
                    "native-shadow-pooled",
                ].includes(entry.case);
                const file = `${engine}-${width}-${entry.case}.json`;
                if (retain) await writeFile(resolve(output, file), bytes);
                observations.push({
                    case: entry.case,
                    kind: entry.kind,
                    baselineRecords: baseline.trimEnd().split("\n").length,
                    targetRecords: target.trimEnd().split("\n").length,
                    complete: field.complete,
                    matched: field.target.matched,
                    fieldFindingCards: cards,
                    reportBytes: bytes.length,
                    reportSha256: sha256(bytes),
                    fullResultMatchesFreshNative: phase === "after",
                    ...(retain ? { download: file } : {}),
                });
                if (entry.case === "native-grouped") {
                    if (phase === "after") {
                        await page.locator(".field-evidence > summary").click();
                        await page.locator(".field-sources > summary").click();
                        assert.match(
                            await page.locator(".field-sources").innerText(),
                            /Target, line 2/,
                        );
                    }
                    await page.locator("#report-content").screenshot({
                        path: resolve(output, `${engine}-${width}.png`),
                        animations: "disabled",
                    });
                }
                assert.equal(
                    await page.evaluate(
                        () =>
                            document.documentElement.scrollWidth <=
                            innerWidth + 1,
                    ),
                    true,
                );
            }
            assert.deepEqual(errors, []);
            if (phase === "after") assert.deepEqual(network, []);
            sessions.push({
                base,
                phase,
                engine,
                browserVersion: browser.version(),
                width,
                offlineBeforeFirstNativeComparison: true,
                engineAssets,
                errors,
                network,
                noPageOverflow: true,
                observations,
            });
            await context.close();
        }
    } finally {
        await browser.close();
    }
}
await writeFile(
    resolve(output, "receipt.json"),
    JSON.stringify(
        {
            phase,
            node: process.version,
            fullResultComparison:
                "Integers, strings and evidence exact; floating scores at 12 significant digits.",
            sessions,
        },
        null,
        2,
    ) + "\n",
);
console.log(
    `${phase}: ${sessions.reduce((n, s) => n + s.observations.length, 0)} actual offline browser comparisons`,
);
