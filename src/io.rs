//! Opening log sources (files, stdin, gzip) and iterating their lines.
//!
//! A log is not always what the program wrote: `build.exe > build.log` in Windows PowerShell
//! saves UTF-16 with a byte-order mark, and some Windows tools put a mark in front of UTF-8.
//! Both are recognized by the mark and read as the text they hold. Without that, a UTF-16
//! target compared with UTF-8 baselines shares no line with them, and a UTF-8 mark makes a
//! log's first line a template of its own.
//!
//! Bytes that are not valid UTF-8 are handled losslessly-as-possible but never fatally:
//! invalid sequences are replaced with `U+FFFD` (via `String::from_utf8_lossy`), matching
//! the brief's "non-UTF-8 bytes handled lossily" requirement. A single malformed line never
//! aborts a multi-gigabyte log.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};

use flate2::read::MultiGzDecoder;

/// Opens `path` for line-oriented reading. `"-"` means stdin. Gzip is recognized by its first
/// two bytes, whatever the name, and decompressed (as a multi-stream gzip, since concatenated
/// `.gz` logs are common).
pub fn open_source(path: &str) -> io::Result<Box<dyn BufRead>> {
    if path == "-" {
        return opened(BufReader::with_capacity(BUFFER, io::stdin()));
    }
    let file = File::open(path).map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
    opened(BufReader::with_capacity(BUFFER, file))
        .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))
}

const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

fn opened<R: BufRead + 'static>(mut reader: R) -> io::Result<Box<dyn BufRead>> {
    if reader.fill_buf()?.starts_with(&GZIP_MAGIC) {
        decoded(BufReader::with_capacity(
            BUFFER,
            MultiGzDecoder::new(reader),
        ))
    } else {
        decoded(reader)
    }
}

const BUFFER: usize = 256 * 1024;

/// `reader` as UTF-8 text: past a UTF-8 byte-order mark, or transcoded from UTF-16 when it
/// starts with that mark. Anything else is passed through untouched, unless it is plainly not
/// text: a NUL byte in the first block (an image, an archive, an executable) is refused, since
/// "mining" one gave hundreds of junk templates and a diff that exited 0.
fn decoded<R: BufRead + 'static>(mut reader: R) -> io::Result<Box<dyn BufRead>> {
    let head = reader.fill_buf()?;
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        reader.consume(3);
        return Ok(Box::new(reader));
    }
    let big_endian = match head {
        [0xFF, 0xFE, ..] => false,
        [0xFE, 0xFF, ..] => true,
        _ if head.contains(&0) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a text file (a log is text: UTF-8, UTF-16 with a byte-order mark, or either gzipped)",
            ))
        }
        _ => return Ok(Box::new(reader)),
    };
    reader.consume(2);
    Ok(Box::new(BufReader::with_capacity(
        BUFFER,
        Utf16Reader::new(reader, big_endian),
    )))
}

/// Reads UTF-16 from `inner` and yields the same text as UTF-8, a block at a time. A code
/// unit that is not valid on its own (half a surrogate pair, an odd last byte) becomes
/// `U+FFFD`, like an invalid UTF-8 sequence does further down.
struct Utf16Reader<R: Read> {
    inner: R,
    big_endian: bool,
    /// The odd byte left over when a read ended in the middle of a code unit.
    half_unit: Option<u8>,
    /// A leading surrogate whose other half is in the next read.
    lead: Option<u16>,
    out: Vec<u8>,
    pos: usize,
    done: bool,
}

impl<R: Read> Utf16Reader<R> {
    fn new(inner: R, big_endian: bool) -> Self {
        Utf16Reader {
            inner,
            big_endian,
            half_unit: None,
            lead: None,
            out: Vec::new(),
            pos: 0,
            done: false,
        }
    }

    fn push(&mut self, decoded: Result<char, std::char::DecodeUtf16Error>) {
        let ch = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
        self.out
            .extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
    }

    /// Decodes the next block of input into `out`.
    fn fill(&mut self) -> io::Result<()> {
        self.out.clear();
        self.pos = 0;
        let mut raw = [0u8; 16 * 1024];
        let n = loop {
            match self.inner.read(&mut raw) {
                Ok(n) => break n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        };
        if n == 0 {
            self.done = true;
            if self.half_unit.take().is_some() || self.lead.take().is_some() {
                self.out.extend_from_slice(
                    char::REPLACEMENT_CHARACTER
                        .encode_utf8(&mut [0; 4])
                        .as_bytes(),
                );
            }
            return Ok(());
        }
        let mut units: Vec<u16> = Vec::with_capacity(n / 2 + 2);
        units.extend(self.lead.take());
        let mut bytes = self
            .half_unit
            .take()
            .into_iter()
            .chain(raw[..n].iter().copied());
        while let Some(first) = bytes.next() {
            match bytes.next() {
                Some(second) if self.big_endian => units.push(u16::from_be_bytes([first, second])),
                Some(second) => units.push(u16::from_le_bytes([first, second])),
                None => self.half_unit = Some(first),
            }
        }
        if matches!(units.last(), Some(0xD800..=0xDBFF)) {
            self.lead = units.pop();
        }
        for decoded in char::decode_utf16(units) {
            self.push(decoded);
        }
        Ok(())
    }
}

impl<R: Read> Read for Utf16Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.out.len() && !self.done {
            self.fill()?;
        }
        let n = buf.len().min(self.out.len() - self.pos);
        buf[..n].copy_from_slice(&self.out[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Iterates the lossily-decoded, newline-stripped lines of `reader`, one allocation per line.
/// Works for arbitrarily large streams: it never buffers more than the current line.
pub struct LineIter<R: BufRead> {
    reader: R,
    buf: Vec<u8>,
}

impl<R: BufRead> LineIter<R> {
    pub fn new(reader: R) -> Self {
        LineIter {
            reader,
            buf: Vec::with_capacity(4096),
        }
    }
}

impl<R: BufRead> Iterator for LineIter<R> {
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        self.buf.clear();
        match self.reader.read_until(b'\n', &mut self.buf) {
            Ok(0) => None,
            Ok(_) => {
                while matches!(self.buf.last(), Some(b'\n' | b'\r')) {
                    self.buf.pop();
                }
                Some(Ok(String::from_utf8_lossy(&self.buf).into_owned()))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Convenience: opens and iterates `path` in one call.
pub fn read_lines(path: &str) -> io::Result<LineIter<Box<dyn BufRead>>> {
    Ok(LineIter::new(open_source(path)?))
}

/// A single line read directly from a `Read` source without buffering the whole thing,
/// used by `novel` for the `tail -f`-friendly streaming path where stdout must be flushed
/// per line rather than batched.
pub fn read_line_from<R: Read>(reader: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<String>> {
    buf.clear();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => {
                if buf.is_empty() {
                    return Ok(None);
                }
                break;
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    while matches!(buf.last(), Some(b'\r')) {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(buf).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn splits_lines_and_strips_crlf() {
        let data = b"one\r\ntwo\nthree".to_vec();
        let lines: Vec<String> = LineIter::new(Cursor::new(data))
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(lines, vec!["one", "two", "three"]);
    }

    #[test]
    fn handles_invalid_utf8_lossily() {
        let mut data = b"good\n".to_vec();
        data.extend_from_slice(&[0xff, 0xfe, b'\n']);
        data.extend_from_slice(b"tail");
        let lines: Vec<String> = LineIter::new(Cursor::new(data))
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(lines[0], "good");
        assert!(lines[1].contains('\u{FFFD}'));
        assert_eq!(lines[2], "tail");
    }

    fn lines_of(bytes: Vec<u8>) -> Vec<String> {
        LineIter::new(decoded(Cursor::new(bytes)).unwrap())
            .map(|r| r.unwrap())
            .collect()
    }

    fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
        let mut bytes = if big_endian {
            vec![0xFE, 0xFF]
        } else {
            vec![0xFF, 0xFE]
        };
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        bytes
    }

    #[test]
    fn a_utf8_byte_order_mark_is_not_part_of_the_first_line() {
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"first\r\nsecond\r\n");
        assert_eq!(lines_of(data), vec!["first", "second"]);
    }

    #[test]
    fn utf16_with_a_mark_reads_as_the_text_it_holds() {
        // What `program > out.log` writes in Windows PowerShell.
        let text = "ERROR caf\u{e9} \u{2192} \u{1F525} failed\r\nsecond line\r\n";
        for big_endian in [false, true] {
            assert_eq!(
                lines_of(utf16(text, big_endian)),
                vec!["ERROR caf\u{e9} \u{2192} \u{1F525} failed", "second line"]
            );
        }
    }

    /// A reader that hands over at most `step` bytes at a time, as a pipe might.
    struct Trickle {
        data: Vec<u8>,
        at: usize,
        step: usize,
    }

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.step.min(buf.len()).min(self.data.len() - self.at);
            buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        }
    }

    #[test]
    fn utf16_split_anywhere_between_reads_decodes_the_same() {
        // Every read size from 1 to 9 bytes puts boundaries inside code units and inside
        // surrogate pairs.
        let line = "x\u{1F525}y\u{e9}\u{1F525}z";
        let text: String = (0..200).map(|_| format!("{line}\r\n")).collect();
        for big_endian in [false, true] {
            let data = utf16(&text, big_endian)[2..].to_vec();
            for step in 1..=9 {
                let source = Trickle {
                    data: data.clone(),
                    at: 0,
                    step,
                };
                let reader = BufReader::new(Utf16Reader::new(source, big_endian));
                let lines: Vec<String> = LineIter::new(reader).map(|r| r.unwrap()).collect();
                assert_eq!(lines.len(), 200, "step {step}");
                assert!(lines.iter().all(|l| l == line), "step {step}");
            }
        }
    }

    #[test]
    fn utf16_longer_than_one_block() {
        let line = "x\u{1F525}y\u{1F525}zz";
        let text: String = (0..9000).map(|_| format!("{line}\n")).collect();
        let lines = lines_of(utf16(&text, false));
        assert_eq!(lines.len(), 9000);
        assert!(lines.iter().all(|l| l == line));
    }

    #[test]
    fn damaged_utf16_is_replaced_not_fatal() {
        let mut data = utf16("ok\n", false);
        data.extend_from_slice(&0xDC00u16.to_le_bytes()); // a trailing surrogate on its own
        data.extend_from_slice(&u16::from(b'a').to_le_bytes());
        data.extend_from_slice(&0xD83Du16.to_le_bytes()); // a leading one at the very end
        data.push(0x41); // and half a code unit
        assert_eq!(lines_of(data), vec!["ok", "\u{FFFD}a\u{FFFD}"]);
    }

    #[test]
    fn text_without_a_mark_is_untouched() {
        assert_eq!(lines_of(b"plain\n".to_vec()), vec!["plain"]);
        assert_eq!(lines_of(vec![0xFF]), vec!["\u{FFFD}"]);
        assert!(lines_of(Vec::new()).is_empty());
    }

    #[test]
    fn empty_input_yields_no_lines() {
        let lines: Vec<String> = LineIter::new(Cursor::new(Vec::new()))
            .map(|r| r.unwrap())
            .collect();
        assert!(lines.is_empty());
    }

    #[test]
    fn missing_file_error_names_the_path() {
        let err = match open_source("/definitely/does/not/exist.log") {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        };
        assert!(
            err.to_string().contains("/definitely/does/not/exist.log"),
            "error should name the path, got: {err}"
        );
    }
}
