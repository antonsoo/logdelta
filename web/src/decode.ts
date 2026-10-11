/**
 * A log's text, as the CLI reads it: UTF-16 when the bytes start with that byte-order mark
 * (what `program > out.log` writes in Windows PowerShell), UTF-8 otherwise. TextDecoder
 * drops the mark of either.
 */
export function decodeLog(bytes: Uint8Array): string {
  return decodeLogInput(bytes).text;
}

export interface DecodedLog {
  text: string;
  /** The decoder replaced malformed source bytes; literal U+FFFD is not a loss. */
  decodingLoss?: string;
}

export function decodeLogInput(bytes: Uint8Array): DecodedLog {
  const utf16 =
    bytes[0] === 0xff && bytes[1] === 0xfe ? "utf-16le" : bytes[0] === 0xfe && bytes[1] === 0xff ? "utf-16be" : null;
  // Like the CLI: a NUL byte near the start means an image, an archive or a program, not a log,
  // and reading one as text filled the editor with hundreds of junk "lines".
  if (!utf16 && bytes.subarray(0, 256 * 1024).includes(0)) throw new NotTextError();
  const encoding = utf16 ?? "utf-8";
  try {
    return { text: new TextDecoder(encoding, { fatal: true }).decode(bytes) };
  } catch {
    return { text: new TextDecoder(encoding).decode(bytes), decodingLoss: encoding.toUpperCase() };
  }
}

export class NotTextError extends Error {
  constructor() {
    super("not a text file (a log is text: UTF-8, UTF-16 with a byte-order mark, or either gzipped)");
    this.name = "NotTextError";
  }
}
