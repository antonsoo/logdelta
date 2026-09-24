//! Turns a raw log line into a *template-friendly* line by replacing the parts that vary
//! from run to run (timestamps, ids, durations, ...) with stable placeholder tokens such as
//! `<TS>` or `<UUID>`.
//!
//! Masking is deliberately conservative: it only replaces things a regex can recognize with
//! reasonable confidence. Anything it misses (an error code, a free-form name, a counter) is
//! still handled correctly downstream, because the Drain-based template miner (see
//! [`crate::drain`]) wildcards any token position that disagrees across otherwise-similar
//! lines. Masking and mining are complementary, not redundant: masking gives Drain a head
//! start and keeps semantically-identical lines from being split into different clusters
//! just because, say, one has a UUID and the other doesn't tokenize the same way.
//!
//! Every placeholder is wrapped in angle brackets (`<TS>`, `<NUM>`, `<*>` for Drain's own
//! wildcard, ...) and contains no digits, so later passes never accidentally re-match text
//! inside an already-emitted placeholder, and renderers can highlight "variable" tokens with
//! a single check: does the token look like `<...>`.

use std::sync::LazyLock;

use regex::Regex;

/// A user-supplied additional mask, either from `--mask REGEX` or a config file line.
/// The replacement defaults to `<CUSTOM>` unless the config gives `name=regex`.
#[derive(Debug, Clone)]
pub struct CustomMask {
    pub regex: Regex,
    pub placeholder: String,
}

impl CustomMask {
    pub fn from_cli(pattern: &str) -> Result<Self, regex::Error> {
        Ok(CustomMask {
            regex: Regex::new(pattern)?,
            placeholder: "<CUSTOM>".to_string(),
        })
    }

    /// Parses one non-empty, non-comment line of a mask config file.
    /// Format: `NAME=REGEX` or a bare `REGEX` (placeholder becomes `<CUSTOM>`).
    pub fn from_config_line(line: &str) -> Result<Self, regex::Error> {
        if let Some((name, pattern)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Ok(CustomMask {
                    regex: Regex::new(pattern.trim())?,
                    placeholder: format!("<{}>", name.to_uppercase()),
                });
            }
        }
        Self::from_cli(line)
    }
}

macro_rules! lazy_re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("valid regex"));
    };
}

// ANSI CSI / SGR escape sequences (colors, cursor movement, ...).
lazy_re!(ANSI, r"\x1b\[[0-9;?]*[ -/]*[@-~]");

// RFC 3339 / ISO 8601, e.g. 2024-01-15T10:23:45.123456Z, 2024-01-15 10:23:45,123 (log4j),
// with optional fractional seconds and timezone offset.
lazy_re!(
    TS_ISO,
    r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?"
);
// Syslog / BSD style: "Jan 15 10:23:45" or "Jan  5 10:23:45".
lazy_re!(
    TS_SYSLOG,
    r"\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\b"
);
// US-style app logs: "01/15/2024 10:23:45" or "15/01/2024 10:23:45.123".
lazy_re!(
    TS_SLASH,
    r"\b\d{1,2}/\d{1,2}/\d{4}[ T]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b"
);
// Bare epoch seconds or milliseconds, as their own token (10 or 13 digits).
lazy_re!(TS_EPOCH, r"\b1[0-9]{9}(?:[0-9]{3})?\b");

lazy_re!(
    UUID,
    r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"
);

lazy_re!(EMAIL, r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b");

lazy_re!(URL, r#"\bhttps?://[^\s"'<>()\[\]]+"#);

lazy_re!(
    IPV4,
    r"\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b(?::\d{1,5})?"
);
// IPv6. Deliberately conservative: matches the bracketed form (`[::1]:8080`, `[2001:db8::1]`,
// unambiguous because of the brackets, so `::` compression is fine inside them) and the full,
// uncompressed 8-group form with an optional zone id (`fe80:0:0:0:...:1%eth0`). It does NOT
// try to recognize bare, compressed addresses like `::1` or `fe80::1` outside brackets:
// `::`-compression is indistinguishable from the `::` path/namespace separator that shows up
// constantly in real logs (`std::io::Error`, `Model::find`, `a::b::c::d`) without look-around
// (which the `regex` crate doesn't support), and a false positive there is worse than missing
// an occasional bare compressed address.
lazy_re!(
    IPV6,
    concat!(
        r"\[[0-9a-fA-F]*:[0-9a-fA-F:]*\](?::\d{1,5})?",
        r"|\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}(?:%[0-9a-zA-Z]+)?\b",
    )
);

lazy_re!(HEX_PREFIXED, r"\b0[xX][0-9a-fA-F]+\b");
// Hex-looking ids/hashes (git SHAs, request ids, ...): 7-40 hex chars containing at least
// one letter a-f, so plain decimal numbers fall through to the numeric masker instead.
// (The `regex` crate has no look-around, so the "contains a letter" check happens in the
// replacement closure rather than the pattern itself.)
lazy_re!(HEX_ID, r"\b[0-9a-fA-F]{7,40}\b");

lazy_re!(
    TEMP_PATH,
    r"(?:/tmp/|/var/tmp/|/var/folders/|\\AppData\\Local\\Temp\\|\\Temp\\)[^\s:]*"
);

// number + unit, e.g. "512KiB", "12 ms", "3.4s", "200Mbps".
lazy_re!(
    QUANTITY,
    r"\b\d+(?:\.\d+)?\s?(?:ns|[uµ]s|ms|s|m|h|d|B|KB|KiB|MB|MiB|GB|GiB|TB|TiB|bps|Kbps|Mbps|Gbps)\b"
);
// hour:minute:second duration/elapsed time not already consumed as part of a timestamp.
lazy_re!(DURATION_CLOCK, r"\b\d{1,2}:\d{2}:\d{2}(?:\.\d+)?\b");
// dotted version-like numbers: 3.11.4, 1.0
lazy_re!(VERSION_NUM, r"\b\d+(?:\.\d+){1,3}\b");
// Any remaining standalone run of 4+ digits: large counters, byte counts, ids. Shorter
// numbers (exit codes, HTTP statuses, small counts) are left as literal tokens on purpose:
// they are usually the signal ("500" vs "200"), and Drain (see `crate::drain`) will still
// wildcard a short-number position on its own once it sees enough differing examples.
lazy_re!(BARE_NUM, r"\b\d{4,}\b");

/// Masks the variable parts of `query` and any purely numeric / hex / UUID path segments
/// inside a URL, keeping the scheme, host and literal path segments intact so that e.g.
/// `/api/users/42/orders/9f1c...` and `/api/users/7/orders/aa21...` collapse to the same
/// template while the route itself stays legible.
fn mask_url_internals(url: &str) -> String {
    let (path_part, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    let masked_path = path_part
        .split('/')
        .map(|seg| {
            if seg.is_empty() {
                return seg.to_string();
            }
            let looks_numeric = seg.chars().all(|c| c.is_ascii_digit());
            let looks_hex = seg.len() >= 6
                && seg.chars().all(|c| c.is_ascii_hexdigit())
                && seg.chars().any(|c| c.is_ascii_alphabetic());
            let looks_uuid = UUID.is_match(seg);
            if looks_numeric || looks_hex || looks_uuid {
                "<ID>".to_string()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    match query {
        Some(q) if !q.is_empty() => format!("{masked_path}?<QUERY>"),
        _ => masked_path,
    }
}

/// Applies any user `custom` masks first (so they take priority over the built-ins on
/// overlapping text), then every built-in masker, in a fixed, deterministic order.
pub fn mask_line(line: &str, custom: &[CustomMask]) -> String {
    let mut s = ANSI.replace_all(line, "").into_owned();

    for cm in custom {
        s = cm
            .regex
            .replace_all(&s, cm.placeholder.as_str())
            .into_owned();
    }

    s = TS_ISO.replace_all(&s, "<TS>").into_owned();
    s = TS_SYSLOG.replace_all(&s, "<TS>").into_owned();
    s = TS_SLASH.replace_all(&s, "<TS>").into_owned();
    s = TS_EPOCH.replace_all(&s, "<TS>").into_owned();
    s = DURATION_CLOCK.replace_all(&s, "<DUR>").into_owned();

    s = UUID.replace_all(&s, "<UUID>").into_owned();
    s = EMAIL.replace_all(&s, "<EMAIL>").into_owned();

    s = {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for m in URL.find_iter(&s) {
            out.push_str(&s[last..m.start()]);
            let masked = mask_url_internals(m.as_str());
            out.push_str(&masked);
            last = m.end();
        }
        out.push_str(&s[last..]);
        out
    };

    s = IPV4.replace_all(&s, "<IP>").into_owned();
    s = IPV6.replace_all(&s, "<IP>").into_owned();

    s = TEMP_PATH.replace_all(&s, "<TMPPATH>").into_owned();

    s = HEX_PREFIXED.replace_all(&s, "<HEX>").into_owned();
    s = HEX_ID
        .replace_all(&s, |caps: &regex::Captures| {
            let m = &caps[0];
            if m.chars()
                .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
            {
                "<HEX>".to_string()
            } else {
                m.to_string()
            }
        })
        .into_owned();

    s = QUANTITY.replace_all(&s, "<QTY>").into_owned();
    s = VERSION_NUM.replace_all(&s, "<NUM>").into_owned();
    s = BARE_NUM.replace_all(&s, "<NUM>").into_owned();

    s
}

/// True if `token` is a masking placeholder or a Drain wildcard, i.e. it should be
/// highlighted as a "variable" position when a template is rendered.
pub fn is_placeholder(token: &str) -> bool {
    token == "<*>" || (token.starts_with('<') && token.ends_with('>') && token.len() > 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> String {
        mask_line(s, &[])
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(m("\x1b[31mERROR\x1b[0m boom"), "ERROR boom");
    }

    #[test]
    fn masks_iso8601_and_rfc3339() {
        assert_eq!(m("2024-01-15T10:23:45.123456Z connected"), "<TS> connected");
        assert_eq!(m("2024-01-15 10:23:45,123 INFO start"), "<TS> INFO start");
        assert_eq!(m("2024-01-15T10:23:45+02:00 tick"), "<TS> tick");
    }

    #[test]
    fn masks_syslog_timestamp() {
        assert_eq!(m("Jan 15 10:23:45 host sshd: ok"), "<TS> host sshd: ok");
    }

    #[test]
    fn masks_epoch() {
        assert_eq!(m("ts=1700000000 event=boot"), "ts=<TS> event=boot");
        assert_eq!(m("ts=1700000000123 event=boot"), "ts=<TS> event=boot");
    }

    #[test]
    fn masks_uuid() {
        assert_eq!(
            m("request 123e4567-e89b-12d3-a456-426614174000 done"),
            "request <UUID> done"
        );
    }

    #[test]
    fn masks_email() {
        assert_eq!(
            m("user alice@example.com logged in"),
            "user <EMAIL> logged in"
        );
    }

    #[test]
    fn masks_ipv4_with_port() {
        assert_eq!(m("connect 10.0.0.5:8443 failed"), "connect <IP> failed");
    }

    #[test]
    fn masks_ipv6_uncompressed_and_bracketed() {
        assert_eq!(
            m("peer fe80:0000:0000:0000:0000:0000:0000:0001 joined"),
            "peer <IP> joined"
        );
        assert_eq!(m("dial [2001:db8::1]:8443 now"), "dial <IP> now");
        assert_eq!(m("dial [::1]:9000 now"), "dial <IP> now");
    }

    #[test]
    fn does_not_mask_rust_style_paths_as_ipv6() {
        // `::` is extremely common as a module-path separator; without look-around support
        // in the `regex` crate, a bare (unbracketed) compressed IPv6 address is out of
        // scope on purpose (see the comment on `IPV6`) to avoid this exact false positive.
        assert_eq!(
            m("called std::io::Read::read_to_string"),
            "called std::io::Read::read_to_string"
        );
        assert_eq!(
            m("panic in module a::b::c::d"),
            "panic in module a::b::c::d"
        );
    }

    #[test]
    fn masks_hex_hash_not_decimal() {
        assert_eq!(m("commit abc1234 pushed"), "commit <HEX> pushed");
        assert_eq!(m("retry 1234567 times"), "retry <NUM> times");
    }

    #[test]
    fn masks_url_query_and_ids() {
        assert_eq!(
            m("GET https://api.example.com/users/4200/orders/9f1c2b?token=xyz&x=1 200"),
            "GET https://api.example.com/users/<ID>/orders/<ID>?<QUERY> 200"
        );
    }

    #[test]
    fn masks_quantities() {
        assert_eq!(m("uploaded 512KiB in 12ms"), "uploaded <QTY> in <QTY>");
        assert_eq!(m("elapsed 3.4s"), "elapsed <QTY>");
    }

    #[test]
    fn masks_clock_duration() {
        assert_eq!(m("job finished in 01:23:45"), "job finished in <DUR>");
    }

    #[test]
    fn masks_temp_path() {
        assert_eq!(
            m("writing /tmp/pytest-of-root/pytest-12/test_foo0/data.json"),
            "writing <TMPPATH>"
        );
    }

    #[test]
    fn leaves_short_numbers_and_wordlike_tokens_alone() {
        assert_eq!(m("exit code 1"), "exit code 1");
        assert_eq!(m("utf8 sha256 log4j http2"), "utf8 sha256 log4j http2");
    }

    #[test]
    fn custom_mask_from_cli() {
        let custom = vec![CustomMask::from_cli(r"job-\d+").unwrap()];
        assert_eq!(
            mask_line("running job-42 now", &custom),
            "running <CUSTOM> now"
        );
    }

    #[test]
    fn custom_mask_from_config_named() {
        let custom = vec![CustomMask::from_config_line("TRACE=trace-[a-f0-9]+").unwrap()];
        assert_eq!(
            mask_line("span trace-deadbeef ok", &custom),
            "span <TRACE> ok"
        );
    }

    #[test]
    fn is_placeholder_detects_variables() {
        assert!(is_placeholder("<TS>"));
        assert!(is_placeholder("<*>"));
        assert!(!is_placeholder("plain"));
        assert!(!is_placeholder("<>"));
    }
}
