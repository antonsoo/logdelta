import type { DiffRequest, DiffResult } from "./types";
import type { WorkerRequest, WorkerResponse } from "./worker";
import { validRateEvidence } from "./rate-contract";

export interface DiffOutcome {
  result: DiffResult;
  /** Time spent inside the engine, excluding the one-off download of the module. */
  ms: number;
  /** SHA-256 of the exact bytes instantiated by this comparison's worker. */
  engineSha256: string;
}

interface Job {
  id: number;
  watchFields: string[];
  watchBy: string[];
  watchRateChange: number | undefined;
  resolve: (outcome: DiffOutcome) => void;
  reject: (error: unknown) => void;
  cleanup: () => void;
}

/** One active comparison. Termination is necessary to interrupt synchronous WASM work. */
export class DiffEngine {
  private worker: Worker | undefined;
  private job: Job | undefined;
  private nextId = 1;

  constructor(private readonly createWorker = () => new Worker(new URL("./worker.ts", import.meta.url), { type: "module" })) {}

  private finish(error: unknown, outcome?: DiffOutcome): void {
    const job = this.job;
    this.job = undefined;
    if (!job) return;
    job.cleanup();
    if (outcome) job.resolve(outcome);
    else job.reject(error);
  }

  cancel(reason: unknown = new DOMException("Comparison cancelled", "AbortError")): void {
    if (this.worker) {
      this.worker.onmessage = null;
      this.worker.onerror = null;
      this.worker.onmessageerror = null;
      this.worker.terminate();
      this.worker = undefined;
    }
    this.finish(reason);
  }

  private getWorker(): Worker {
    if (this.worker) return this.worker;
    const worker = this.createWorker();
    this.worker = worker;
    worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
      if (this.worker !== worker || !this.job) return;
      try {
        const reply = event.data;
        if (!reply || typeof reply.id !== "number") throw new Error("The diff engine returned an unreadable response. Try comparing again.");
        if (reply.id !== this.job.id) return;
        if (reply.ok === false && typeof reply.error === "string") this.finish(new Error(reply.error));
        else if (reply.ok === true && Number.isFinite(reply.ms) && reply.ms >= 0 && typeof reply.engineSha256 === "string" && /^[0-9a-f]{64}$/.test(reply.engineSha256)) {
          const result = JSON.parse(reply.json) as DiffResult | null;
          if (!result || !Array.isArray(result.baseline_totals) || !Array.isArray(result.findings) || !Array.isArray(result.blocks) || !Array.isArray(result.value_findings) || !Number.isSafeInteger(result.target_total) || !Number.isSafeInteger(result.total_templates)) {
            throw new Error("The diff engine returned an incomplete report. Try comparing again.");
          }
          const fields = result.watched_fields ?? [];
          const groupBy = this.job.watchBy;
          if (!Array.isArray(fields) || fields.length !== this.job.watchFields.length || fields.some((field, index) =>
            !field || field.pointer !== this.job!.watchFields[index] || typeof field.complete !== "boolean" ||
            !Array.isArray(field.group_by ?? []) || (field.group_by ?? []).length !== groupBy.length || (field.group_by ?? []).some((p, i) => p !== groupBy[i]) ||
            !Array.isArray(field.baselines) || field.baselines.length !== result.baseline_totals.length || !field.target ||
            !Array.isArray(field.values) || field.values.some((v) => !v || typeof v.value_json !== "string" ||
              !Array.isArray(v.group_values_json ?? []) || (v.group_values_json ?? []).length !== groupBy.length || (v.group_values_json ?? []).some((g) => typeof g !== "string") ||
              (groupBy.length && field.complete ? typeof v.group_seen_in_baseline !== "boolean" : v.group_seen_in_baseline !== undefined) ||
              !Array.isArray(v.baseline_counts) || v.baseline_counts.length !== result.baseline_totals.length ||
              !Number.isSafeInteger(v.target_count) || (field.complete ? typeof v.is_new !== "boolean" : v.is_new !== null)))) {
            throw new Error("The diff engine did not return the requested field evidence. Reload the page and compare again.");
          }
          if (fields.some((field) => !validRateEvidence(field, this.job!.watchRateChange))) {
            throw new Error("The diff engine did not return the requested rate evidence. Reload the page and compare again.");
          }
          this.finish(undefined, { result, ms: reply.ms, engineSha256: reply.engineSha256 });
        } else throw new Error("The diff engine returned an unreadable response. Try comparing again.");
      } catch (error) {
        this.cancel(error);
      }
    };
    worker.onerror = (event) => {
      event.preventDefault();
      if (this.worker === worker) this.cancel(new Error(event.message || "The diff engine failed to load. Try comparing again."));
    };
    worker.onmessageerror = () => {
      if (this.worker === worker) this.cancel(new Error("Could not read the diff engine response. Try comparing again."));
    };
    return worker;
  }

  run(request: DiffRequest, signal: AbortSignal): Promise<DiffOutcome> {
    if (signal.aborted) return Promise.reject(signal.reason);
    if (this.job) this.cancel();
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const abort = () => this.cancel(signal.reason);
      this.job = { id, watchFields: [...(request.watch_fields ?? [])], watchBy: [...(request.watch_by ?? [])], watchRateChange: request.watch_rate_change, resolve, reject, cleanup: () => signal.removeEventListener("abort", abort) };
      signal.addEventListener("abort", abort, { once: true });
      try {
        this.getWorker().postMessage({ id, request } satisfies WorkerRequest);
      } catch (error) {
        this.cancel(error);
      }
    });
  }
}

export const engine = new DiffEngine();
