import type { DiffRequest, DiffResult } from "./types";
import type { WorkerRequest, WorkerResponse } from "./worker";

export interface DiffOutcome {
  result: DiffResult;
  /** Time spent inside the engine, excluding the one-off download of the module. */
  ms: number;
}

let worker: Worker | undefined;
let nextId = 1;
const pending = new Map<number, { resolve: (o: DiffOutcome) => void; reject: (e: Error) => void }>();

function getWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const reply = event.data;
    const waiter = pending.get(reply.id);
    if (!waiter) return;
    pending.delete(reply.id);
    if (reply.ok) waiter.resolve({ result: JSON.parse(reply.json) as DiffResult, ms: reply.ms });
    else waiter.reject(new Error(reply.error));
  };
  worker.onerror = (event) => {
    for (const waiter of pending.values()) waiter.reject(new Error(event.message || "the diff engine failed to load"));
    pending.clear();
    worker = undefined;
  };
  return worker;
}

export function runDiff(request: DiffRequest): Promise<DiffOutcome> {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    getWorker().postMessage({ id, request } satisfies WorkerRequest);
  });
}
