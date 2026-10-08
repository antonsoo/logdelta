//! Keep diagnostic-looking lines visible when a block's representatives do not fit.
//!
//! This is a presentation heuristic, not a detector: a test's `Log` and `Error` can
//! have exactly the same prefix. It never changes findings, scores or source text.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;

use super::{head_and_tail, printable, BlockLine};

/// A representative's index in `block_lines`, or a count of omitted representatives.
/// Gaps count templates, not source lines (one template may occur many times).
#[derive(Debug, PartialEq, Eq)]
pub enum ExcerptRow {
    Line(usize),
    Gap(usize),
}

// Recognize output shapes, not project names, assertion wording, or the study's
// expected answers. Standalone source locations include test/helper output and
// compiler diagnostics; timestamped logger source locations are not standalone.
static PATTERNS: LazyLock<(Regex, Regex)> = LazyLock::new(|| {
    #[derive(Deserialize)]
    struct Patterns {
        diagnostic: String,
        severity: String,
    }
    // Shared with the browser preview; keep the expressions in the common subset
    // of Rust regex and JavaScript RegExp, without lookaround or backreferences.
    let patterns: Patterns = serde_json::from_str(include_str!("excerpt-patterns.json"))
        .expect("excerpt pattern definitions");
    let compile = |pattern: &str| {
        RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .expect("excerpt pattern")
    };
    (compile(&patterns.diagnostic), compile(&patterns.severity))
});

fn priority(raw: &str) -> u8 {
    let text = printable(raw);
    if PATTERNS.0.is_match(&text) {
        2
    } else if PATTERNS.1.is_match(&text) {
        1
    } else {
        0
    }
}

/// Choose at most `limit` representatives (`0`: all), always in source order.
///
/// Keep the block's opening and closing line and any marked lines already visible
/// in the head/tail view. Use the remaining slots for standalone diagnostics, then
/// severity-marked lines. Prefer later diagnostics when the budget is exhausted:
/// setup chatter often precedes a test's assertion or an exception's final cause.
/// Fill any remaining space with the previous first-two-thirds/last-third view.
/// With no diagnostic markers, the excerpt is unchanged, including tiny limits.
pub fn block_excerpt(lines: &[BlockLine<'_>], limit: usize) -> Vec<ExcerptRow> {
    let total = lines.len();
    if limit == 0 || total <= limit {
        return (0..total).map(ExcerptRow::Line).collect();
    }
    let (head, tail) = head_and_tail(total, limit);
    let fallback = (0..head).chain((total - tail..total).rev());
    let mut candidates: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let rank = priority(line.raw);
            (rank > 0).then_some((rank, index))
        })
        .collect();
    let mut selected = BTreeSet::new();
    if !candidates.is_empty() {
        // A one-line budget is useful too: show the strongest diagnostic, not the
        // setup line. Two lines keep a diagnostic and the opening for orientation.
        if limit >= 2 {
            selected.insert(0);
        }
        if limit >= 3 {
            selected.insert(total - 1);
            // Don't hide an early exception just because a later subtest logs many
            // source locations. Every pinned line already fit the old excerpt.
            for &(_, index) in &candidates {
                if index < head || index >= total - tail {
                    selected.insert(index);
                }
            }
        }
        candidates.sort_unstable_by(|a, b| b.cmp(a));
        for (_, index) in candidates {
            if selected.len() == limit {
                break;
            }
            selected.insert(index);
        }
    }
    for index in fallback {
        if selected.len() == limit {
            break;
        }
        selected.insert(index);
    }

    let mut rows = Vec::with_capacity(2 * selected.len() + 1);
    let mut next = 0;
    for index in selected {
        if index > next {
            rows.push(ExcerptRow::Gap(index - next));
        }
        rows.push(ExcerptRow::Line(index));
        next = index + 1;
    }
    if next < total {
        rows.push(ExcerptRow::Gap(total - next));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_browser_previews_share_cases_and_account_for_every_omission() {
        #[derive(Deserialize)]
        struct Case {
            name: String,
            lines: Vec<String>,
            previews: Vec<Preview>,
        }
        #[derive(Deserialize)]
        struct Preview {
            limit: usize,
            lines: Vec<usize>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../tests/excerpt-cases.json")).unwrap();
        for case in cases {
            let lines: Vec<_> = case
                .lines
                .iter()
                .enumerate()
                .map(|(i, raw)| BlockLine {
                    line_no: 100 + 7 * i,
                    raw,
                    count: 3,
                })
                .collect();
            for preview in case.previews {
                let rows = block_excerpt(&lines, preview.limit);
                let shown: Vec<_> = rows
                    .iter()
                    .filter_map(|row| match row {
                        ExcerptRow::Line(i) => Some(*i),
                        ExcerptRow::Gap(_) => None,
                    })
                    .collect();
                assert_eq!(
                    shown, preview.lines,
                    "{} at limit {}",
                    case.name, preview.limit
                );
                let mut cursor = 0;
                for row in rows {
                    match row {
                        ExcerptRow::Line(i) => {
                            assert_eq!(i, cursor, "every gap is exact, in source order");
                            cursor += 1;
                        }
                        ExcerptRow::Gap(n) => {
                            assert!(n > 0);
                            cursor += n;
                        }
                    }
                }
                assert_eq!(cursor, lines.len());
            }
            for limit in [0, lines.len(), lines.len() + 1] {
                assert_eq!(
                    block_excerpt(&lines, limit),
                    (0..lines.len()).map(ExcerptRow::Line).collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(block_excerpt(&[], 0), []);
        assert_eq!(block_excerpt(&[], 12), []);
    }

    #[test]
    fn recognizes_diagnostic_shapes_without_treating_any_failure_word_as_a_cause() {
        for line in [
            "    versioning_test.go:250: context deadline exceeded",
            "\tC:/work/src/check.rs:17:4: error: mismatched types",
            "  panic: runtime error: index out of range",
            "E       AssertionError: expected True",
            "ValueError: invalid literal",
            "Caused by: java.net.ConnectException: refused",
            "thread 'main' panicked at src/main.rs:42:9:",
            "\x1b[31merror[E0308]: mismatched types\x1b[0m",
        ] {
            assert_eq!(priority(line), 2, "{line}");
        }
        for line in [
            "2026-10-08T12:00:00Z ERROR connection refused",
            r#"{"level":"error","message":"refused"}"#,
            "[FATAL] cannot start",
        ] {
            assert_eq!(priority(line), 1, "{line}");
        }
        for line in [
            "expected failure test passed",
            "retrying after last failure",
            "errors=0",
            "I1008 01:00:00.12345 123 server.go:123] controller started",
        ] {
            assert_eq!(priority(line), 0, "{line}");
        }
        // Severity tokens are not a truth classifier.
        assert_eq!(priority("INFO no error occurred"), 1);
    }
}
