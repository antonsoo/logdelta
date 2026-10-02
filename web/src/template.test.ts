import { describe, expect, it } from "vitest";
import { baselineCounts, headAndTail, lineCount, printable, templateParts } from "./template";

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

describe("printable", () => {
  const esc = String.fromCharCode(27);
  const bel = String.fromCharCode(7);

  it("returns ordinary text as it is", () => {
    expect(printable("plain\ttext with <angle> brackets")).toBe("plain\ttext with <angle> brackets");
  });

  it("drops color codes and hyperlinks", () => {
    expect(printable(`${esc}[31m${esc}[1mFAILED${esc}[0m test_a`)).toBe("FAILED test_a");
    expect(printable(`warning: ${esc}]8;;file:///src/a.rs${esc}\\src/a.rs${esc}]8;;${esc}\\ unused`)).toBe("warning: src/a.rs unused");
    expect(printable(`${esc}]0;building${bel}done`)).toBe("done");
  });

  it("turns a carriage return into a space and drops other control characters", () => {
    expect(printable(`10%\r20%${bel} done${esc}`)).toBe("10% 20% done");
  });
});

describe("headAndTail", () => {
  it("shows a short list whole", () => {
    expect(headAndTail(12, 12)).toEqual({ head: 12, tail: 0 });
    expect(headAndTail(500, 0)).toEqual({ head: 500, tail: 0 });
  });

  it("keeps the start and the end of a long one", () => {
    expect(headAndTail(88, 12)).toEqual({ head: 8, tail: 4 });
    expect(headAndTail(88, 36)).toEqual({ head: 24, tail: 12 });
  });
});
