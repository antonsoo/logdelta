/// <reference lib="webworker" />
// Hosts the WebAssembly build of the logdelta library off the main thread: a diff of a few
// hundred thousand lines takes a second or two, and the page should stay responsive meanwhile.
import type { DiffRequest } from "./types";
import wasmURL from "./generated/logdelta.wasm?url";

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

export type WorkerResponse = { id: number; ok: true; json: string; ms: number; engineSha256: string } | { id: number; ok: false; error: string };

interface LoadedEngine { exports: Exports; sha256: string }
let engine: Promise<LoadedEngine> | undefined;

function load(): Promise<LoadedEngine> {
  engine ??= (async () => {
    const response = await fetch(wasmURL);
    if (!response.ok) throw new Error(`Could not load the diff engine (HTTP ${response.status}). Try comparing again.`);
    const bytes = await response.arrayBuffer();
    // Hash exactly the buffer passed to instantiate, rather than a second fetch
    // that could describe different bytes. The engine stays off the main thread.
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const sha256 = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
    const { instance } = await WebAssembly.instantiate(bytes, {});
    if (!(instance.exports.memory instanceof WebAssembly.Memory) || !["ld_alloc", "ld_diff", "ld_result_ptr", "ld_result_len"].every((name) => typeof instance.exports[name] === "function")) {
      throw new Error("The diff engine could not be initialized. Try comparing again or reload the page.");
    }
    return { exports: instance.exports as unknown as Exports, sha256 };
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

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, request } = event.data;
  try {
    const e = await load();
    const started = performance.now();
    const { ok, json } = diff(e.exports, request);
    const ms = performance.now() - started;
    const reply: WorkerResponse = ok ? { id, ok, json, ms, engineSha256: e.sha256 } : { id, ok: false, error: (JSON.parse(json) as { error: string }).error };
    self.postMessage(reply);
  } catch (err) {
    // A panic aborts the module (panic = "abort"); start from a fresh instance next time.
    engine = undefined;
    self.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) } satisfies WorkerResponse);
  }
};
