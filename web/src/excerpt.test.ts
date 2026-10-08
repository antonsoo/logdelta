import { expect, it } from "vitest";
import cases from "../../tests/excerpt-cases.json";
import { blockExcerpt } from "./excerpt";

for (const example of cases) {
  it(`shares native excerpt choices: ${example.name}`, () => {
    const raw = (i: number) => example.lines[i]!;
    for (const preview of example.previews) {
      const rows = blockExcerpt(example.lines.length, preview.limit, raw);
      expect(rows.flatMap((row) => "line" in row ? [row.line] : [])).toEqual(preview.lines);
      let next = 0;
      for (const row of rows) {
        if ("line" in row) { expect(row.line).toBe(next); next++; }
        else { expect(row.gap).toBeGreaterThan(0); next += row.gap; }
      }
      expect(next).toBe(example.lines.length);
    }
    for (const limit of [0, example.lines.length, example.lines.length + 1]) {
      expect(blockExcerpt(example.lines.length, limit, raw)).toEqual(example.lines.map((_, line) => ({ line })));
    }
  });
}
