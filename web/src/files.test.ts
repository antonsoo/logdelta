import { describe, expect, it, vi } from "vitest";
import { gzipSync } from "node:zlib";
import { MAX_LOG_BYTES, readBounded, readLogFile, textBytes } from "./files";

const signal = () => new AbortController().signal;
const utf8 = (text: string) => new TextEncoder().encode(text);

describe("bounded log imports", () => {
  it.each(["", "ascii\n", "café", "日本語", "a😀b", "\ud800x\udc00", "\ufeffhi\r\n"])("counts UTF-8 bytes without changing %j", (text) => {
    expect(textBytes(text)).toBe(utf8(text).length);
  });
  it("keeps exact-limit streams and empty logs", async () => {
    const stream = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(utf8("abc")); c.enqueue(utf8("de")); c.close(); } });
    expect(await readBounded(stream, 5, signal())).toEqual(utf8("abcde"));
    expect(stream.locked).toBe(false);
    expect(await readLogFile(new File([], "empty.log"), signal())).toBe("");
  });
  it("cancels as soon as the byte limit is crossed, without collecting the remaining stream", async () => {
    const cancel = vi.fn();
    const stream = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(utf8("abc")); c.enqueue(utf8("def")); }, cancel });
    await expect(readBounded(stream, 5, signal())).rejects.toThrow("25 MiB");
    expect(cancel).toHaveBeenCalledOnce();
    expect(stream.locked).toBe(false);
  });
  it("aborts a pending read and releases its lock", async () => {
    const cancel = vi.fn();
    const stream = new ReadableStream<Uint8Array>({ cancel });
    const controller = new AbortController();
    const result = readBounded(stream, 100, controller.signal);
    controller.abort();
    await expect(result).rejects.toMatchObject({ name: "AbortError" });
    expect(cancel).toHaveBeenCalledOnce();
    expect(stream.locked).toBe(false);
  });
  it("does not acquire a reader for an already-cancelled import", async () => {
    const stream = new ReadableStream<Uint8Array>();
    const controller = new AbortController();
    controller.abort();
    await expect(readBounded(stream, 100, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(stream.locked).toBe(false);
  });
  it("propagates a read failure and releases the stream", async () => {
    const stream = new ReadableStream<Uint8Array>({ start(c) { c.error(new Error("disk read failed")); } });
    await expect(readBounded(stream, 100, signal())).rejects.toThrow("disk read failed");
    expect(stream.locked).toBe(false);
  });
  it("rejects oversized files before reading even the header", async () => {
    const file = new File([], "big.log");
    Object.defineProperty(file, "size", { value: MAX_LOG_BYTES + 1 });
    const slice = vi.spyOn(file, "slice");
    await expect(readLogFile(file, signal())).rejects.toThrow("25 MiB");
    expect(slice).not.toHaveBeenCalled();
  });
  it("reads gzip by magic bytes and decodes UTF-16 after expansion", async () => {
    const bytes = Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from("hello 世界\r\n", "utf16le")]);
    const file = new File([gzipSync(bytes)], "no-gzip-extension.log");
    expect(await readLogFile(file, signal())).toBe("hello 世界\r\n");
  });
  it("rejects gzip that expands beyond the limit", async () => {
    const file = new File([gzipSync(Buffer.alloc(MAX_LOG_BYTES + 1, 120))], "small.gz");
    expect(file.size).toBeLessThan(30000);
    await expect(readLogFile(file, signal())).rejects.toThrow("25 MiB");
  });
  it("rejects corrupt gzip and binary input", async () => {
    await expect(readLogFile(new File([new Uint8Array([0x1f, 0x8b, 0, 0])], "bad.gz"), signal())).rejects.toThrow();
    await expect(readLogFile(new File([new Uint8Array([1, 2, 0, 3])], "image.log"), signal())).rejects.toThrow("not a text file");
  });
});
