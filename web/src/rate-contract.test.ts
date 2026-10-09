import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { validRateEvidence } from "./rate-contract";
import { comparisonInput, exportReport, parseRateChange, type CompletedComparison } from "./report";
import type { DiffResult } from "./types";

const native = (): DiffResult => JSON.parse(readFileSync(new URL("../../docs/field-rate-evidence/native.json", import.meta.url), "utf8"));

describe("rate evidence from the native HTTP capture", () => {
  it("accepts complete native evidence only for the setting that requested it", () => {
    const field = native().watched_fields![0]!;
    expect(validRateEvidence(field, 5)).toBe(true);
    expect(validRateEvidence(field, 10)).toBe(false);
    expect(validRateEvidence(field, undefined)).toBe(false);
    delete field.rate_comparison;
    expect(validRateEvidence(field, 5)).toBe(false);
    expect(validRateEvidence(field, undefined)).toBe(true);
  });
  it.each(["missing-group", "duplicate-group", "wrong-denominator", "wrong-value-index", "wrong-group", "missing-fractions", "invented-zero", "incomplete"])("rejects %s evidence", (failure) => {
    const field = native().watched_fields![0]!;
    const rates = field.rate_comparison!;
    const group = rates.groups[0]!;
    if (failure === "missing-group") rates.groups.pop();
    if (failure === "duplicate-group") rates.groups.push(structuredClone(group));
    if (failure === "wrong-denominator") group.target_total++;
    if (failure === "wrong-value-index") group.changes[0]!.value_index = 999;
    if (failure === "wrong-group") group.changes[0]!.value_index = 2; // Maintenance cannot supply a checkout change.
    if (failure === "missing-fractions") group.changes[0]!.baseline_rates.pop();
    if (failure === "invented-zero") group.status = "no_target_observations";
    if (failure === "incomplete") field.complete = false;
    expect(validRateEvidence(field, 5)).toBe(false);
  });
  it("permits unknown coverage without accepting partial denominators", () => {
    const field = native().watched_fields![0]!;
    field.complete = false;
    field.rate_comparison!.complete = false;
    field.rate_comparison!.groups = [];
    expect(validRateEvidence(field, 5)).toBe(true);
    field.rate_comparison!.complete = true;
    expect(validRateEvidence(field, 5)).toBe(false);
  });
  it("snapshots the applied threshold and preserves the full rates in a versioned report", () => {
    const log = { text: "{}", name: "log", origin: "file" as const, ready: true };
    const input = comparisonInput([log], log, "2", "", "/status", "/route", "5");
    const report: CompletedComparison = { ...input, revision: 1, completedAt: "2026-10-09T12:00:00Z", outcome: { result: native(), ms: 1, engineSha256: "a".repeat(64) } };
    const exported = exportReport(report);
    input.request.watch_rate_change = 50;
    expect(exported.schema_version).toBe(3);
    expect(exported.settings.watch_rate_change).toBe(5);
    expect(exported.result.watched_fields![0]!.rate_comparison!.groups).toHaveLength(2);
    expect(parseRateChange(undefined, false)).toBeUndefined();
    expect(() => parseRateChange("5", false)).toThrow("at least one watched field");
    for (const value of ["", " ", "0", "-5", "100.01", "NaN", "Infinity"]) expect(() => parseRateChange(value, true)).toThrow("greater than 0");
    expect(parseRateChange("100", true)).toBe(100);
    expect(parseRateChange("0.25", true)).toBe(0.25);
  });
});
