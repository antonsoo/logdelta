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
use serde_json::Value;

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

// Terminal escape sequences: CSI (colors, cursor movement, ...) and OSC (window titles, the
// hyperlinks newer compilers and package managers wrap file names in), the latter ended by
// BEL or by ESC \.
lazy_re!(
    ANSI,
    r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?"
);

/// `line` without its terminal escape sequences. A log captured from a program that thought
/// it was writing to a terminal is full of them; they are not part of what it says.
pub fn strip_escapes(line: &str) -> std::borrow::Cow<'_, str> {
    ANSI.replace_all(line, "")
}

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
// Timestamps that spell the month or the weekday, in one pass:
// - C `ctime()` / `asctime()`: "Sun Dec  4 04:47:44 2005", with or without the year, and
//   with a time zone before it as `date` prints ("Sun Dec  4 04:47:44 UTC 2005"). Apache's
//   error log, `date`, Python's `time.ctime()` and `git log` all write it.
// - RFC 2822 / HTTP dates: "Sun, 04 Dec 2005 04:47:44 GMT" or "... +0000".
// - Common Log Format, the access log of Apache and nginx: "04/Dec/2005:04:47:44 +0000".
lazy_re!(
    TS_NAMED,
    concat!(
        r"\b(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun) (?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) {1,2}\d{1,2} \d{2}:\d{2}:\d{2}(?:\.\d+)?(?: [A-Z]{2,5})?(?: \d{4})?\b",
        r"|\b(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d{1,2} (?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{4} \d{2}:\d{2}:\d{2}(?: (?:[A-Z]{2,5}|[+-]\d{4}))?",
        r"|\b\d{1,2}/(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)/\d{4}:\d{2}:\d{2}:\d{2}(?: [+-]\d{4})?",
    )
);
lazy_re!(
    MONTH_NAME,
    r"Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec"
);
// Bare epoch seconds or milliseconds, as their own token (10 or 13 digits).
lazy_re!(TS_EPOCH, r"\b1[0-9]{9}(?:[0-9]{3})?\b");

lazy_re!(
    UUID,
    r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"
);

lazy_re!(EMAIL, r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b");

// http(s) plus the connection-string schemes that actually show up in CI/service logs
// (database URLs, message queues, object storage), since those are exactly the URLs most
// likely to carry credentials worth masking.
lazy_re!(
    URL,
    r#"\b(?:https?|postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|rediss|amqps?|ftp|sftp|ssh|s3)://[^\s"'<>()\[\]]+"#
);

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
// Hex-looking ids/hashes (git SHAs, MD5/SHA-1/SHA-256/SHA-512 digests, request ids, ...): 7
// or more hex chars containing at least one letter a-f, so plain decimal numbers fall
// through to the numeric masker instead. No upper bound: an earlier version capped this at
// 40 (git-SHA length), which meant longer hashes like a 64-char sha256 digest had no valid
// `\b` inside them at all and were never masked. (The `regex` crate has no look-around, so
// the "contains a letter" check happens in the replacement closure rather than the pattern.)
lazy_re!(HEX_ID, r"\b[0-9a-fA-F]{7,}\b");

// Path-safe characters only (not `[^\s:]*`, which used to swallow trailing punctuation like
// a closing paren or comma straight out of the surrounding sentence into the placeholder).
lazy_re!(
    TEMP_PATH,
    r"(?:/tmp/|/var/tmp/|/var/folders/|\\AppData\\Local\\Temp\\|\\Temp\\)[A-Za-z0-9_.=-]*(?:[/\\][A-Za-z0-9_.=-]*)*"
);

// CRI/containerd raw log lines (what `kubectl logs` shows): "<ts> stdout F <line>" or
// "...P <partial line>". The timestamp and the F/P (full/partial) flag are pure noise for
// clustering; the stream name is real signal (worth keeping distinct: an error on stderr is
// a different story than the same text on stdout).
lazy_re!(CRI_PREFIX, r"^\S+\s+(stdout|stderr)\s+[FP]\s+(.*)$");
// journald/syslog with a hostname + process[pid]: header, e.g. "Jan 15 10:23:45 host
// sshd[1234]: Accepted password". Drops the hostname (usually the thing being compared
// across, not part of "what does this log line mean"); keeps the process name, with the pid
// itself genericized since it's an id, not a template-relevant token.
lazy_re!(
    JOURNALD_PREFIX,
    r"^[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\S+\s+([\w.-]+(?:\[\d+\])?):\s?(.*)$"
);
lazy_re!(PID_BRACKET, r"\[\d+\]");

/// Strips a Docker `json-file` log driver envelope (`{"log":"...","stream":"stdout","time":
/// "..."}`) if `s` is one, returning the stream name as a leading token and the unwrapped
/// `log` text as the remaining payload. Requires both `log` and (`stream` or `time`) so an
/// arbitrary application JSON log that happens to have a `log` field isn't misread as this
/// specific envelope format.
fn strip_docker_wrapper(s: &str) -> (Vec<String>, String) {
    let trimmed = s.trim_start();
    if !trimmed.starts_with('{') {
        return (Vec::new(), s.to_string());
    }
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) else {
        return (Vec::new(), s.to_string());
    };
    let has_envelope_shape = map.contains_key("stream") || map.contains_key("time");
    match map.get("log") {
        Some(Value::String(log)) if has_envelope_shape => {
            let prefix = map
                .get("stream")
                .and_then(Value::as_str)
                .map(|s| vec![s.to_string()])
                .unwrap_or_default();
            (prefix, log.trim_end_matches('\n').to_string())
        }
        _ => (Vec::new(), s.to_string()),
    }
}

/// A leading timestamp with nothing else structural around it (GitHub Actions' raw step
/// logs look like this: `2024-01-15T10:23:45.1234567Z <message>`).
fn strip_leading_timestamp(s: &str) -> Option<String> {
    for re in [&*TS_ISO, &*TS_SYSLOG, &*TS_SLASH] {
        if let Some(m) = re.find(s) {
            if m.start() == 0 {
                return Some(
                    s[m.end()..]
                        .trim_start_matches([' ', '\t', ':', '-'])
                        .to_string(),
                );
            }
        }
    }
    None
}

/// Recognizes CRI/containerd and journald/syslog headers, or else a bare leading timestamp,
/// and strips it, returning any literal token worth keeping (e.g. the stream name) plus the
/// remaining payload. A no-op (empty prefix, `s` returned unchanged) if nothing matches.
pub(crate) fn strip_structural_prefix(s: &str) -> (Vec<String>, String) {
    if let Some(caps) = CRI_PREFIX.captures(s) {
        return (vec![caps[1].to_string()], caps[2].to_string());
    }
    if let Some(caps) = JOURNALD_PREFIX.captures(s) {
        let process = PID_BRACKET.replace(&caps[1], "[<NUM>]").into_owned();
        return (vec![process], caps[2].to_string());
    }
    if let Some(rest) = strip_leading_timestamp(s) {
        return (Vec::new(), rest);
    }
    (Vec::new(), s.to_string())
}

/// Masks one JSON value for use as a single Drain token. Strings and numbers get the same
/// treatment as free text / bare numbers elsewhere; numbers are *always* masked here (unlike
/// the free-text `BARE_NUM` pass, which leaves short numbers alone) because a JSON value is
/// known, structurally, to be "the variable part" rather than incidental text.
fn mask_json_value(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{}\"", mask_body(s)),
        Value::Number(_) => "<NUM>".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => {
            let cap = 8;
            let inner: Vec<String> = items.iter().take(cap).map(mask_json_value).collect();
            let more = if items.len() > cap { ",…" } else { "" };
            format!("[{}{more}]", inner.join(","))
        }
        Value::Object(map) => {
            let cap = 8;
            let inner: Vec<String> = map
                .iter()
                .take(cap)
                .map(|(k, v)| format!("{k}={}", mask_json_value(v)))
                .collect();
            let more = if map.len() > cap { ",…" } else { "" };
            format!("{{{}{more}}}", inner.join(","))
        }
    }
}

/// If `payload` is a single-line JSON object, flattens it into `key=` / value token pairs
/// (two tokens per field, so Drain can wildcard just the value and keep the key literal —
/// see the module docs) instead of letting a naive whitespace split break a quoted message
/// like `"msg":"database connection failed"` into three unrelated tokens. Returns `None` for
/// anything that isn't a non-empty JSON object, so the caller falls back to the plain-text
/// pipeline (this also covers JSON arrays/scalars at the top level, which are rare in logs
/// and don't have a natural key to key a token on).
fn flatten_json_line(payload: &str) -> Option<Vec<String>> {
    let trimmed = payload.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let Value::Object(map) = serde_json::from_str::<Value>(trimmed).ok()? else {
        return None;
    };
    if map.is_empty() {
        return None;
    }
    let mut tokens = Vec::with_capacity(map.len() * 2);
    for (key, value) in &map {
        match value {
            // A sentence is mined as a sentence: the key, then its words. As one token it
            // could only be equal to another message or not, so `"user alice logged in"`
            // and `"user bob logged in"` were two unrelated values.
            Value::String(text) if text.split_whitespace().nth(1).is_some() => {
                tokens.push(format!("{key}="));
                split_tokens(&mask_body(text), &mut tokens);
            }
            // Anything else is an attribute of the event, one `key=value` token.
            _ => tokens.push(format!("{key}={}", mask_json_value(value))),
        }
    }
    Some(tokens)
}

/// How far ahead a quoted string or a bracketed field may close and still be one token.
const MAX_ATOM_BYTES: usize = 200;
/// A bracketed field of more words than this is a sentence in brackets, not a field.
const MAX_BRACKET_WORDS: usize = 8;

/// Where the double-quoted string opening at `open` closes, if it does so soon enough.
fn closing_quote(bytes: &[u8], open: usize) -> Option<usize> {
    let limit = bytes.len().min(open + 1 + MAX_ATOM_BYTES);
    let mut i = open + 1;
    while i < limit {
        match bytes[i] {
            b'\\' => i += 1,
            b'"' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Where the square bracket opening at `open` closes, if what it holds is a short field:
/// a thread name (`[IPC Server handler 14 on 62270]`), a request context
/// (`[req-<UUID> <HEX> <HEX> - - -]`), a date (`[Sun Dec 04 04:47:44 2005]`).
fn closing_bracket(bytes: &[u8], open: usize) -> Option<usize> {
    let limit = bytes.len().min(open + 1 + MAX_ATOM_BYTES);
    let mut depth = 1usize;
    let mut words = 1usize;
    let mut i = open + 1;
    while i < limit {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b' ' | b'\t' if !bytes[i - 1].is_ascii_whitespace() => {
                words += 1;
                if words > MAX_BRACKET_WORDS {
                    return None;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `key` when `token` is `key="several words"` (with or without a trailing comma or
/// semicolon), as logfmt writes a message; the words are then mined as words.
fn quoted_assignment(token: &str) -> Option<(&str, &str)> {
    let (key, rest) = token.split_once("=\"")?;
    let inner = rest
        .trim_end_matches([',', ';'])
        .strip_suffix('"')
        .filter(|inner| !inner.contains('"'))?;
    let identifier = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'@'));
    (identifier && inner.split_whitespace().nth(1).is_some()).then_some((key, inner))
}

/// Splits masked text into tokens at whitespace, with two exceptions. A short
/// square-bracketed field is one token however many words it holds: split at every space,
/// `[RMCommunicator Allocator]` and `[main]` gave the lines of one Java log statement
/// different lengths, and so different templates. And a quoted value after `key=` is read
/// whole, then written as the key and its words when it is a sentence (see
/// [`quoted_assignment`]).
fn split_tokens(masked: &str, out: &mut Vec<String>) {
    let bytes = masked.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            let close = match bytes[i] {
                b'"' if i > start && bytes[i - 1] == b'=' => closing_quote(bytes, i),
                b'[' => closing_bracket(bytes, i),
                _ => None,
            };
            i = close.unwrap_or(i) + 1;
        }
        // `char::is_whitespace` also splits on a few non-ASCII spaces; keep doing that.
        for piece in masked[start..i].split(|c: char| c.is_whitespace() && !c.is_ascii()) {
            match quoted_assignment(piece) {
                Some((key, words)) => {
                    out.push(format!("{key}="));
                    out.extend(words.split_whitespace().map(str::to_owned));
                }
                None if piece.is_empty() => {}
                // A bracketed field keeps its words and loses its padding: `[  6%]` and
                // `[ 93%]` are the same field.
                None if piece.bytes().any(|b| b.is_ascii_whitespace()) => {
                    out.push(piece.split_ascii_whitespace().collect::<Vec<_>>().join(" "));
                }
                None => out.push(piece.to_string()),
            }
        }
    }
}

// number + unit, e.g. "512KiB", "12 ms", "3.4s", "200Mbps"; and, first, a duration in more
// than one unit as Go and many CLIs print it: `1m6.046s`, `2h45m`, `1h2m3.5s`. gotestsum
// writes one after every package (`✓ pkg/util (1m6.046s)`), and `(57.002s)` when the
// package took less than a minute: one field, both forms.
lazy_re!(
    QUANTITY,
    concat!(
        r"\b\d+h(?:\d+m)?(?:\d+(?:\.\d+)?s)?\b|\b\d+m\d+(?:\.\d+)?s\b",
        r"|\b\d+(?:\.\d+)?\s?(?:ns|[uµ]s|ms|s|m|h|d|B|KB|KiB|MB|MiB|GB|GiB|TB|TiB|bps|Kbps|Mbps|Gbps)\b",
    )
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

// URLs are masked before the line's general timestamp/UUID passes run (see the comment in
// `mask_line`), so a URL path segment that's itself a timestamp or UUID needs its own check
// here rather than relying on those later passes to catch it.
fn mask_path_segment(seg: &str) -> String {
    if seg.is_empty() {
        return seg.to_string();
    }
    let looks_numeric = seg.chars().all(|c| c.is_ascii_digit());
    let looks_hex = seg.len() >= 6
        && seg.chars().all(|c| c.is_ascii_hexdigit())
        && seg.chars().any(|c| c.is_ascii_alphabetic());
    if UUID.is_match(seg) {
        return "<UUID>".to_string();
    }
    if TS_ISO.is_match(seg) || TS_EPOCH.is_match(seg) {
        return "<TS>".to_string();
    }
    if looks_numeric || looks_hex {
        "<ID>".to_string()
    } else {
        seg.to_string()
    }
}

/// Masks the variable parts of a `scheme://[user[:pass]@]host[:port][/path][?query]` URL:
/// basic-auth credentials in the authority, the query string, and any purely numeric / hex /
/// UUID path segment — keeping the scheme, host, port, and literal path segments intact so
/// that e.g. `/api/users/42/orders/9f1c...` and `/api/users/7/orders/aa21...` collapse to the
/// same template while the route itself stays legible.
fn mask_url_internals(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (before_query, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let (authority, path) = match before_query.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (before_query, None),
    };
    let masked_authority = match authority.rsplit_once('@') {
        Some((_credentials, host)) => format!("<CRED>@{host}"),
        None => authority.to_string(),
    };

    let mut out = format!("{scheme}://{masked_authority}");
    if let Some(p) = path {
        out.push('/');
        out.push_str(
            &p.split('/')
                .map(mask_path_segment)
                .collect::<Vec<_>>()
                .join("/"),
        );
    }
    if let Some(q) = query {
        if !q.is_empty() {
            out.push_str("?<QUERY>");
        }
    }
    out
}

/// The placeholder for a token that is a test runner's progress line, if it is one.
///
/// pytest, unittest, RSpec, minitest, PHPUnit and mocha's dot reporter print one character
/// per test: `.` for a pass, and a letter for anything else (`s` skipped, `x` expected
/// failure, `F` failed, `E` error). Which tests land on which line depends on timing and
/// worker count, so the same suite never prints the same line twice and every line would
/// be a template of its own. The run is masked as one thing, and as another when a failure
/// or error is among the dots, which is the one fact about such a line worth keeping.
///
/// A token qualifies when it has nothing but those characters, at least four of them, and
/// at least half are dots: an ellipsis (`...`) and a word (`FEES`) stay as they are.
fn progress_placeholder(token: &str) -> Option<&'static str> {
    if token.len() < 4 {
        return None;
    }
    let mut dots = 0;
    let mut failed = false;
    for b in token.bytes() {
        match b {
            b'.' => dots += 1,
            b'F' | b'E' => failed = true,
            b's' | b'S' | b'x' | b'X' | b'*' | b'U' | b'R' | b'I' | b'f' | b'e' => {}
            _ => return None,
        }
    }
    if dots < 3 || dots * 2 < token.len() {
        return None;
    }
    Some(if failed { "<PROGRESS!>" } else { "<PROGRESS>" })
}

/// A progress line's counter, with its digits masked: `6%]`, `[100%]`, `63`, `500`, `12%)`.
/// The percentage a line ends on depends on how many tests there are and how they were
/// batched, so it differs between two runs of the same suite like the dots do. Only a token
/// that is nothing but digits and counter punctuation is one; `0.12s` is not.
fn progress_counter(token: &str) -> Option<String> {
    let counter = token
        .bytes()
        .all(|b| b.is_ascii_digit() || b"%/[]()".contains(&b));
    if !counter || !token.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut out = String::with_capacity(token.len() + 4);
    let mut in_digits = false;
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push_str("<NUM>");
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(ch);
        }
    }
    Some(out)
}

/// Replaces every whitespace-separated token that [`progress_placeholder`] recognizes, and
/// the counters after it on the same line, leaving the whitespace as it was. `None` when
/// the line cannot hold one.
fn mask_progress_runs(s: &str) -> Option<String> {
    // A line with fewer than three dots has no such token; most lines are that.
    if s.bytes().filter(|&b| b == b'.').count() < 3 {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut seen_progress = false;
    let mut push_token = |out: &mut String, token: &str| {
        if let Some(placeholder) = progress_placeholder(token) {
            seen_progress = true;
            out.push_str(placeholder);
        } else if let Some(counter) = seen_progress.then(|| progress_counter(token)).flatten() {
            out.push_str(&counter);
        } else {
            out.push_str(token);
        }
    };
    let mut token_start: Option<usize> = None;
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                push_token(&mut out, &s[start..i]);
            }
            out.push(ch);
        } else if token_start.is_none() {
            token_start = Some(i);
        }
    }
    if let Some(start) = token_start {
        push_token(&mut out, &s[start..]);
    }
    Some(out)
}

/// Replaces every match of `re` in `s`. A masker that matches nothing leaves the line where
/// it is: most of them match nothing on most lines, and copying the line once per masker was
/// a dozen allocations a line.
fn replace_in(s: &mut String, re: &Regex, with: &str) {
    if let std::borrow::Cow::Owned(replaced) = re.replace_all(s, with) {
        *s = replaced;
    }
}

/// The regex-masking pipeline: everything in this module's doc comment, applied in a fixed
/// order, to text that's already had ANSI escapes and any custom masks handled. This is the
/// part that's reusable for masking a *piece* of a line (a JSON field value, a URL path
/// segment) as well as a whole one — see [`tokenize_line`], which is the real entry point
/// for turning a raw log line into Drain tokens; [`mask_line`] (ANSI + custom + this, with no
/// further structure) exists mainly so each masker can be unit-tested against plain text.
fn mask_body(s: &str) -> String {
    let mut s = s.to_string();

    // URLs are masked before anything else touches the rest of the line (timestamps, UUIDs,
    // emails, ...): every one of those maskers inserts a `<PLACEHOLDER>` containing `<`/`>`,
    // and the URL matcher stops at the first `<`/`>` it sees (so it doesn't re-match text
    // some earlier pass already masked) — so if they ran first, a URL with a timestamp or
    // credentials in it would get truncated right before the placeholder and only half-mask.
    // Masking URLs first means their *internal* structure (credentials, query string, id-like
    // path segments — see `mask_url_internals`) has to be handled by dedicated logic, not by
    // falling through to the later general-purpose passes.
    if URL.is_match(&s) {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for m in URL.find_iter(&s) {
            out.push_str(&s[last..m.start()]);
            let masked = mask_url_internals(m.as_str());
            out.push_str(&masked);
            last = m.end();
        }
        out.push_str(&s[last..]);
        s = out;
    }

    if let Some(masked) = mask_progress_runs(&s) {
        s = masked;
    }

    replace_in(&mut s, &TS_ISO, "<TS>");
    // Every one of these has a month name in it, and most lines have none. The named
    // forms go first: the syslog form is the middle of one of them.
    if MONTH_NAME.is_match(&s) {
        replace_in(&mut s, &TS_NAMED, "<TS>");
        replace_in(&mut s, &TS_SYSLOG, "<TS>");
    }
    replace_in(&mut s, &TS_SLASH, "<TS>");
    replace_in(&mut s, &TS_EPOCH, "<TS>");
    replace_in(&mut s, &DURATION_CLOCK, "<DUR>");

    replace_in(&mut s, &UUID, "<UUID>");
    replace_in(&mut s, &EMAIL, "<EMAIL>");

    replace_in(&mut s, &IPV4, "<IP>");
    replace_in(&mut s, &IPV6, "<IP>");

    replace_in(&mut s, &TEMP_PATH, "<TMPPATH>");

    replace_in(&mut s, &HEX_PREFIXED, "<HEX>");
    let hex_ids = HEX_ID.replace_all(&s, |caps: &regex::Captures| {
        let m = &caps[0];
        if m.chars()
            .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
        {
            "<HEX>".to_string()
        } else {
            m.to_string()
        }
    });
    if let std::borrow::Cow::Owned(replaced) = hex_ids {
        s = replaced;
    }

    replace_in(&mut s, &QUANTITY, "<QTY>");
    replace_in(&mut s, &VERSION_NUM, "<NUM>");
    replace_in(&mut s, &BARE_NUM, "<NUM>");

    s
}

/// Strips ANSI escapes, applies any user `custom` masks, then `mask_body`. Useful on its
/// own for testing/demonstrating a masker against plain text; real log lines should go
/// through [`tokenize_line`] instead, which additionally strips structural envelopes and
/// handles JSON payloads before this pipeline ever sees them.
pub fn mask_line(line: &str, custom: &[CustomMask]) -> String {
    let mut s = strip_escapes(line).into_owned();
    for cm in custom {
        s = cm
            .regex
            .replace_all(&s, cm.placeholder.as_str())
            .into_owned();
    }
    mask_body(&s)
}

/// The real entry point: turns one raw log line into the token sequence Drain (see
/// [`crate::drain`]) mines templates from.
///
/// Order: strip ANSI and apply custom masks on the raw line, then peel off any recognized
/// structural envelope — a Docker `json-file` wrapper, a CRI/containerd `<ts> stdout F `
/// prefix, a journald/syslog header, or a bare leading timestamp — keeping only the literal
/// part of it worth comparing on (e.g. the stream name) as leading tokens. What's left is
/// either a single-line JSON object, flattened into `key=`/value token pairs so a quoted
/// multi-word message doesn't get shredded by a naive whitespace split (see
/// `flatten_json_line`), or plain text, run through `mask_body` and split on whitespace
/// as before.
pub fn tokenize_line(line: &str, custom: &[CustomMask]) -> Vec<String> {
    let mut s = strip_escapes(line).into_owned();
    for cm in custom {
        s = cm
            .regex
            .replace_all(&s, cm.placeholder.as_str())
            .into_owned();
    }

    let (docker_prefix, s) = strip_docker_wrapper(&s);
    let (struct_prefix, payload) = strip_structural_prefix(&s);

    let mut tokens = Vec::with_capacity(4);
    tokens.extend(docker_prefix);
    tokens.extend(struct_prefix);

    if let Some(json_tokens) = flatten_json_line(&payload) {
        tokens.extend(json_tokens);
    } else {
        split_tokens(&mask_body(&payload), &mut tokens);
    }
    tokens
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
    fn masks_test_runner_progress_lines() {
        // pytest: the same suite prints a different arrangement every run, and a different
        // percentage at the end of each line.
        assert_eq!(
            m("....s..........................x......................................... [  6%]"),
            "<PROGRESS> [  <NUM>%]"
        );
        assert_eq!(
            m("s....................................................................... [  7%]"),
            "<PROGRESS> [  <NUM>%]"
        );
        assert_eq!(m("........ [100%]"), "<PROGRESS> [<NUM>%]");
        assert_eq!(m(".s.s.s.s"), "<PROGRESS>");
        // A failure or an error among the dots is kept as a different template.
        assert_eq!(m("......F......E... [ 42%]"), "<PROGRESS!> [ <NUM>%]");
        // PHPUnit and RSpec.
        assert_eq!(
            m("...............................................................  63 / 500 ( 12%)"),
            "<PROGRESS>  <NUM> / <NUM> ( <NUM>%)"
        );
        assert_eq!(m("..*..F..."), "<PROGRESS!>");
        // With colors, as a CI log has them.
        assert_eq!(
            m("\x1b[32m.\x1b[0m\x1b[32m.\x1b[0m\x1b[33ms\x1b[0m\x1b[32m.\x1b[0m\x1b[32m.\x1b[0m [ 50%]"),
            "<PROGRESS> [ <NUM>%]"
        );
        // Every such line of a run is then one template.
        assert_eq!(
            tokenize_line("....s....................x.............. [  6%]", &[]),
            tokenize_line(
                "s.......................................................... [ 93%]",
                &[]
            )
        );
    }

    #[test]
    fn leaves_ordinary_dots_and_words_alone() {
        assert_eq!(m("waiting for the server..."), "waiting for the server...");
        assert_eq!(m("retrying ... done"), "retrying ... done");
        assert_eq!(m("FEES and EXXES"), "FEES and EXXES");
        assert_eq!(m("see ../../src/lib.rs"), "see ../../src/lib.rs");
        assert_eq!(m("sss..x"), "sss..x");
        assert_eq!(m("Fs.E"), "Fs.E");
        // A leader of dots is one thing too, whatever its length.
        assert_eq!(
            m("test_login ............ PASSED"),
            "test_login <PROGRESS> PASSED"
        );
        assert_eq!(
            m("test_logout .................... PASSED"),
            "test_logout <PROGRESS> PASSED"
        );
        // Only a counter after the dots is masked with them: a duration keeps its own masker.
        assert_eq!(
            m("suite ........ 12 passed in 0.12s"),
            "suite <PROGRESS> <NUM> passed in <QTY>"
        );
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(m("\x1b[31mERROR\x1b[0m boom"), "ERROR boom");
        // An OSC 8 hyperlink around a file name, ended by ESC \ or by BEL.
        assert_eq!(
            m("warning: \x1b]8;;file:///src/main.rs\x1b\\src/main.rs\x1b]8;;\x1b\\ unused"),
            "warning: src/main.rs unused"
        );
        assert_eq!(m("\x1b]0;building\x07done"), "done");
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
    fn masks_hex_hash_regardless_of_length() {
        // A 40-char git SHA-1 and a 64-char sha256 digest should both be masked; an earlier
        // version capped the pattern at 40 chars, which meant a longer hex run had no valid
        // `\b` boundary inside it anywhere and was silently left unmasked.
        assert_eq!(
            m("commit 1234567890abcdef1234567890abcdef12345678 pushed"),
            "commit <HEX> pushed"
        );
        assert_eq!(
            m("layer sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 pulled"),
            "layer sha256:<HEX> pulled"
        );
    }

    #[test]
    fn masks_url_query_and_ids() {
        assert_eq!(
            m("GET https://api.example.com/users/4200/orders/9f1c2b?token=xyz&x=1 200"),
            "GET https://api.example.com/users/<ID>/orders/<ID>?<QUERY> 200"
        );
    }

    #[test]
    fn masks_timestamp_and_uuid_inside_url_path() {
        // URLs are masked before the line-wide timestamp/UUID passes run (see the comment
        // in `mask_line`), so this exercises the path-segment-level fallback in
        // `mask_path_segment` that keeps those still covered inside a URL.
        assert_eq!(
            m("GET https://api.example.com/events/2024-01-15T10:23:45Z 200"),
            "GET https://api.example.com/events/<TS> 200"
        );
        assert_eq!(
            m("GET https://api.example.com/jobs/123e4567-e89b-12d3-a456-426614174000 200"),
            "GET https://api.example.com/jobs/<UUID> 200"
        );
    }

    #[test]
    fn masks_url_credentials_and_non_http_schemes() {
        assert_eq!(
            m("connecting to postgres://admin:hunter2@db.internal:5432/orders"),
            "connecting to postgres://<CRED>@db.internal:<NUM>/orders"
        );
        assert_eq!(
            m("mongo mongodb+srv://svc:s3cr3t@cluster0.example.net/mydb"),
            "mongo mongodb+srv://<CRED>@cluster0.example.net/mydb"
        );
        // no credentials: authority is left alone
        assert_eq!(
            m("cache redis://cache.internal:6379/0"),
            "cache redis://cache.internal:<NUM>/<ID>"
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
    fn temp_path_does_not_swallow_trailing_punctuation() {
        // A greedy `[^\s:]*` used to eat the closing paren/comma right along with the path.
        assert_eq!(
            m("(see /tmp/pytest-of-root/pytest-12/data.json) done"),
            "(see <TMPPATH>) done"
        );
        assert_eq!(
            m("wrote /tmp/out.bin, then exited"),
            "wrote <TMPPATH>, then exited"
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

    fn tok(s: &str) -> Vec<String> {
        tokenize_line(s, &[])
    }

    #[test]
    fn strips_cri_prefix_and_keeps_stream_name() {
        assert_eq!(
            tok("2024-03-02T08:15:00.123456789Z stdout F server ready"),
            vec!["stdout", "server", "ready"]
        );
        assert_eq!(
            tok("2024-03-02T08:15:00.123456789Z stderr F oops"),
            vec!["stderr", "oops"]
        );
    }

    #[test]
    fn strips_journald_prefix_keeps_process_genericizes_pid() {
        assert_eq!(
            tok("Jan 15 10:23:45 host sshd[1234]: Accepted password"),
            vec!["sshd[<NUM>]", "Accepted", "password"]
        );
    }

    #[test]
    fn strips_bare_leading_timestamp() {
        // The GitHub Actions raw-log shape: an ISO timestamp with nothing else structural.
        assert_eq!(
            tok("2024-01-15T10:23:45.1234567Z Run actions/checkout@v7"),
            vec!["Run", "actions/checkout@v7"]
        );
    }

    #[test]
    fn unwraps_docker_json_file_envelope() {
        let line =
            r#"{"log":"listening on :8080\n","stream":"stdout","time":"2024-01-15T10:23:45Z"}"#;
        assert_eq!(tok(line), vec!["stdout", "listening", "on", ":<NUM>"]);
    }

    #[test]
    fn flattens_json_payload_into_key_value_token_pairs() {
        let line = r#"2024-03-02T08:15:00Z stdout F {"level":"info","msg":"server listening","addr":"0.0.0.0:8080"}"#;
        assert_eq!(
            tok(line),
            vec![
                "stdout",
                "addr=\"<IP>\"",
                "level=\"info\"",
                "msg=",
                "server",
                "listening",
            ]
        );
    }

    #[test]
    fn a_quoted_sentence_after_a_key_is_its_words_and_a_quoted_word_stays_whole() {
        assert_eq!(
            tok(r#"level=error msg="database connection failed" service=api tag="x""#),
            vec![
                "level=error",
                "msg=",
                "database",
                "connection",
                "failed",
                "service=api",
                "tag=\"x\"",
            ]
        );
        // A quoted string that is not a field's value is split as before.
        assert_eq!(
            tok(r#"<IP> "GET /health HTTP/1.1" ok"#).len(),
            tok(r#"<IP> "POST /login HTTP/1.1" ok"#).len()
        );
        assert_eq!(tok(r#"say "hello there" twice"#).len(), 4);
    }

    #[test]
    fn a_short_bracketed_field_is_one_token_whatever_it_holds() {
        assert_eq!(
            tok("INFO [IPC Server handler 14 on 62270] org.apache.Foo: ready"),
            vec![
                "INFO",
                "[IPC Server handler 14 on <NUM>]",
                "org.apache.Foo:",
                "ready"
            ]
        );
        assert_eq!(tok("INFO [main] org.apache.Foo: ready").len(), 4);
        assert_eq!(
            tok("[Sun Dec 04 04:47:44 2005] [notice] ok"),
            vec!["[<TS>]", "[notice]", "ok"]
        );
        // A sentence in brackets is still a sentence, and an unclosed bracket is a character.
        assert_eq!(
            tok("[one two three four five six seven eight nine ten] done").len(),
            11
        );
        assert_eq!(tok("array[ index out of range").len(), 5);
    }

    #[test]
    fn masks_durations_in_more_than_one_unit() {
        assert_eq!(m("ok pkg/util (1m6.046s)"), "ok pkg/util (<QTY>)");
        assert_eq!(m("ok pkg/util (57.002s)"), "ok pkg/util (<QTY>)");
        assert_eq!(m("took 2h45m, then 1h2m3.5s"), "took <QTY>, then <QTY>");
        // Not a duration: a word that starts with digits.
        assert_eq!(m("the 3m tape and 5hours"), "the <QTY> tape and 5hours");
    }

    #[test]
    fn masks_the_timestamps_of_web_servers_and_of_ctime() {
        assert_eq!(
            m("[Sun Dec 04 04:47:44 2005] [notice] ok"),
            "[<TS>] [notice] ok"
        );
        assert_eq!(m("Sun Dec  4 04:47:44 UTC 2005 started"), "<TS> started");
        assert_eq!(m("at Mon Jun 20 03:40:59 2005"), "at <TS>");
        assert_eq!(
            m(r#"<IP> - - [04/Dec/2005:04:47:44 +0000] "GET / HTTP/1.1" 200"#),
            r#"<IP> - - [<TS>] "GET / HTTP/<NUM>" 200"#
        );
        assert_eq!(m("Date: Sun, 04 Dec 2005 04:47:44 GMT"), "Date: <TS>");
    }

    #[test]
    fn json_numbers_are_always_masked_even_when_short() {
        // Unlike the free-text pipeline (which leaves short numbers like exit codes alone),
        // a JSON value is structurally "the variable part" by construction.
        let line = r#"{"retries":3}"#;
        assert_eq!(tok(line), vec!["retries=<NUM>"]);
    }

    #[test]
    fn non_object_json_falls_back_to_plain_text_tokenizing() {
        assert_eq!(tok("[1,2,3]"), vec!["[1,2,3]"]);
    }

    #[test]
    fn plain_text_with_no_recognized_prefix_is_unaffected() {
        assert_eq!(
            tok("user alice logged in"),
            vec!["user", "alice", "logged", "in"]
        );
    }
}
