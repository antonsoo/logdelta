import type { DiffRequest, DiffResult } from "./types";
import type { WorkerRequest, WorkerResponse } from "./worker";
import { validRateEvidence } from "./rate-contract";
import DiffWorker from "./worker?worker&inline";
import wasmURL from "./generated/logdelta.wasm?url";

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

/** One active comparison and independent file decoders. Termination interrupts WASM work. */
export class DiffEngine {
  private worker: Worker | undefined;
  private job: Job | undefined;
  private nextId = 1;
  // Executable bytes only. Cancelling a worker releases its log data and WASM
  // memory, while a replacement can initialize without the HTTP cache/network.
  private engineBytes: ArrayBuffer | undefined;

  constructor(private readonly createWorker = () => new DiffWorker()) {}

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
        else if (reply.ok === true && "json" in reply && typeof reply.json === "string" && Number.isFinite(reply.ms) && reply.ms >= 0 && typeof reply.engineSha256 === "string" && /^[0-9a-f]{64}$/.test(reply.engineSha256)) {
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
          this.rememberModule(reply.engineBytes);
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

  private rememberModule(bytes: ArrayBuffer | undefined): void {
    if (bytes === undefined) return;
    if (!(bytes instanceof ArrayBuffer) || bytes.byteLength === 0) {
      throw new Error("The diff engine returned unreadable module bytes. Try comparing again.");
    }
    this.engineBytes = bytes;
  }

  /** Each file owns a worker: baseline and target imports can finish independently. */
  gunzip(bytes: Uint8Array<ArrayBuffer>, signal: AbortSignal): Promise<Uint8Array<ArrayBuffer>> {
    if (signal.aborted) return Promise.reject(signal.reason);
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      let worker: Worker | undefined;
      let settled = false;
      const finish = (error: unknown, output?: ArrayBuffer) => {
        if (settled) return;
        settled = true;
        signal.removeEventListener("abort", abort);
        if (worker) {
          worker.onmessage = null;
          worker.onerror = null;
          worker.onmessageerror = null;
          worker.terminate(); // Release the file's compressed data and WASM memory.
        }
        if (output !== undefined) resolve(new Uint8Array(output));
        else reject(error);
      };
      const abort = () => finish(signal.reason);
      signal.addEventListener("abort", abort, { once: true });
      try {
        worker = this.createWorker();
        worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
          if (settled) return;
          const reply = event.data;
          if (!reply || typeof reply.id !== "number") { finish(new Error("The gzip decoder returned an unreadable response. Try opening the file again.")); return; }
          if (reply.id !== id) return;
          if (reply.ok === false && typeof reply.error === "string") finish(new Error(reply.error));
          else if (reply.ok === true && "bytes" in reply && reply.bytes instanceof ArrayBuffer &&
            Number.isFinite(reply.ms) && reply.ms >= 0 && typeof reply.engineSha256 === "string" && /^[0-9a-f]{64}$/.test(reply.engineSha256)) {
            try { this.rememberModule(reply.engineBytes); finish(undefined, reply.bytes); }
            catch (error) { finish(error); }
          } else finish(new Error("The gzip decoder returned an unreadable response. Try opening the file again."));
        };
        worker.onerror = (event) => { event.preventDefault(); finish(new Error(event.message || "The gzip decoder failed to load. Try opening the file again.")); };
        worker.onmessageerror = () => finish(new Error("Could not read the gzip decoder response. Try opening the file again."));
        const input = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes : bytes.slice();
        worker.postMessage({ id, gzip: input.buffer, engine: this.engineBytes ? { bytes: this.engineBytes } : { url: new URL(wasmURL, import.meta.url).href } } satisfies WorkerRequest, [input.buffer]);
      } catch (error) { finish(error); }
    });
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
        const fresh = !this.worker;
        this.getWorker().postMessage({ id, request, ...(fresh ? {
          engine: this.engineBytes ? { bytes: this.engineBytes } : { url: new URL(wasmURL, import.meta.url).href },
        } : {}) } satisfies WorkerRequest);
      } catch (error) {
        this.cancel(error);
      }
    });
  }
}

export const engine = new DiffEngine();
