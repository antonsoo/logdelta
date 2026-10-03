import { decodeLog } from "./decode";

export const MAX_LOG_BYTES = 25 * 1024 * 1024;
export const MAX_COMPARISON_BYTES = 50 * 1024 * 1024;
export const MAX_BASELINES = 8;

/** UTF-8 size without allocating another copy of a potentially large pasted log. */
export function textBytes(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x80) bytes++;
    else if (c < 0x800) bytes += 2;
    else if (c >= 0xd800 && c <= 0xdbff && text.charCodeAt(i + 1) >= 0xdc00 && text.charCodeAt(i + 1) <= 0xdfff) {
      bytes += 4;
      i++;
    } else bytes += 3; // Including lone surrogates, which TextEncoder replaces with U+FFFD.
  }
  return bytes;
}

function tooLarge(): Error {
  return new Error("Log exceeds the 25 MiB browser limit (including expanded gzip and decoded text). Use the CLI for larger logs.");
}

/** Bound the stream as it is read, including decompressor output, and release it on every exit. */
export async function readBounded(stream: ReadableStream<Uint8Array>, limit: number, signal: AbortSignal): Promise<Uint8Array> {
  signal.throwIfAborted();
  const reader = stream.getReader();
  const cancel = () => { void reader.cancel().catch(() => {}); };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    const chunks: Uint8Array[] = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      signal.throwIfAborted();
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw tooLarge();
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return bytes;
  } catch (error) {
    cancel();
    throw error;
  } finally {
    signal.removeEventListener("abort", cancel);
    reader.releaseLock();
  }
}

export async function readLogFile(file: File, signal: AbortSignal): Promise<string> {
  signal.throwIfAborted();
  if (file.size > MAX_LOG_BYTES) throw tooLarge();
  const header = new Uint8Array(await file.slice(0, 2).arrayBuffer());
  signal.throwIfAborted();
  let stream = file.stream();
  // Like the CLI, recognize gzip by magic bytes, regardless of the filename.
  if (header[0] === 0x1f && header[1] === 0x8b) stream = stream.pipeThrough(new DecompressionStream("gzip"));
  const text = decodeLog(await readBounded(stream, MAX_LOG_BYTES, signal));
  if (textBytes(text) > MAX_LOG_BYTES) throw tooLarge();
  return text;
}
