//! Collects `-C N` context lines around a small set of specific line numbers by making one
//! extra pass over a (re-readable) file with a small ring buffer, rather than loading the
//! whole file into memory. Used by `logdelta diff` to show a few lines before/after each
//! finding's first matching line.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io;

use serde::Serialize;

use crate::io::read_lines;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ContextWindow {
    /// (line_no, raw text), oldest first.
    pub before: Vec<(usize, String)>,
    pub after: Vec<(usize, String)>,
}

/// Re-reads `path` and, for every line number in `wanted`, collects up to `context` raw
/// lines before and after it. `path` must be re-readable (a real file, not `"-"` stdin);
/// callers should skip context collection for stdin targets.
pub fn collect_context(
    path: &str,
    wanted: &BTreeSet<usize>,
    context: usize,
) -> io::Result<HashMap<usize, ContextWindow>> {
    let mut result: HashMap<usize, ContextWindow> = HashMap::new();
    if context == 0 || wanted.is_empty() {
        return Ok(result);
    }

    let mut ring: VecDeque<(usize, String)> = VecDeque::with_capacity(context);
    let mut active: Vec<usize> = Vec::new();

    for (idx, line) in read_lines(path)?.enumerate() {
        let line_no = idx + 1;
        let raw = line?;

        if wanted.contains(&line_no) {
            let before: Vec<(usize, String)> = ring.iter().cloned().collect();
            result.entry(line_no).or_insert(ContextWindow {
                before,
                after: Vec::new(),
            });
            active.push(line_no);
        }

        active.retain(|&t| {
            if line_no > t {
                if let Some(w) = result.get_mut(&t) {
                    if w.after.len() < context {
                        w.after.push((line_no, raw.clone()));
                    }
                    return w.after.len() < context;
                }
            }
            true
        });

        ring.push_back((line_no, raw));
        if ring.len() > context {
            ring.pop_front();
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn collects_before_and_after() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(f, "line{i}").unwrap();
        }
        f.flush().unwrap();

        let wanted: BTreeSet<usize> = [5].into_iter().collect();
        let ctx = collect_context(f.path().to_str().unwrap(), &wanted, 2).unwrap();
        let w = &ctx[&5];
        assert_eq!(
            w.before,
            vec![(3, "line3".to_string()), (4, "line4".to_string())]
        );
        assert_eq!(
            w.after,
            vec![(6, "line6".to_string()), (7, "line7".to_string())]
        );
    }

    #[test]
    fn handles_edges_of_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=3 {
            writeln!(f, "line{i}").unwrap();
        }
        f.flush().unwrap();

        let wanted: BTreeSet<usize> = [1, 3].into_iter().collect();
        let ctx = collect_context(f.path().to_str().unwrap(), &wanted, 5).unwrap();
        assert!(ctx[&1].before.is_empty());
        assert_eq!(ctx[&1].after.len(), 2);
        assert_eq!(ctx[&3].before.len(), 2);
        assert!(ctx[&3].after.is_empty());
    }

    #[test]
    fn zero_context_returns_empty_map() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "a").unwrap();
        f.flush().unwrap();
        let wanted: BTreeSet<usize> = [1].into_iter().collect();
        let ctx = collect_context(f.path().to_str().unwrap(), &wanted, 0).unwrap();
        assert!(ctx.is_empty());
    }
}
