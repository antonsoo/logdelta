//! Opening log sources (files, stdin, gzip) and iterating their lines.
//!
//! Bytes that are not valid UTF-8 are handled losslessly-as-possible but never fatally:
//! invalid sequences are replaced with `U+FFFD` (via `String::from_utf8_lossy`), matching
//! the brief's "non-UTF-8 bytes handled lossily" requirement. A single malformed line never
//! aborts a multi-gigabyte log.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use flate2::read::MultiGzDecoder;

/// Opens `path` for line-oriented reading. `"-"` means stdin. `.gz` files are transparently
/// decompressed (as a multi-stream gzip, since concatenated `.gz` logs are common).
pub fn open_source(path: &str) -> io::Result<Box<dyn BufRead>> {
    if path == "-" {
        return Ok(Box::new(BufReader::with_capacity(256 * 1024, io::stdin())));
    }
    let file = File::open(path)?;
    if is_gzip(path) {
        Ok(Box::new(BufReader::with_capacity(
            256 * 1024,
            MultiGzDecoder::new(file),
        )))
    } else {
        Ok(Box::new(BufReader::with_capacity(256 * 1024, file)))
    }
}

fn is_gzip(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gz"))
        .unwrap_or(false)
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

    #[test]
    fn empty_input_yields_no_lines() {
        let lines: Vec<String> = LineIter::new(Cursor::new(Vec::new()))
            .map(|r| r.unwrap())
            .collect();
        assert!(lines.is_empty());
    }
}
