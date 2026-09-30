/// <reference lib="webworker" />
// Hosts the WebAssembly build of the logdelta library off the main thread: a diff of a few
// hundred thousand lines takes a second or two, and the page should stay responsive meanwhile.
import type { DiffRequest } from "./types";

interface Exports {
  memory: WebAssembly.Memory;
  ld_alloc(len: number): number;
  ld_diff(ptr: number, len: number): number;
  ld_result_ptr(): number;
  ld_result_len(): number;
}

export interface WorkerRequest {
  id: number;
  request: DiffRequest;
}

export type WorkerResponse = { id: number; ok: true; json: string; ms: number } | { id: number; ok: false; error: string };

let engine: Promise<Exports> | undefined;

function load(): Promise<Exports> {
  engine ??= WebAssembly.instantiateStreaming(fetch(`${import.meta.env.BASE_URL}logdelta.wasm`), {}).then(
    ({ instance }) => instance.exports as unknown as Exports,
  );
  return engine;
}

function diff(e: Exports, request: DiffRequest): { ok: boolean; json: string } {
  const bytes = new TextEncoder().encode(JSON.stringify(request));
  const ptr = e.ld_alloc(bytes.length);
  // Re-read memory.buffer after every call: allocation can grow the memory and detach old views.
  new Uint8Array(e.memory.buffer, ptr, bytes.length).set(bytes);
  const code = e.ld_diff(ptr, bytes.length);
  const json = new TextDecoder().decode(new Uint8Array(e.memory.buffer, e.ld_result_ptr(), e.ld_result_len()));
  return { ok: code === 0, json };
}

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, request } = event.data;
  try {
    const e = await load();
    const started = performance.now();
    const { ok, json } = diff(e, request);
    const ms = performance.now() - started;
    const reply: WorkerResponse = ok ? { id, ok, json, ms } : { id, ok: false, error: (JSON.parse(json) as { error: string }).error };
    self.postMessage(reply);
  } catch (err) {
    // A panic aborts the module (panic = "abort"); start from a fresh instance next time.
    engine = undefined;
    self.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) } satisfies WorkerResponse);
  }
};
