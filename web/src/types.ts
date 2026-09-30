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
  context?: ContextWindow;
}

export interface ValueFinding {
  template: string;
  position: number;
  new_value: string;
  baseline_values: string[];
  first_target_line_no: number;
  first_target_raw: string;
  established: number;
  context?: ContextWindow;
}

export interface DiffResult {
  baseline_totals: number[];
  target_total: number;
  total_templates: number;
  findings: Finding[];
  value_findings: ValueFinding[];
}

export interface DiffRequest {
  baselines: string[];
  target: string;
  context: number;
  masks: string[];
}
