import { formatCount, printable } from "./template";
import type { ContextWindow } from "./types";

export function esc(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

export function logLine(no: number, text: string, cls: string, times = 1): string {
  const repeat = times > 1 ? `<span class="times">×${formatCount(times)}</span>` : "";
  return `<div class="log-line ${cls}"><span class="gutter">${formatCount(no)}</span><span class="log-text">${esc(printable(text))}</span>${repeat}</div>`;
}

export function contextHtml(lineNo: number | null, raw: string | null, context: ContextWindow | undefined): string {
  if (lineNo === null || raw === null) return "";
  return `<div class="log" tabindex="0" role="group" aria-label="Log lines">${(context?.before ?? []).map(([n, t]) => logLine(n, t, "is-context")).join("")}${logLine(lineNo, raw, "is-hit")}${(context?.after ?? []).map(([n, t]) => logLine(n, t, "is-context")).join("")}</div>`;
}
