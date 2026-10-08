//! A Drain-inspired online log template miner.
//!
//! Reference: P. He, J. Zhu, Z. Zheng, M. R. Lyu, "Drain: An Online Log Parsing Approach
//! with Fixed Depth Tree", IEEE International Conference on Web Services (ICWS), 2017.
//!
//! Drain's core idea is a fixed-depth prefix tree that routes each incoming log line to a
//! small candidate list of existing "log groups" (clusters), each represented by a template
//! with wildcard (`<*>`) positions, then picks the best-matching group by token-position
//! similarity and either folds the line into it (generalizing any position that disagrees)
//! or starts a new group.
//!
//! **Variant used here, and why.** The paper routes a line through its token count and then
//! its first few tokens, and scores it against a group by the share of positions where the
//! two hold the same token. Three things differ here, each because a benchmark of real logs
//! (`studies/loghub`) showed the plain rule failing:
//!
//! - **Routing is by the first constant word, not the first token.** A line whose message
//!   starts with a value (`www.baidu.com:80 open through proxy ...`,
//!   `attempt_1445144423722_0020_m_000000_0 TaskAttempt Transitioned ...`,
//!   `1005 floating point alignment exceptions`) had a first token of its own, and so a
//!   group of its own. The index key is the token count and the first token that holds no
//!   digit and no placeholder.
//! - **Tokens are compared by shape.** `blk_38865049064139660` and `blk_7128370237687728475`
//!   are the same thing to a reader; so are `ambient=30` and `ambient=29`. The shape of a
//!   token is the token with every run of digits written `#` (see [`shape`]). A position
//!   whose shape is nothing but a number, or a masking placeholder, is no evidence either
//!   way: nearly every line has a timestamp somewhere.
//! - **A group's evidence is fixed when it is created.** Its weight is the number of
//!   positions that say something in its first line, and a later line joins when it agrees
//!   on at least the threshold's share *of that weight*. A position that has turned into
//!   `<*>` no longer agrees with anything, as in the paper. An earlier version counted a
//!   wildcard as agreement, so every wildcard made a group easier to join, and one group
//!   could swallow a log: 343 Android lines of 27 different templates, in the benchmark.
//!
//! [`crate::mask`] has already turned timestamps, ids and durations into placeholders
//! before mining starts; the threshold is the paper's 0.5.
//!
//! Processing is single-threaded and clusters are matched/created in input order with a
//! deterministic tie-break, so output (cluster ids, templates, counts) is a pure function of
//! the input stream.

use std::borrow::Cow;
use std::collections::HashMap;

use serde::Serialize;

/// A discovered log template ("group" in Drain's terminology).
#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    pub id: usize,
    /// Template tokens; a token equal to `<*>` is a wildcard position.
    pub tokens: Vec<String>,
    pub count: u64,
    /// 1-based line number of the first line that matched this cluster.
    pub first_line_no: usize,
    /// The original, unmasked text of that first line.
    pub first_line_raw: String,
    /// What a line is compared against, position by position: the shape every line so far
    /// has had there, or `None` once two of them differed. `tokens` is what is shown; a
    /// position can read `<*>` there (two block ids) and still have a shape here (`blk_#`).
    #[serde(skip)]
    shapes: Vec<Option<String>>,
    /// The key at each position that had one (`level` for `level=` and for `level="info"`).
    /// Every line of a cluster has the same keys in the same places.
    #[serde(skip)]
    keys: Vec<Option<String>>,
    /// The number of positions that held a word in the line that started the cluster, and
    /// the number that held an attribute.
    #[serde(skip)]
    words: usize,
    #[serde(skip)]
    attributes: usize,
}

impl Cluster {
    pub fn template(&self) -> String {
        self.tokens.join(" ")
    }
}

/// Default similarity threshold from the Drain paper (`st` in the original notation).
pub const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.5;

pub struct Drain {
    threshold: f64,
    // token count -> first constant word -> candidate cluster ids. A line with no constant
    // word at all is keyed by the empty string. Two levels, so that a line is looked up
    // by a borrowed word.
    index: HashMap<usize, HashMap<String, Vec<usize>>>,
    clusters: Vec<Cluster>,
}

/// What a line's tokens are for matching, worked out once however many clusters it is
/// compared with.
struct Seen<'a> {
    shapes: Vec<Cow<'a, str>>,
    keys: Vec<Option<&'a str>>,
    kinds: Vec<Kind>,
    route: &'a str,
}

impl<'a> Seen<'a> {
    fn of(tokens: &'a [String]) -> Self {
        let kinds: Vec<Kind> = tokens.iter().map(|t| kind(t)).collect();
        Seen {
            shapes: tokens.iter().map(|t| shape(t)).collect(),
            keys: tokens.iter().map(|t| attribute_key(t)).collect(),
            route: route(tokens, &kinds),
            kinds,
        }
    }
}

impl Default for Drain {
    fn default() -> Self {
        Self::new(DEFAULT_SIMILARITY_THRESHOLD)
    }
}

impl Drain {
    pub fn new(threshold: f64) -> Self {
        Drain {
            threshold,
            index: HashMap::new(),
            clusters: Vec::new(),
        }
    }

    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    pub fn into_clusters(self) -> Vec<Cluster> {
        self.clusters
    }

    /// Feeds one already-masked line (plus its 1-based line number and raw source text for
    /// reporting) into the miner and returns the id of the cluster it was assigned to.
    /// Convenience wrapper around [`Self::add_tokens`] that tokenizes by whitespace; prefer
    /// calling [`crate::mask::tokenize_line`] and [`Self::add_tokens`] directly for real log
    /// lines (JSON payloads need token boundaries whitespace-splitting can't produce).
    pub fn add_line(&mut self, masked: &str, line_no: usize, raw: &str) -> usize {
        let tokens: Vec<String> = masked.split_whitespace().map(str::to_owned).collect();
        self.add_tokens(tokens, line_no, raw)
    }

    /// Feeds one line's pre-built token sequence into the miner and returns the id of the
    /// cluster it was assigned to.
    pub fn add_tokens(&mut self, tokens: Vec<String>, line_no: usize, raw: &str) -> usize {
        match self.fold_into_existing(&tokens) {
            Some(cid) => cid,
            None => self.start_cluster(tokens, line_no, raw),
        }
    }

    /// [`Self::add_tokens`] for a caller that still needs the tokens afterwards: they are
    /// copied only when the line starts a cluster of its own, which few lines do.
    pub fn add_token_slice(&mut self, tokens: &[String], line_no: usize, raw: &str) -> usize {
        match self.fold_into_existing(tokens) {
            Some(cid) => cid,
            None => self.start_cluster(tokens.to_vec(), line_no, raw),
        }
    }

    /// Folds a line into the existing cluster it matches best (generalizing any position
    /// that disagrees) and returns that cluster's id; `None` when no cluster matches well
    /// enough. A line with no tokens always has a cluster once one such line has been seen.
    fn fold_into_existing(&mut self, tokens: &[String]) -> Option<usize> {
        if tokens.is_empty() {
            let cid = *self.index.get(&0)?.get("")?.first()?;
            self.clusters[cid].count += 1;
            return Some(cid);
        }
        let seen = Seen::of(tokens);
        let cid = self.best_match(&seen)?;
        let cluster = &mut self.clusters[cid];
        for (((shown, kept), token), shape) in cluster
            .tokens
            .iter_mut()
            .zip(cluster.shapes.iter_mut())
            .zip(tokens)
            .zip(&seen.shapes)
        {
            if shown != token && !is_wildcard(shown) {
                // An attribute keeps its name: `level=<*>`, not `<*>`.
                *shown = match attribute_key(token) {
                    Some(key) if !token.ends_with('=') => format!("{key}=<*>"),
                    _ => "<*>".to_string(),
                };
            }
            if kept.as_deref().is_some_and(|k| k != shape) {
                *kept = None;
            }
        }
        cluster.count += 1;
        Some(cid)
    }

    /// The cluster a line agrees with best, if it agrees with any at the threshold. Ties
    /// go to the cluster created first, so the result does not depend on hashing.
    fn best_match(&self, seen: &Seen) -> Option<usize> {
        let candidates = self.index.get(&seen.shapes.len())?.get(seen.route)?;
        let mut best: Option<(usize, f64)> = None;
        for &cid in candidates {
            let sim = similarity(&self.clusters[cid], seen);
            if sim >= self.threshold && best.is_none_or(|(_, bs)| sim > bs) {
                best = Some((cid, sim));
            }
        }
        best.map(|(cid, _)| cid)
    }

    fn start_cluster(&mut self, tokens: Vec<String>, line_no: usize, raw: &str) -> usize {
        let cid = self.clusters.len();
        let seen = Seen::of(&tokens);
        let route = seen.route.to_string();
        let shapes = seen.shapes.iter().map(|s| Some(s.to_string())).collect();
        let keys = seen.keys.iter().map(|k| k.map(str::to_owned)).collect();
        let words = seen.kinds.iter().filter(|k| **k == Kind::Word).count();
        let attributes = seen.kinds.iter().filter(|k| **k == Kind::Attribute).count();
        self.index
            .entry(tokens.len())
            .or_default()
            .entry(route)
            .or_default()
            .push(cid);
        self.clusters.push(Cluster {
            id: cid,
            shapes,
            keys,
            words,
            attributes,
            tokens,
            count: 1,
            first_line_no: line_no,
            first_line_raw: raw.to_string(),
        });
        cid
    }

    /// Read-only membership query used by `logdelta novel`: true if some existing cluster's
    /// template matches `tokens` at or above the similarity threshold, without adding or
    /// mutating anything.
    pub fn contains_matching_template(&self, tokens: &[String]) -> bool {
        if tokens.is_empty() {
            return self.index.get(&0).is_some_and(|m| m.contains_key(""));
        }
        self.best_match(&Seen::of(tokens)).is_some()
    }
}

/// The placeholders that stand for a number of some kind. Which of them a value gets
/// depends on how long it is or how it is punctuated, not on what it is: one line's byte
/// count is `<NUM>`, the next line's is ten digits starting with 1 and so `<TS>`.
const NUMERIC_PLACEHOLDERS: [&str; 6] = ["<NUM>", "<HEX>", "<TS>", "<DUR>", "<QTY>", "<ID>"];

/// A token with every number in it written `#`: a run of ASCII digits, a numeric
/// placeholder, a minus sign in front of either when it cannot be a hyphen
/// (`blk_-7988596544606686086`, `Translation=-24.0`), and numbers joined by the punctuation
/// of dates, times and versions (`2005.06.03`, `22:16:0:859`, `1.2.3`). So
/// `blk_38865049064139660` and `blk_-7128370237687728475` are both `blk_#`, `ambient=30`
/// is `ambient=#`, and `8`, `1005`, `<NUM>`, `<HEX>` and `12:01:03` are all `#`. Any other
/// placeholder stays itself.
///
/// Two tokens of one shape are, for grouping, the same token with a different value in it.
fn shape(token: &str) -> Cow<'_, str> {
    if !token.bytes().any(|b| b.is_ascii_digit() || b == b'<') {
        return Cow::Borrowed(token);
    }
    let bytes = token.as_bytes();
    // The length of the number starting at `i`, if one does.
    let number_at = |i: usize| -> Option<usize> {
        if bytes.get(i).is_some_and(u8::is_ascii_digit) {
            return Some(1);
        }
        NUMERIC_PLACEHOLDERS
            .iter()
            .find(|p| token[i..].starts_with(*p))
            .map(|p| p.len())
    };
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(len) = number_at(i) {
            if !out.ends_with('#') {
                out.push('#');
            }
            i += len;
        } else if bytes[i] == b'-'
            && number_at(i + 1).is_some()
            && (i == 0 || matches!(bytes[i - 1], b'_' | b'=' | b':' | b'(' | b'[' | b','))
        {
            i += 1; // the sign of the number that follows
        } else if matches!(bytes[i], b'-' | b':' | b'.' | b'/')
            && out.ends_with('#')
            && number_at(i + 1).is_some()
        {
            i += 1; // punctuation inside one value
        } else {
            let ch = token[i..].chars().next().expect("i is inside the token");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    Cow::Owned(out)
}

/// True for a template position that stands for any value: `<*>`, or `key=<*>` for an
/// attribute whose value varies.
pub fn is_wildcard(token: &str) -> bool {
    token == "<*>" || (token.ends_with("=<*>") && attribute_key(token).is_some())
}

/// The name of the field a token belongs to: `level` for the bare key `level=` (what JSON
/// flattening and logfmt put in front of a sentence) and for the attribute `level="info"`.
/// `None` for anything that is not `name=...` with a plain name: `a==b`, `--flag=x`, a
/// base64 string with its padding.
fn attribute_key(token: &str) -> Option<&str> {
    let (name, value) = token.split_once('=')?;
    let plain = !name.is_empty()
        && name.len() <= 40
        && (name.as_bytes()[0].is_ascii_alphabetic() || matches!(name.as_bytes()[0], b'_' | b'@'))
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'@'));
    (plain && !value.starts_with('=')).then_some(name)
}

/// What a token is, for comparing two lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A bare `key=`, or punctuation: part of how the line is laid out.
    Structure,
    /// `key=value`: something the event has, not something it says.
    Attribute,
    /// Holds at least one letter that is not part of a placeholder.
    Word,
    /// A number, a date, a placeholder: `8`, `<IP>,`, `[<UUID>]`.
    Value,
}

fn kind(token: &str) -> Kind {
    match attribute_key(token) {
        Some(_) if token.ends_with('=') => Kind::Structure,
        Some(_) => Kind::Attribute,
        None if has_word(token) => Kind::Word,
        None if token.bytes().any(|b| b.is_ascii_alphanumeric()) => Kind::Value,
        None => Kind::Structure,
    }
}

/// True if `token` holds a letter outside any placeholder.
fn has_word(token: &str) -> bool {
    let mut rest = token;
    while let Some(open) = rest.find('<') {
        if rest[..open].chars().any(char::is_alphabetic) {
            return true;
        }
        let after = &rest[open + 1..];
        let name = after
            .bytes()
            .take_while(|b| b.is_ascii_uppercase() || matches!(b, b'*' | b'!'))
            .count();
        rest = if name > 0 && after.as_bytes().get(name) == Some(&b'>') {
            &after[name + 1..]
        } else {
            after
        };
    }
    rest.chars().any(char::is_alphabetic)
}

/// The word a line is indexed under: its first token that is a constant word, with no
/// digit and no placeholder in it, or the empty string when it has none. The first token
/// itself is the paper's choice, and is a value in too many real lines (see the module
/// docs).
fn route<'a>(tokens: &'a [String], kinds: &[Kind]) -> &'a str {
    tokens
        .iter()
        .zip(kinds)
        .find(|(t, kind)| {
            **kind == Kind::Word && !t.bytes().any(|b| b.is_ascii_digit()) && !holds_placeholder(t)
        })
        .map_or("", |(t, _)| t.as_str())
}

/// True if a masking placeholder (`<TS>`, `<UUID>`, `<*>`) appears anywhere in `token`.
/// `<init>` and `<1` are not placeholders: those are upper-case names in angle brackets.
fn holds_placeholder(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut i = 0;
    while let Some(open) = bytes[i..].iter().position(|&b| b == b'<') {
        let start = i + open + 1;
        let name = bytes[start..]
            .iter()
            .take_while(|b| b.is_ascii_uppercase() || matches!(b, b'*' | b'!'))
            .count();
        if name > 0 && bytes.get(start + name) == Some(&b'>') {
            return true;
        }
        i = start;
    }
    false
}

/// How well a line agrees with a cluster, from 0 to 1. The line must be as long as the
/// cluster's template (callers only compare within a token-count bucket).
///
/// **Words decide.** The score is the share of the cluster's words (the positions that
/// held a word in its first line) at which the line still has the shape every line of the
/// cluster has had. The denominator never changes: a position that has become a wildcard
/// no longer agrees with anything, so a cluster can lose at most the share of its words
/// that the threshold allows. Counting a wildcard as agreement, as this function once did,
/// made a cluster easier to join with every line it took in.
///
/// **Values say nothing.** A number, a date or a placeholder is left out on both sides:
/// that two lines both have a timestamp is no evidence they are the same kind of line, and
/// that one has `8` where the other has `<NUM>` is none against it.
///
/// **Keys are structure.** A line with a different key where the cluster has one, or none,
/// is a different kind of line whatever else it shares, and scores 0. What an attribute's
/// value is does not count while there are words to go on: `level="info"` beside a message
/// is something the event has, and a log whose every line shares a service name and a
/// host would otherwise be one template.
///
/// **With no words,** a cluster of attributes (`user="alice" action="login"`) is compared
/// on their values, and a cluster of bare values (`<TS> <NUM>`, `0 0 0 0`) on its sequence
/// of shapes, which must be the same.
fn similarity(cluster: &Cluster, seen: &Seen) -> f64 {
    debug_assert_eq!(cluster.shapes.len(), seen.shapes.len());
    let mut words = 0usize;
    let mut attributes = 0usize;
    let mut identical = true;
    let mut line_has_word = false;
    for (i, kept) in cluster.shapes.iter().enumerate() {
        if cluster.keys[i].as_deref() != seen.keys[i] {
            return 0.0;
        }
        let what = seen.kinds[i];
        line_has_word |= what == Kind::Word;
        match kept.as_deref() {
            Some(k) if k == seen.shapes[i] => match what {
                Kind::Word => words += 1,
                Kind::Attribute => attributes += 1,
                Kind::Structure | Kind::Value => {}
            },
            Some(_) => identical = false,
            None => {}
        }
    }
    if cluster.words > 0 {
        words as f64 / cluster.words as f64
    } else if line_has_word {
        0.0
    } else if cluster.attributes > 0 {
        attributes as f64 / cluster.attributes as f64
    } else if identical {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(drain: &mut Drain, lines: &[&str]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| drain.add_line(l, i + 1, l))
            .collect()
    }

    #[test]
    fn identical_lines_form_one_cluster() {
        let mut d = Drain::default();
        let got = ids(&mut d, &["a b c", "a b c", "a b c"]);
        assert_eq!(got, vec![0, 0, 0]);
        assert_eq!(d.clusters()[0].count, 3);
        assert_eq!(d.clusters()[0].template(), "a b c");
    }

    #[test]
    fn one_varying_token_becomes_wildcard() {
        let mut d = Drain::default();
        ids(&mut d, &["connected to <IP>", "connected to <IP>"]);
        // same masked text both times: still one cluster, no wildcard needed
        assert_eq!(d.clusters().len(), 1);
        assert_eq!(d.clusters()[0].template(), "connected to <IP>");

        let mut d2 = Drain::default();
        ids(&mut d2, &["user alice logged in", "user bob logged in"]);
        assert_eq!(d2.clusters().len(), 1);
        assert_eq!(d2.clusters()[0].template(), "user <*> logged in");
        assert_eq!(d2.clusters()[0].count, 2);
    }

    #[test]
    fn different_token_counts_are_different_clusters() {
        let mut d = Drain::default();
        let got = ids(&mut d, &["a b c", "a b c d"]);
        assert_ne!(got[0], got[1]);
        assert_eq!(d.clusters().len(), 2);
    }

    #[test]
    fn below_threshold_similarity_starts_new_cluster() {
        let mut d = Drain::new(0.9);
        // 3 tokens, 1 shared out of 3 => similarity 0.33 < 0.9
        let got = ids(&mut d, &["start job one", "abort task two"]);
        assert_ne!(got[0], got[1]);
    }

    #[test]
    fn deterministic_across_runs() {
        let lines = [
            "user alice logged in",
            "user bob logged in",
            "disk usage 92 percent",
            "disk usage 10 percent",
            "user carol logged in",
        ];
        let mut d1 = Drain::default();
        let ids1 = ids(&mut d1, &lines);
        let mut d2 = Drain::default();
        let ids2 = ids(&mut d2, &lines);
        assert_eq!(ids1, ids2);
        let templates1: Vec<_> = d1.clusters().iter().map(Cluster::template).collect();
        let templates2: Vec<_> = d2.clusters().iter().map(Cluster::template).collect();
        assert_eq!(templates1, templates2);
    }

    #[test]
    fn records_first_occurrence() {
        let mut d = Drain::default();
        d.add_line("user alice logged in", 1, "raw: user alice logged in");
        d.add_line("user bob logged in", 5, "raw: user bob logged in");
        assert_eq!(d.clusters()[0].first_line_no, 1);
        assert_eq!(d.clusters()[0].first_line_raw, "raw: user alice logged in");
    }

    #[test]
    fn empty_lines_cluster_together() {
        let mut d = Drain::default();
        let got = ids(&mut d, &["", "", "a b"]);
        assert_eq!(got[0], got[1]);
        assert_ne!(got[0], got[2]);
    }

    #[test]
    fn shared_placeholder_prefix_does_not_merge_unrelated_lines() {
        // Regression test for the logdelta-diff hero bug: two unrelated 6-token lines that
        // both start with "<TS> stderr F" used to merge into a near-fully-wildcarded,
        // meaningless template (3/6 = 0.5 similarity, right at the default threshold) purely
        // because "<TS>" matched itself and the boilerplate "stderr"/"F" tokens matched too.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "<TS> stderr F alpha=1 beta=2 gamma=3",
                "<TS> stderr F goroutine 42 [running]:",
            ],
        );
        assert_ne!(
            got[0], got[1],
            "unrelated lines sharing only a <TS> + boilerplate prefix must not merge"
        );
    }

    #[test]
    fn a_line_that_is_mostly_placeholders_joins_its_own_template() {
        // One literal token in four. The masked positions say nothing about whether two lines
        // are the same kind of line; they must not count against it either.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "<NUM> <NUM> <HEX> ok",
                "<NUM> <NUM> <HEX> ok",
                "<NUM> <NUM> <HEX> ok",
            ],
        );
        assert_eq!(got, vec![0, 0, 0]);
        assert_eq!(d.clusters()[0].template(), "<NUM> <NUM> <HEX> ok");
        // The read-only query `novel` uses agrees.
        let tokens: Vec<String> = "<NUM> <NUM> <HEX> ok"
            .split(' ')
            .map(str::to_owned)
            .collect();
        assert!(d.contains_matching_template(&tokens));
    }

    #[test]
    fn every_line_matches_the_template_it_started() {
        // Whatever mix of literals and placeholders a line is, seeing it again must not start
        // a second template.
        let placeholders = ["<TS>", "<NUM>", "<HEX>", "<IP>", "<UUID>"];
        let literals = ["GET", "ok", "user", "conn", "x=1", "(<QTY>),"];
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        for _ in 0..2000 {
            let len = 1 + next(8);
            let line: Vec<&str> = (0..len)
                .map(|_| {
                    if next(3) == 0 {
                        literals[next(literals.len())]
                    } else {
                        placeholders[next(placeholders.len())]
                    }
                })
                .collect();
            let line = line.join(" ");
            let mut d = Drain::default();
            let got = ids(&mut d, &[&line, &line]);
            assert_eq!(got[0], got[1], "{line:?} did not join its own template");
        }
    }

    #[test]
    fn one_matching_word_among_placeholders_is_not_enough_to_merge_different_lines() {
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &["<NUM> <NUM> <HEX> ok", "<NUM> <NUM> <HEX> failed"],
        );
        assert_ne!(got[0], got[1]);
    }

    #[test]
    fn fully_templated_short_lines_still_cluster_on_placeholder_sequence_alone() {
        // The exception to the rule above: a line with *no* literal content at all has
        // nothing else to compare on, so an identical placeholder sequence should still count.
        let mut d = Drain::default();
        let got = ids(&mut d, &["<TS> <NUM>", "<TS> <NUM>"]);
        assert_eq!(got[0], got[1]);
        assert_eq!(d.clusters()[0].count, 2);
    }

    #[test]
    fn literal_tokens_still_generalize_around_a_real_shared_prefix() {
        // Sanity check that the stricter rule doesn't break the common, legitimate case: two
        // lines that share real literal content plus one placeholder should still merge.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &["user alice logged in at <TS>", "user bob logged in at <TS>"],
        );
        assert_eq!(got[0], got[1]);
        assert_eq!(d.clusters()[0].template(), "user <*> logged in at <TS>");
    }

    #[test]
    fn a_line_that_starts_with_a_value_is_grouped_by_its_words() {
        // Loghub's Proxifier, Hadoop and BGL messages: the first token is a host, an id, a
        // count. Indexed by it, each was a template of its own.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "proxy.cse.cuhk.edu.hk:5070 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS",
                "www.baidu.com:80 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS",
                "attempt_1445144423722_0020_m_000000_0 TaskAttempt Transitioned from NEW to UNASSIGNED",
                "attempt_1445144423722_0020_m_000001_0 TaskAttempt Transitioned from NEW to UNASSIGNED",
                "8 floating point alignment exceptions",
                "<NUM> floating point alignment exceptions",
                "<TS> floating point alignment exceptions",
            ],
        );
        assert_eq!(got, vec![0, 0, 1, 1, 2, 2, 2]);
    }

    #[test]
    fn tokens_that_differ_only_in_their_numbers_are_the_same_token() {
        assert_eq!(shape("blk_38865049064139660"), "blk_#");
        assert_eq!(shape("blk_-7128370237687728475"), "blk_#");
        assert_eq!(shape("ambient=30"), "ambient=#");
        assert_eq!(shape("Translation=-24.0"), "Translation=#");
        assert_eq!(
            shape("20171223-22:16:0:859|Step_LSC|30002312|onExtend:<TS>"),
            "#|Step_LSC|#|onExtend:#"
        );
        assert_eq!(shape("I1005"), "I#");
        assert_eq!(shape("part-00720."), "part-#.");
        for value in [
            "8",
            "1005",
            "<NUM>",
            "<HEX>",
            "<DUR>",
            "2005.06.03",
            "12:01:03",
        ] {
            assert_eq!(shape(value), "#", "{value}");
        }
        assert_eq!(shape("<IP>,"), "<IP>,");
        assert_eq!(shape("plain"), "plain");

        // klog writes the month and day into the first token of every line.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "I1004 <DUR> <NUM> controller.go:667] quota admission added evaluator for: pods",
                "I1005 <DUR> <NUM> controller.go:667] quota admission added evaluator for: jobs",
                "ambient=30",
                "ambient=29",
                "temperature=45",
            ],
        );
        assert_eq!(got, vec![0, 0, 1, 1, 2]);
    }

    #[test]
    fn a_template_does_not_get_easier_to_join_as_it_takes_lines_in() {
        // Android's logcat in Loghub: 343 lines of 27 templates ended up in one cluster,
        // because each wildcard a cluster gained counted as agreement with anything.
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "PhoneStatusBar: suspendAutohide now",
                "PhoneStatusBar: resumeAutohide now",
                "PhoneStatusBar: setLightsOn later",
                "NotificationManager: cancelNotification later",
                "NotificationManager: enqueueToast soon",
            ],
        );
        // The second joins the first (two of three words). After that the cluster agrees
        // with a line on `PhoneStatusBar:` and `now` only, and its wildcard on nothing.
        assert_eq!(got[0], got[1]);
        assert_ne!(got[2], got[0]);
        assert_ne!(got[3], got[0]);
        assert_ne!(got[3], got[2]);
        assert_ne!(got[4], got[3]);
        assert_eq!(d.clusters()[0].template(), "PhoneStatusBar: <*> now");
    }

    #[test]
    fn attributes_are_structure_and_words_decide() {
        // Two events with the same fields and different messages are different events.
        let mut d = Drain::default();
        let line = |level: &str, words: &str| -> Vec<String> {
            let mut tokens = vec![format!("level=\"{level}\""), "msg=".to_string()];
            tokens.extend(words.split(' ').map(str::to_owned));
            tokens.extend(["port=<NUM>".to_string(), "service=\"api\"".to_string()]);
            tokens
        };
        let started = d.add_tokens(line("info", "server started"), 1, "");
        let handled = d.add_tokens(line("info", "request handled"), 2, "");
        let failed = d.add_tokens(line("error", "database connection"), 3, "");
        assert_ne!(started, handled);
        assert_ne!(failed, started);
        assert_ne!(failed, handled);
        // The same message at another level is the same event, and its level a value of it.
        let again = d.add_tokens(line("warn", "request handled"), 4, "");
        assert_eq!(again, handled);
        assert_eq!(
            d.clusters()[handled].template(),
            "level=<*> msg= request handled port=<NUM> service=\"api\""
        );
        assert!(is_wildcard("level=<*>") && is_wildcard("<*>") && !is_wildcard("a=b"));

        // A different key in the same place is a different structure, whatever else agrees.
        let mut d = Drain::default();
        let a = d.add_line("user=alice logged in", 1, "");
        let b = d.add_line("host=alice logged in", 2, "");
        assert_ne!(a, b);
    }

    #[test]
    fn with_no_words_attributes_are_compared_and_then_shapes() {
        let mut d = Drain::default();
        let got = ids(
            &mut d,
            &[
                "user=alice action=login",
                "user=bob action=login",
                "user=bob action=logout zone=eu",
                "0 0 0 0",
                "<NUM> <NUM> <NUM> 240",
                "<IP> <UUID>",
                "<TS> <NUM>",
            ],
        );
        assert_eq!(got[0], got[1]);
        assert_ne!(got[2], got[0]);
        assert_eq!(got[3], got[4]);
        assert_ne!(got[5], got[6]);
        assert_eq!(d.clusters()[got[0]].template(), "user=<*> action=login");
    }

    #[test]
    fn add_tokens_matches_add_line_for_plain_whitespace_tokenizing() {
        let mut d = Drain::default();
        let id_a = d.add_line("user alice in", 1, "user alice in");
        let mut d2 = Drain::default();
        let id_b = d2.add_tokens(
            vec!["user".into(), "alice".into(), "in".into()],
            1,
            "user alice in",
        );
        assert_eq!(id_a, id_b);
        assert_eq!(d.clusters()[0].template(), d2.clusters()[0].template());
    }
}
