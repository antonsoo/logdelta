import { describe, expect, it } from "vitest";
import { comparisonInput, exportReport, type CompletedComparison, type LogInput } from "./report";
import { MAX_LOG_BYTES } from "./files";

const log = (text = "one\r\ntwo\n", name = "run.log"): LogInput => ({ text, name, origin: "file", ready: true });
const unused = (): LogInput => ({ text: "", name: "No log loaded", origin: "pasted", ready: false });
const capture = (baselines = [log()], target = log(), context = "2", masks = "") => comparisonInput(baselines, target, context, masks);

describe("comparison evidence", () => {
  it("captures source names, order and text independently of later edits", () => {
    const first = log("baseline\n", "first.log");
    const target = log("target\r\n", "target.log");
    const inputs = [first, unused(), log("", "empty.log")];
    const snapshot = capture(inputs, target, "3", " ID=ord_[a-z]+ \n\n");
    first.text = "changed"; first.name = "other.log";
    target.text = "changed"; target.name = "other-target.log";
    inputs.reverse();
    expect(snapshot.request).toEqual({ baselines: ["baseline\n", ""], target: "target\r\n", context: 3, masks: ["ID=ord_[a-z]+"] });
    expect(snapshot.sources.baselines.map((b) => [b.label, b.name, b.lines])).toEqual([["Baseline 1", "first.log", 1], ["Baseline 3", "empty.log", 0]]);
    expect(snapshot.sources.omitted_baselines).toEqual([2]);
    expect(snapshot.sources.target).toMatchObject({ name: "target.log", lines: 1, utf8_bytes: 8 });
  });
  it("accepts explicitly empty logs and whitespace, but rejects untouched inputs", () => {
    expect(capture([log("")], log("")).request).toMatchObject({ baselines: [""], target: "" });
    expect(capture([log("  \n")], log("\n")).sources.target.lines).toBe(1);
    expect(() => capture([unused()])).toThrow("Add at least one baseline");
    expect(() => capture([log()], unused())).toThrow("target log");
  });
  it.each(["", " ", "-1", "11", "2.5", "NaN", "Infinity"])("rejects invalid context %j rather than silently clamping it", (context) => {
    expect(() => capture([log()], log(), context)).toThrow("whole number");
  });
  it("bounds baseline count, decoded UTF-8 size and aggregate inputs", () => {
    expect(() => capture(Array.from({ length: 9 }, () => log()))).toThrow("8 baselines");
    expect(() => capture([log("x".repeat(MAX_LOG_BYTES + 1))])).toThrow("25 MiB");
    expect(() => capture([log("界".repeat(Math.floor(MAX_LOG_BYTES / 3) + 1))])).toThrow("25 MiB");
    expect(() => capture([log("a".repeat(MAX_LOG_BYTES)), log("b".repeat(MAX_LOG_BYTES))], log("c"))).toThrow("50 MiB");
  });
  it("bounds custom masks", () => {
    expect(() => capture([log()], log(), "2", "a\n".repeat(101))).toThrow("100 extra masks");
    expect(() => capture([log()], log(), "2", "a".repeat(65537))).toThrow("64 KiB");
  });
  it("exports applied settings and full findings, without raw full input logs or mutable UI state", () => {
    const input = capture([log("non-finding baseline secret\n")], log("non-finding target secret\n"), "0", "KEY=key_[a-z]+");
    const result = { baseline_totals: [1], target_total: 1, total_templates: 1, findings: [], blocks: [], value_findings: [] };
    const report: CompletedComparison = { ...input, outcome: { result, ms: 99 }, completedAt: "2026-10-03T00:00:00.000Z", revision: 123 };
    const exported = exportReport(report);
    expect(exported).toMatchObject({ format: "logdelta-report", schema_version: 1, settings: { context: 0, masks: ["KEY=key_[a-z]+"] }, result });
    expect(exported.sources.baselines[0]!.name).toBe("run.log");
    expect(JSON.stringify(exported)).not.toContain("non-finding");
    expect(exported).not.toHaveProperty("revision");
    expect(exported).not.toHaveProperty("request");
  });
});
