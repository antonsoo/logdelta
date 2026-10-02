// Pure helpers for rendering templates and counts; no DOM, so they're unit-tested directly.

/** One piece of a template: literal text, or a masked slot such as <NUM> or Drain's <*>. */
export type TemplatePart = { kind: "text"; text: string } | { kind: "slot"; text: string; name: string; token: number };

const SLOT = /<(\*|[A-Z][A-Z0-9_]*)>/g;

// What each placeholder in src/mask.rs stands for; Drain's <*> marks a position whose value varies.
const SLOT_NAMES: Record<string, string> = {
  "*": "varies",
  TS: "timestamp",
  NUM: "number",
  UUID: "UUID",
  IP: "IP address",
  HEX: "hex id",
  ID: "id",
  EMAIL: "email",
  DUR: "duration",
  QTY: "quantity",
  TMPPATH: "temp path",
  CUSTOM: "custom mask",
};

function slotName(key: string): string {
  return SLOT_NAMES[key] ?? key.toLowerCase();
}

/**
 * Splits a template into literal text and slots, remembering which whitespace-separated token
 * each slot sits in - a NEW VALUE finding names its flipped position by that token index.
 */
export function templateParts(template: string): TemplatePart[] {
  const parts: TemplatePart[] = [];
  const tokens = template.split(" ");
  tokens.forEach((token, index) => {
    if (index > 0) pushText(parts, " ");
    let last = 0;
    for (const match of token.matchAll(SLOT)) {
      const at = match.index ?? 0;
      if (at > last) pushText(parts, token.slice(last, at));
      parts.push({ kind: "slot", text: match[0], name: slotName(match[1]!), token: index });
      last = at + match[0].length;
    }
    if (last < token.length) pushText(parts, token.slice(last));
  });
  return parts;
}

function pushText(parts: TemplatePart[], text: string): void {
  const previous = parts[parts.length - 1];
  if (previous?.kind === "text") previous.text += text;
  else parts.push({ kind: "text", text });
}

/** "0 in the baselines", "12 · 14 · 13 in the baselines" (one count per baseline run). */
export function baselineCounts(counts: number[]): string {
  if (counts.length === 0) return "no baselines";
  const plural = counts.length === 1 ? "baseline" : "baselines";
  return `${counts.map(formatCount).join(" · ")} in the ${plural}`;
}

export function formatCount(n: number): string {
  return n.toLocaleString("en-US");
}

/** Number of lines in pasted text, counted the way the engine counts them (str::lines). */
export function lineCount(text: string): number {
  if (text.length === 0) return 0;
  const n = text.split("\n").length;
  return text.endsWith("\n") ? n - 1 : n;
}

// Built from character codes so the source holds no control characters.
const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);
// CSI (colors, cursor movement) and OSC (titles, hyperlinks) sequences, as src/mask.rs strips them.
const ESCAPES = new RegExp(`${ESC}\\[[0-9;?]*[ -/]*[@-~]|${ESC}\\][^${BEL}${ESC}]*(?:${BEL}|${ESC}\\\\)?`, "g");

function isControl(code: number): boolean {
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

/**
 * A log line as the page shows it: without terminal escape sequences or control characters
 * (a tab stays, a line break becomes a space). Same rule as `printable` in src/output/mod.rs.
 */
export function printable(text: string): string {
  let clean = true;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code !== 9 && isControl(code)) {
      clean = false;
      break;
    }
  }
  if (clean) return text;
  let out = "";
  for (const ch of text.replace(ESCAPES, "")) {
    if (ch === "\n" || ch === "\r") out += " ";
    else if (ch === "\t" || !isControl(ch.charCodeAt(0))) out += ch;
  }
  return out;
}

/**
 * Which of `total` rows to show when at most `limit` fit: all of them, or the first two thirds
 * and the last third. Same split as `head_and_tail` in src/output/mod.rs.
 */
export function headAndTail(total: number, limit: number): { head: number; tail: number } {
  if (limit <= 0 || total <= limit) return { head: total, tail: 0 };
  const tail = Math.floor(limit / 3);
  return { head: limit - tail, tail };
}
