import { describe, expect, it, vi } from "vitest";
import { DiffEngine } from "./engine";
import type { WorkerRequest, WorkerResponse } from "./worker";

const request = { baselines: ["ok\n"], target: "failed\n", context: 2, masks: [] };
const result = { baseline_totals: [1], target_total: 1, total_templates: 2, findings: [], blocks: [], value_findings: [] };
const engineSha256 = "a".repeat(64);
class FakeWorker {
  onmessage: ((e: MessageEvent<WorkerResponse>) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  terminate = vi.fn();
  postMessage = vi.fn<(message: WorkerRequest) => void>();
  reply(data: Partial<WorkerResponse> = {}) {
    this.onmessage?.({ data: { id: this.postMessage.mock.calls.at(-1)![0].id, ok: true, json: JSON.stringify(result), ms: 4, engineSha256, ...data } } as MessageEvent<WorkerResponse>);
  }
}
function setup() {
  const workers: FakeWorker[] = [];
  const factory = vi.fn(() => { const w = new FakeWorker(); workers.push(w); return w as unknown as Worker; });
  const engine = new DiffEngine(factory);
  const run = (controller = new AbortController()) => engine.run(request, controller.signal);
  return { workers, factory, engine, run };
}

describe("diff worker ownership", () => {
  it("keeps only successful module bytes across termination and sends them only to fresh workers", async () => {
    const { engine, workers, run } = setup();
    const bytes = new Uint8Array([0, 97, 115, 109]).buffer;
    const first = run();
    expect(workers[0]!.postMessage.mock.calls[0]![0].engine).toHaveProperty("url");
    workers[0]!.reply({ engineBytes: bytes });
    await first;
    const reused = run();
    expect(workers[0]!.postMessage.mock.calls[1]![0].engine).toBeUndefined();
    workers[0]!.reply();
    await reused;
    engine.cancel();
    const controller = new AbortController();
    const cancelled = run(controller);
    const stale = workers[1]!.onmessage!;
    expect(workers[1]!.postMessage.mock.calls[0]![0].engine).toEqual({ bytes });
    controller.abort();
    await expect(cancelled).rejects.toMatchObject({ name: "AbortError" });
    const fresh = run();
    stale({ data: { id: 4, ok: true, json: JSON.stringify(result), ms: 1, engineSha256, engineBytes: new ArrayBuffer(1) } } as MessageEvent<WorkerResponse>);
    expect(workers[2]!.postMessage.mock.calls[0]![0].engine).toEqual({ bytes });
    workers[2]!.reply();
    await fresh;
  });
  it("does not retain module bytes from an invalid report", async () => {
    const { workers, run } = setup();
    const first = run();
    workers[0]!.reply({ json: "null", engineBytes: new ArrayBuffer(8) });
    await expect(first).rejects.toThrow("incomplete report");
    const retry = run();
    expect(workers[1]!.postMessage.mock.calls[0]![0].engine).toHaveProperty("url");
    workers[1]!.reply();
    await retry;
  });
  it.each(["omitted-groups", "wrong-pointer", "wrong-key-count", "missing-group-history", "nonstring-key"])("refuses a grouped comparison with %s evidence", async (failure) => {
    const { engine, workers } = setup();
    const pending = engine.run({ ...request, watch_fields: ["/status"], watch_by: ["/route"] }, new AbortController().signal);
    const field = { pointer: "/status", complete: true, baselines: [{}], target: {}, group_by: ["/route"], values: [{ value_json: "200", baseline_counts: [1], target_count: 1, is_new: false, group_values_json: ['"/checkout"'], group_seen_in_baseline: true }] };
    const malformed = JSON.parse(JSON.stringify(field));
    if (failure === "omitted-groups") { delete malformed.group_by; delete malformed.values[0].group_values_json; delete malformed.values[0].group_seen_in_baseline; }
    if (failure === "wrong-pointer") malformed.group_by = ["/service"];
    if (failure === "wrong-key-count") malformed.values[0].group_values_json = [];
    if (failure === "missing-group-history") delete malformed.values[0].group_seen_in_baseline;
    if (failure === "nonstring-key") malformed.values[0].group_values_json = [200];
    workers[0]!.reply({ json: JSON.stringify({ ...result, watched_fields: [malformed] }) });
    await expect(pending).rejects.toThrow("requested field evidence");
    // A retry can use a fresh engine, preserving the same explicit grouping contract.
    const retry = engine.run({ ...request, watch_fields: ["/status"], watch_by: ["/route"] }, new AbortController().signal);
    workers[1]!.reply({ json: JSON.stringify({ ...result, watched_fields: [field] }) });
    await expect(retry).resolves.toMatchObject({ result: { watched_fields: [field] } });
  });
  it("rejects an engine reply that silently omits a requested watch", async () => {
    const { engine, workers } = setup();
    const pending = engine.run({ ...request, watch_fields: ["/status"] }, new AbortController().signal);
    workers[0]!.reply();
    await expect(pending).rejects.toThrow("requested field evidence");
    expect(workers[0]!.terminate).toHaveBeenCalledOnce();
  });
  it("reuses a healthy worker and removes completed jobs' abort listeners", async () => {
    const { workers, run, factory } = setup();
    const firstController = new AbortController();
    const first = run(firstController);
    workers[0]!.reply();
    await expect(first).resolves.toEqual({ result, ms: 4, engineSha256 });
    const second = run();
    firstController.abort();
    expect(workers[0]!.terminate).not.toHaveBeenCalled();
    workers[0]!.reply();
    await expect(second).resolves.toEqual({ result, ms: 4, engineSha256 });
    expect(factory).toHaveBeenCalledOnce();
  });
  it("terminates synchronous work on cancellation and retries with a fresh worker", async () => {
    const { workers, run } = setup();
    const controller = new AbortController();
    const first = run(controller);
    controller.abort();
    await expect(first).rejects.toMatchObject({ name: "AbortError" });
    expect(workers[0]!.terminate).toHaveBeenCalledOnce();
    const second = run();
    workers[1]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
  it("a newer request supersedes pending work; queued old replies cannot settle it", async () => {
    const { workers, run } = setup();
    const first = run();
    const stale = workers[0]!.onmessage!;
    const firstRejected = expect(first).rejects.toMatchObject({ name: "AbortError" });
    const second = run();
    await firstRejected;
    stale({ data: { id: 2, ok: false, error: "stale failure" } } as MessageEvent<WorkerResponse>);
    workers[1]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
  it("does not start a cancelled request", async () => {
    const { run, factory } = setup();
    const controller = new AbortController(); controller.abort();
    await expect(run(controller)).rejects.toMatchObject({ name: "AbortError" });
    expect(factory).not.toHaveBeenCalled();
  });
  it("recovers from a constructor failure without retaining a pending job", async () => {
    const { run, factory, workers } = setup();
    factory.mockImplementationOnce(() => { throw new Error("worker blocked"); });
    await expect(run()).rejects.toThrow("worker blocked");
    const second = run(); workers[0]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
  it("cleans up a failed postMessage and can retry", async () => {
    const { run, factory, workers } = setup();
    const broken = new FakeWorker();
    broken.postMessage.mockImplementation(() => { throw new Error("cannot clone"); });
    factory.mockReturnValueOnce(broken as unknown as Worker);
    await expect(run()).rejects.toThrow("cannot clone");
    expect(broken.terminate).toHaveBeenCalledOnce();
    const second = run(); workers[0]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
  it.each(["error", "messageerror", "malformed-json", "incomplete-report", "invalid-time", "invalid-digest", "missing-digest"])("settles %s and recovers", async (failure) => {
    const { run, workers } = setup();
    const first = run();
    if (failure === "error") workers[0]!.onerror?.({ message: "crashed", preventDefault: vi.fn() } as unknown as ErrorEvent);
    if (failure === "messageerror") workers[0]!.onmessageerror?.();
    if (failure === "malformed-json") workers[0]!.reply({ json: "{" });
    if (failure === "incomplete-report") workers[0]!.reply({ json: "null" });
    if (failure === "invalid-time") workers[0]!.reply({ ms: NaN });
    if (failure === "invalid-digest") workers[0]!.reply({ engineSha256: "not-a-hash" });
    if (failure === "missing-digest") workers[0]!.reply({ engineSha256: undefined } as unknown as WorkerResponse);
    await expect(first).rejects.toBeInstanceOf(Error);
    expect(workers[0]!.terminate).toHaveBeenCalledOnce();
    const second = run(); workers[1]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
  it("rejects a handled engine error without discarding a healthy worker", async () => {
    const { run, workers } = setup();
    const first = run(); workers[0]!.reply({ ok: false, error: "invalid regex" });
    await expect(first).rejects.toThrow("invalid regex");
    expect(workers[0]!.terminate).not.toHaveBeenCalled();
    const second = run(); workers[0]!.reply();
    await expect(second).resolves.toMatchObject({ result });
  });
});

describe("gzip worker ownership", () => {
  const input = () => new Uint8Array([31, 139, 8, 0]);
  const output = () => new TextEncoder().encode("all members\n").buffer;
  const reply = (worker: FakeWorker, overrides: object = {}) => worker.reply({ bytes: output(), ...overrides } as Partial<WorkerResponse>);

  it("loads files independently of each other and of a comparison, retaining executable bytes only", async () => {
    const { engine, workers, run } = setup();
    const baseline = engine.gunzip(input(), new AbortController().signal);
    const target = engine.gunzip(input(), new AbortController().signal);
    const comparison = run();
    const module = new Uint8Array([0, 97, 115, 109]).buffer;
    reply(workers[1]!, { engineBytes: module });
    expect(new TextDecoder().decode(await target)).toBe("all members\n");
    expect(workers[1]!.terminate).toHaveBeenCalledOnce();
    expect(workers[0]!.terminate).not.toHaveBeenCalled();
    expect(workers[2]!.terminate).not.toHaveBeenCalled();
    workers[2]!.reply(); await comparison;
    reply(workers[0]!); await baseline;
    const next = engine.gunzip(input(), new AbortController().signal);
    expect(workers[3]!.postMessage.mock.calls[0]![0].engine).toEqual({ bytes: module });
    reply(workers[3]!); await next;
  });

  it("terminates only the cancelled file, ignores queued replies and releases listeners", async () => {
    const { engine, workers, run } = setup();
    const controller = new AbortController();
    const cancelled = engine.gunzip(input(), controller.signal);
    const stale = workers[0]!.onmessage!;
    const pending = engine.gunzip(input(), new AbortController().signal);
    controller.abort();
    await expect(cancelled).rejects.toMatchObject({ name: "AbortError" });
    const comparison = run();
    stale({ data: { id: 3, ok: true, bytes: output(), ms: 1, engineSha256, engineBytes: new ArrayBuffer(1) } } as MessageEvent<WorkerResponse>);
    expect(workers[2]!.postMessage.mock.calls[0]![0].engine).toHaveProperty("url");
    expect(workers[1]!.terminate).not.toHaveBeenCalled();
    workers[2]!.reply(); await comparison;
    reply(workers[1]!); await pending;
    expect(workers[0]!.terminate).toHaveBeenCalledOnce();
  });

  it("does not create a worker for a cancelled import", async () => {
    const { engine, factory } = setup();
    const controller = new AbortController(); controller.abort();
    await expect(engine.gunzip(input(), controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(factory).not.toHaveBeenCalled();
  });

  it.each(["constructor", "post", "error", "messageerror", "wrong-kind", "invalid-digest", "invalid-module", "invalid-time", "unreadable", "gzip-error"])("settles %s without retaining file data or invalid module bytes", async (failure) => {
    const { engine, factory, workers } = setup();
    if (failure === "constructor") factory.mockImplementationOnce(() => { throw new Error("blocked"); });
    if (failure === "post") factory.mockImplementationOnce(() => {
      const worker = new FakeWorker(); workers.push(worker);
      worker.postMessage.mockImplementation(() => { throw new Error("cannot clone"); });
      return worker as unknown as Worker;
    });
    const pending = engine.gunzip(input(), new AbortController().signal);
    if (failure === "error") workers[0]!.onerror?.({ message: "crashed", preventDefault: vi.fn() } as unknown as ErrorEvent);
    if (failure === "messageerror") workers[0]!.onmessageerror?.();
    if (failure === "wrong-kind") workers[0]!.reply();
    if (failure === "invalid-digest") reply(workers[0]!, { engineSha256: "bad", engineBytes: new ArrayBuffer(8) });
    if (failure === "invalid-module") reply(workers[0]!, { engineBytes: new ArrayBuffer(0) });
    if (failure === "invalid-time") reply(workers[0]!, { ms: NaN, engineBytes: new ArrayBuffer(8) });
    if (failure === "unreadable") workers[0]!.onmessage?.({ data: null } as unknown as MessageEvent<WorkerResponse>);
    if (failure === "gzip-error") workers[0]!.reply({ ok: false, error: "checksum failed" });
    await expect(pending).rejects.toBeInstanceOf(Error);
    if (failure !== "constructor") expect(workers[0]!.terminate).toHaveBeenCalledOnce();
    const next = engine.gunzip(input(), new AbortController().signal);
    const latest = workers.at(-1)!;
    expect(latest.postMessage.mock.calls[0]![0].engine).toHaveProperty("url");
    reply(latest); await next;
  });
});
