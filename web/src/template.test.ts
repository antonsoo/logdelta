import { describe, expect, it } from "vitest";
import { baselineCounts, lineCount, templateParts } from "./template";

describe("templateParts", () => {
  it("splits slots from literal text and records their token index", () => {
    expect(templateParts("GET /api/v1/orders/<NUM> took <DUR> status=<*>")).toEqual([
      { kind: "text", text: "GET /api/v1/orders/" },
      { kind: "slot", text: "<NUM>", name: "number", token: 1 },
      { kind: "text", text: " took " },
      { kind: "slot", text: "<DUR>", name: "duration", token: 3 },
      { kind: "text", text: " status=" },
      { kind: "slot", text: "<*>", name: "varies", token: 4 },
    ]);
  });

  it("leaves text that only looks like markup alone", () => {
    expect(templateParts("<div> <html>")).toEqual([{ kind: "text", text: "<div> <html>" }]);
  });
});

describe("baselineCounts", () => {
  it("lists one count per baseline", () => {
    expect(baselineCounts([0])).toBe("0 in the baseline");
    expect(baselineCounts([1200, 14, 13])).toBe("1,200 · 14 · 13 in the baselines");
  });
});

describe("lineCount", () => {
  it("counts lines like Rust's str::lines", () => {
    expect(lineCount("")).toBe(0);
    expect(lineCount("a")).toBe(1);
    expect(lineCount("a\nb\n")).toBe(2);
    expect(lineCount("a\n\nb")).toBe(3);
  });
});
