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
