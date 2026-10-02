// The JSON the wasm engine returns: serde's rendering of logdelta's analysis types
// (src/analysis.rs and src/context.rs in the crate), kept field-for-field.

export type FindingKind = "new" | "gone" | "changed";
export type Direction = "up" | "down" | "flat";

export interface ContextWindow {
  /** [line number, raw text], oldest first. */
  before: [number, string][];
  after: [number, string][];
}

export interface Finding {
  kind: FindingKind;
  direction: Direction;
  template: string;
  score: number;
  baseline_counts: number[];
  target_count: number;
  first_target_line_no: number | null;
  first_target_raw: string | null;
  /** Gone findings only: the template's first line in the first baseline. */
  first_baseline_line_no?: number;
  first_baseline_raw?: string;
  /** Index into DiffResult.blocks when the finding is part of a block. */
  block?: number;
  context?: ContextWindow;
}

/** Findings that are one event in the log: a traceback, the steps a failed job skipped. */
export interface Block {
  kind: FindingKind;
  /** Lines of the target for a new block, of the first baseline for a gone one. */
  first_line_no: number;
  last_line_no: number;
  /** Lines its templates account for in that run, wherever they are. */
  line_count: number;
  /** Of those, the ones outside first_line_no..last_line_no. */
  lines_elsewhere: number;
  score: number;
  /** Indexes into DiffResult.findings, in line order. */
  findings: number[];
}

export interface ValueFinding {
  template: string;
  position: number;
  new_value: string;
  baseline_values: string[];
  first_target_line_no: number;
  first_target_raw: string;
  established: number;
  /** Index into DiffResult.blocks when the line is inside a new block. */
  block?: number;
  context?: ContextWindow;
}

export interface DiffResult {
  baseline_totals: number[];
  target_total: number;
  total_templates: number;
  findings: Finding[];
  blocks: Block[];
  value_findings: ValueFinding[];
}

export interface DiffRequest {
  baselines: string[];
  target: string;
  context: number;
  masks: string[];
}
