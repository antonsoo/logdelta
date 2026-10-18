/// <reference lib="webworker" />
// Hosts the WebAssembly build of the logdelta library off the main thread: a diff of a few
// hundred thousand lines takes a second or two, and the page should stay responsive meanwhile.
import type { DiffRequest } from "./types";

interface Exports {
  memory: WebAssembly.Memory;
  ld_alloc(len: number): number;
  ld_diff(ptr: number, len: number): number;
  ld_gunzip?(ptr: number, len: number): number;
  ld_result_ptr(): number;
  ld_result_len(): number;
}

export type EngineSource = { url: string } | { bytes: ArrayBuffer };
export type WorkerRequest = { id: number; engine?: EngineSource } &
  ({ request: DiffRequest } | { gzip: ArrayBuffer });

export type WorkerResponse = ({ id: number; ok: true; ms: number; engineSha256: string; engineBytes?: ArrayBuffer } &
  ({ json: string } | { bytes: ArrayBuffer })) | { id: number; ok: false; error: string };

interface LoadedEngine { exports: Exports; sha256: string; bytes: ArrayBuffer }
let engine: Promise<LoadedEngine> | undefined;
let source: WorkerRequest["engine"];
let announced = false;

function load(): Promise<LoadedEngine> {
  engine ??= (async () => {
    let bytes: ArrayBuffer;
    if (source && "bytes" in source) bytes = source.bytes;
    else {
      if (!source) throw new Error("The diff engine source is missing. Reload the page and compare again.");
      const response = await fetch(source.url);
      if (!response.ok) throw new Error(`Could not load the diff engine (HTTP ${response.status}). Try comparing again.`);
      bytes = await response.arrayBuffer();
    }
    // Hash exactly the buffer passed to instantiate, rather than a second fetch
    // that could describe different bytes. The engine stays off the main thread.
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const sha256 = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
    const { instance } = await WebAssembly.instantiate(bytes, {});
    if (!(instance.exports.memory instanceof WebAssembly.Memory) || !["ld_alloc", "ld_diff", "ld_result_ptr", "ld_result_len"].every((name) => typeof instance.exports[name] === "function")) {
      throw new Error("The diff engine could not be initialized. Try comparing again or reload the page.");
    }
    // Only retain an initialized module. Invalid downloads must remain retryable.
    // Keep the bytes in this worker too, so a trapped instance can restart offline.
    source = { bytes };
    return { exports: instance.exports as unknown as Exports, sha256, bytes };
  })();
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

function gunzip(e: Exports, bytes: ArrayBuffer): ArrayBuffer {
  if (typeof e.ld_gunzip !== "function") throw new Error("This engine cannot open gzip. Reload the page to load the current engine.");
  const ptr = e.ld_alloc(bytes.byteLength);
  new Uint8Array(e.memory.buffer, ptr, bytes.byteLength).set(new Uint8Array(bytes));
  const code = e.ld_gunzip(ptr, bytes.byteLength);
  const output = new Uint8Array(e.memory.buffer, e.ld_result_ptr(), e.ld_result_len());
  if (code !== 0) throw new Error((JSON.parse(new TextDecoder().decode(output)) as { error: string }).error);
  // Copy out of WASM memory; only this bounded result buffer is transferred to the page.
  return new Uint8Array(output).buffer;
}

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id } = event.data;
  if (event.data.engine) source = event.data.engine;
  try {
    const e = await load();
    const started = performance.now();
    const metadata = () => ({ id, ok: true as const, ms: performance.now() - started, engineSha256: e.sha256, ...(!announced ? { engineBytes: e.bytes } : {}) });
    if ("gzip" in event.data) {
      const bytes = gunzip(e.exports, event.data.gzip);
      self.postMessage({ ...metadata(), bytes } satisfies WorkerResponse, [bytes]);
      announced = true;
    } else {
      const { ok, json } = diff(e.exports, event.data.request);
      self.postMessage(ok ? { ...metadata(), json } satisfies WorkerResponse : { id, ok: false, error: (JSON.parse(json) as { error: string }).error } satisfies WorkerResponse);
      if (ok) announced = true;
    }
  } catch (err) {
    // A panic aborts the module (panic = "abort"); start from a fresh instance next time.
    engine = undefined;
    self.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) } satisfies WorkerResponse);
  }
};
