mod excerpt;
mod fields;
pub mod human;
pub mod json;
pub mod markdown;

use crate::analysis::{Finding, FindingKind};
use crate::blocks::Block;

pub use excerpt::{block_excerpt, ExcerptRow};

/// How many of a block's lines a report shows unless told otherwise (`--block-lines`).
pub const DEFAULT_BLOCK_LINES: usize = 12;

/// One line of a block as reports show it: a template's first line in the run the block is
/// in, and how many lines that template has there.
pub struct BlockLine<'a> {
    pub line_no: usize,
    pub raw: &'a str,
    pub count: u64,
}

/// A block's lines, one per template, in line order.
pub fn block_lines<'a>(block: &Block, findings: &'a [Finding]) -> Vec<BlockLine<'a>> {
    block
        .findings
        .iter()
        .filter_map(|&index| findings.get(index))
        .filter_map(|f| {
            let (line_no, raw, count) = match block.kind {
                FindingKind::Gone => (
                    f.first_baseline_line_no?,
                    f.first_baseline_raw.as_deref()?,
                    f.baseline_counts.first().copied().unwrap_or(0),
                ),
                _ => (
                    f.first_target_line_no?,
                    f.first_target_raw.as_deref()?,
                    f.target_count,
                ),
            };
            Some(BlockLine {
                line_no,
                raw,
                count,
            })
        })
        .collect()
}

/// How many of a block's `lines` to show from its start and from its end when at most
/// `limit` fit (`0`: no limit). A block that fits is shown whole; a longer one keeps its
/// first two thirds and its last third, since a traceback opens with what failed and
/// closes with the error.
pub fn head_and_tail(lines: usize, limit: usize) -> (usize, usize) {
    if limit == 0 || lines <= limit {
        return (lines, 0);
    }
    let tail = limit / 3;
    (limit - tail, tail)
}

/// The blocks and the findings outside any block of one kind, highest score first: what a
/// report lists under that kind.
pub enum Item<'a> {
    Block(&'a Block),
    Finding(&'a Finding),
}

impl Item<'_> {
    fn score(&self) -> f64 {
        match self {
            Item::Block(b) => b.score,
            Item::Finding(f) => f.score,
        }
    }
}

pub fn items_of_kind<'a>(
    kind: FindingKind,
    blocks: &'a [Block],
    findings: &'a [Finding],
) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = blocks
        .iter()
        .filter(|b| b.kind == kind)
        .map(Item::Block)
        .chain(
            findings
                .iter()
                .filter(|f| f.kind == kind && f.block.is_none())
                .map(Item::Finding),
        )
        .collect();
    // Stable: at equal scores a block stays ahead of a finding, and each keeps its order.
    items.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items
}

/// Colors a template's variable parts: masking placeholders and Drain wildcards, wherever
/// in a token they are (`<TS>`, `trace=<UUID>`, `level=<*>`, `[<NUM>%]`). No-op (returns
/// the template unchanged) when `use_color` is false.
pub fn highlight_template(template: &str, use_color: bool) -> String {
    if !use_color {
        return template.to_string();
    }
    let var_style = anstyle::Style::new()
        .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
        .bold();
    let reset = anstyle::Reset;
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let name = after
            .bytes()
            .take_while(|b| b.is_ascii_uppercase() || matches!(b, b'*' | b'!'))
            .count();
        if name > 0 && after.as_bytes().get(name) == Some(&b'>') {
            let end = open + name + 2;
            out.push_str(&rest[..open]);
            out.push_str(&format!("{var_style}{}{reset}", &rest[open..end]));
            rest = &rest[end..];
        } else {
            out.push_str(&rest[..=open]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// A log line or template as a report prints it: without terminal escape sequences and
/// without control characters (a tab stays, a line break inside a value becomes a space).
/// Logs captured from a colored build are full of both, and written out as they are they
/// recolor the reader's terminal from the point a line was cut, move the cursor, or show
/// as `␛[31m` in a Markdown table. `--json` keeps every line as it was read.
pub fn printable(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(|c| c.is_control() && c != '\t') {
        return std::borrow::Cow::Borrowed(s);
    }
    let stripped = crate::mask::strip_escapes(s);
    std::borrow::Cow::Owned(
        stripped
            .chars()
            .filter_map(|c| match c {
                '\n' | '\r' => Some(' '),
                '\t' => Some(c),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect(),
    )
}

/// The most of one template or log line a report shows. Logs do contain lines of hundreds
/// of kilobytes (a minified bundle, a base64 blob, a JSON payload on one line); printed
/// whole, one such finding buries every other one and can push a PR comment past GitHub's
/// size limit. `--json` output is never clipped.
pub const MAX_SHOWN_CHARS: usize = 400;

/// `s` cut to `max_chars` characters, saying how much was left out. Counts characters, not
/// bytes, so multi-byte UTF-8 is never split.
pub fn clip(s: &str, max_chars: usize) -> std::borrow::Cow<'_, str> {
    match s.char_indices().nth(max_chars) {
        None => std::borrow::Cow::Borrowed(s),
        Some((cut, _)) => {
            let rest = s[cut..].chars().count();
            std::borrow::Cow::Owned(format!("{}… (+{rest} more characters)", &s[..cut]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_drops_escape_sequences_and_control_characters() {
        assert_eq!(printable("plain\ttext"), "plain\ttext");
        assert_eq!(
            printable("\x1b[31m\x1b[1mFAILED\x1b[0m test_a"),
            "FAILED test_a"
        );
        // A progress bar redrawn with carriage returns, a bell, a lone escape.
        assert_eq!(printable("10%\r20%\x07 done\x1b"), "10% 20% done");
        assert!(matches!(
            printable("nothing to do"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_block_that_fits_is_shown_whole() {
        assert_eq!(head_and_tail(12, 12), (12, 0));
        assert_eq!(head_and_tail(3, 12), (3, 0));
        assert_eq!(head_and_tail(500, 0), (500, 0));
    }

    #[test]
    fn a_long_block_keeps_its_start_and_its_end() {
        assert_eq!(head_and_tail(13, 12), (8, 4));
        assert_eq!(head_and_tail(88, 12), (8, 4));
        assert_eq!(head_and_tail(88, 2), (2, 0));
        assert_eq!(head_and_tail(88, 1), (1, 0));
    }

    #[test]
    fn clip_leaves_what_fits() {
        assert_eq!(clip("short line", 400), "short line");
        assert_eq!(clip("exact", 5), "exact");
    }

    #[test]
    fn clip_says_how_much_it_left_out() {
        assert_eq!(clip("abcdefghij", 4), "abcd… (+6 more characters)");
    }

    #[test]
    fn clip_counts_characters_not_bytes() {
        assert_eq!(clip("ééééé", 2), "éé… (+3 more characters)");
    }

    #[test]
    fn highlight_noop_without_color() {
        assert_eq!(highlight_template("user <*> in", false), "user <*> in");
    }

    #[test]
    fn highlight_wraps_placeholders_with_color() {
        let out = highlight_template("user <*> in", true);
        assert!(out.contains("<*>"));
        assert!(out.len() > "user <*> in".len());
    }
}
