import { describe, expect, it } from "vitest";
import { decodeLog, NotTextError } from "./decode";

const TEXT = "ERROR caf\u00e9 \u2192 \u{1F525} failed\r\nsecond line\r\n";

function utf16(text: string, bigEndian: boolean): Uint8Array {
  const bytes = new Uint8Array(2 + text.length * 2);
  const view = new DataView(bytes.buffer);
  view.setUint16(0, 0xfeff, !bigEndian);
  for (let i = 0; i < text.length; i++) view.setUint16(2 + i * 2, text.charCodeAt(i), !bigEndian);
  return bytes;
}

describe("decodeLog", () => {
  it("reads UTF-8, with or without a byte-order mark", () => {
    const utf8 = new TextEncoder().encode(TEXT);
    expect(decodeLog(utf8)).toBe(TEXT);
    expect(decodeLog(new Uint8Array([0xef, 0xbb, 0xbf, ...utf8]))).toBe(TEXT);
  });

  it("reads what a PowerShell redirect writes: UTF-16 with a mark", () => {
    expect(decodeLog(utf16(TEXT, false))).toBe(TEXT);
    expect(decodeLog(utf16(TEXT, true))).toBe(TEXT);
  });

  it("an empty file is empty text", () => {
    expect(decodeLog(new Uint8Array())).toBe("");
  });

  it("refuses bytes that are not text, as the CLI does", () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d]);
    expect(() => decodeLog(png)).toThrow(NotTextError);
  });
});
