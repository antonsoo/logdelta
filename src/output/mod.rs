pub mod human;
pub mod json;
pub mod markdown;

use crate::mask::is_placeholder;

/// Colors and styles a template's tokens, highlighting masking placeholders / Drain
/// wildcards as the "variable" parts of the line. No-op (returns the template unchanged)
/// when `use_color` is false.
pub fn highlight_template(template: &str, use_color: bool) -> String {
    if !use_color {
        return template.to_string();
    }
    let var_style = anstyle::Style::new()
        .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
        .bold();
    let reset = anstyle::Reset;
    template
        .split(' ')
        .map(|tok| {
            if is_placeholder(tok) {
                format!("{var_style}{tok}{reset}")
            } else {
                tok.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
