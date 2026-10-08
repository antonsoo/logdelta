import patterns from "../../src/output/excerpt-patterns.json";
import { headAndTail, printable } from "./template";

const diagnostic = new RegExp(patterns.diagnostic, "iu");
const severity = new RegExp(patterns.severity, "iu");

export type ExcerptRow = { line: number } | { gap: number };

/** Same bounded selection as src/output/excerpt.rs; patterns and cases are shared. */
export function blockExcerpt(total: number, limit: number, raw: (index: number) => string): ExcerptRow[] {
  if (limit <= 0 || total <= limit) return Array.from({ length: total }, (_, line) => ({ line }));
  const { head, tail } = headAndTail(total, limit);
  const candidates: { rank: number; index: number }[] = [];
  for (let index = 0; index < total; index++) {
    const text = printable(raw(index));
    const rank = diagnostic.test(text) ? 2 : severity.test(text) ? 1 : 0;
    if (rank) candidates.push({ rank, index });
  }
  const selected = new Set<number>();
  if (candidates.length) {
    if (limit >= 2) selected.add(0);
    if (limit >= 3) {
      selected.add(total - 1);
      for (const { index } of candidates) {
        if (index < head || index >= total - tail) selected.add(index);
      }
    }
    candidates.sort((a, b) => b.rank - a.rank || b.index - a.index);
    for (const { index } of candidates) {
      if (selected.size === limit) break;
      selected.add(index);
    }
  }
  for (let i = 0; i < head && selected.size < limit; i++) selected.add(i);
  for (let i = total - 1; i >= total - tail && selected.size < limit; i--) selected.add(i);
  const rows: ExcerptRow[] = [];
  let next = 0;
  for (const line of [...selected].sort((a, b) => a - b)) {
    if (line > next) rows.push({ gap: line - next });
    rows.push({ line });
    next = line + 1;
  }
  if (next < total) rows.push({ gap: total - next });
  return rows;
}
