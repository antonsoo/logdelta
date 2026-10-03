import { version } from "../package.json";
import { MAX_BASELINES, MAX_COMPARISON_BYTES, MAX_LOG_BYTES, textBytes } from "./files";
import { lineCount } from "./template";
import type { DiffOutcome } from "./engine";
import type { DiffRequest } from "./types";

export interface LogInput {
  text: string;
  name: string;
  origin: "file" | "example" | "pasted" | "edited" | "empty";
  /** An empty file is a real input; an unused editor is not. */
  ready: boolean;
}

export interface SourceInfo {
  label: string;
  name: string;
  origin: LogInput["origin"];
  lines: number;
  utf8_bytes: number;
}

export interface ComparisonInput {
  request: DiffRequest;
  sources: { baselines: SourceInfo[]; target: SourceInfo; omitted_baselines: number[] };
}

export interface CompletedComparison extends ComparisonInput {
  outcome: DiffOutcome;
  completedAt: string;
  revision: number;
}

/** Capture text and metadata together, before crossing any asynchronous boundary. */
export function comparisonInput(baselines: readonly LogInput[], target: LogInput, context: string, masks: string): ComparisonInput {
  if (baselines.length > MAX_BASELINES) throw new Error("The browser supports up to 8 baselines. Use the CLI for more runs.");
  if (!baselines.some((b) => b.ready) || !target.ready) throw new Error("Add at least one baseline and a target log. To compare an empty log, open an empty file or choose Use empty log.");
  const n = Number(context);
  if (context.trim() === "" || !Number.isInteger(n) || n < 0 || n > 10) throw new Error("Context lines must be a whole number from 0 to 10.");
  let total = 0;
  const describe = (source: LogInput, label: string): SourceInfo => {
    // Check code units first so a giant paste can be rejected without walking every character.
    if (source.text.length > MAX_LOG_BYTES) throw new Error(`${label} exceeds the 25 MiB browser limit. Use the CLI for larger logs.`);
    const size = textBytes(source.text);
    if (size > MAX_LOG_BYTES) throw new Error(`${label} exceeds the 25 MiB browser limit. Use the CLI for larger logs.`);
    total += size;
    if (total > MAX_COMPARISON_BYTES) throw new Error("The comparison exceeds the 50 MiB total browser limit. Use the CLI for larger logs.");
    return { label, name: source.name, origin: source.origin, lines: lineCount(source.text), utf8_bytes: size };
  };
  const sources: ComparisonInput["sources"] = { baselines: [], target: describe(target, "Target"), omitted_baselines: [] };
  const texts: string[] = [];
  baselines.forEach((source, index) => {
    if (!source.ready) sources.omitted_baselines.push(index + 1);
    else {
      sources.baselines.push(describe(source, `Baseline ${index + 1}`));
      texts.push(source.text);
    }
  });
  const extraMasks = masks.split("\n").map((m) => m.trim()).filter(Boolean);
  if (extraMasks.length > 100 || textBytes(masks) > 64 * 1024) throw new Error("Use at most 100 extra masks and 64 KiB of mask text in the browser.");
  return { request: { baselines: texts, target: target.text, context: n, masks: extraMasks }, sources };
}

/** A portable report, without full input logs. The findings themselves contain log excerpts. */
export function exportReport(report: CompletedComparison) {
  return {
    format: "logdelta-report",
    schema_version: 1,
    engine: { name: "logdelta", version },
    completed_at: report.completedAt,
    settings: { context: report.request.context, masks: [...report.request.masks] },
    sources: report.sources,
    result: report.outcome.result,
  };
}
