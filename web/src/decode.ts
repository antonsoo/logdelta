/**
 * A log's text, as the CLI reads it: UTF-16 when the bytes start with that byte-order mark
 * (what `program > out.log` writes in Windows PowerShell), UTF-8 otherwise. TextDecoder
 * drops the mark of either.
 */
export function decodeLog(bytes: Uint8Array): string {
  const utf16 =
    bytes[0] === 0xff && bytes[1] === 0xfe ? "utf-16le" : bytes[0] === 0xfe && bytes[1] === 0xff ? "utf-16be" : null;
  return new TextDecoder(utf16 ?? "utf-8").decode(bytes);
}
