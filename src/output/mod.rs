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

#[cfg(test)]
mod tests {
    use super::*;

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
