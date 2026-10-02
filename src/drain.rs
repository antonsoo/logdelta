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
//! **Variant used here, and why:** the original paper routes lines through `token_count`,
//! then up to `depth` further levels keyed by the first few *tokens* (with an early
//! wildcard branch for tokens containing digits), before reaching a leaf list of groups.
//! Because [`crate::mask`] already turns every digit-bearing variable field (timestamps,
//! ids, durations, ...) into a placeholder token *before* mining starts, the digit-based
//! branch collapses to a single case in practice, and CI/service logs rarely have enough
//! distinct log statements for a two-level index (`token_count`, first token) to leave more
//! than a handful of candidate groups per bucket. So this implementation uses that two-level
//! index instead of a fixed depth-N tree: it is simpler, still effectively O(1) average
//! lookup for realistic logs, and produces identical groupings to the full-depth tree
//! whenever the first token alone disambiguates the groups in a bucket (true in all of this
//! crate's fixtures). The wildcarding rule and default 0.5 similarity threshold are
//! unchanged from the paper; the similarity function itself has one deliberate addition (see
//! `similarity`'s doc comment): a position where both sides are the *same masking
//! placeholder* is left out of the comparison unless the line has no literal content at
//! all, because otherwise two genuinely unrelated lines that each merely contain a timestamp
//! (or any other masked field) can accumulate enough placeholder-vs-placeholder and
//! boilerplate-prefix matches to clear the threshold and merge into a useless,
//! over-generalized template.
//!
//! Processing is single-threaded and clusters are matched/created in input order with a
//! deterministic tie-break, so output (cluster ids, templates, counts) is a pure function of
//! the input stream.

use std::collections::HashMap;

use serde::Serialize;

use crate::mask::is_placeholder;

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
    // token_count -> first token -> candidate cluster ids. A first token that is itself a
    // wildcard indexes separately, since it can match lines with any first token.
    index: HashMap<(usize, String), Vec<usize>>,
    clusters: Vec<Cluster>,
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
        let Some(first) = tokens.first() else {
            let cid = *self.index.get(&(0, String::new()))?.first()?;
            self.clusters[cid].count += 1;
            return Some(cid);
        };
        let candidates = self.index.get(&(tokens.len(), first.clone()))?;

        let mut best: Option<(usize, f64)> = None;
        for &cid in candidates.iter() {
            let sim = similarity(&self.clusters[cid].tokens, tokens);
            if sim >= self.threshold && best.map(|(_, bs)| sim > bs).unwrap_or(true) {
                best = Some((cid, sim));
            }
        }

        let (cid, _) = best?;
        let cluster = &mut self.clusters[cid];
        for (t, l) in cluster.tokens.iter_mut().zip(tokens.iter()) {
            if t != l && t != "<*>" {
                *t = "<*>".to_string();
            }
        }
        cluster.count += 1;
        Some(cid)
    }

    fn start_cluster(&mut self, tokens: Vec<String>, line_no: usize, raw: &str) -> usize {
        let key = (tokens.len(), tokens.first().cloned().unwrap_or_default());
        let cid = self.clusters.len();
        self.clusters.push(Cluster {
            id: cid,
            tokens,
            count: 1,
            first_line_no: line_no,
            first_line_raw: raw.to_string(),
        });
        self.index.entry(key).or_default().push(cid);
        cid
    }

    /// Read-only membership query used by `logdelta novel`: true if some existing cluster's
    /// template matches `tokens` at or above the similarity threshold, without adding or
    /// mutating anything.
    pub fn contains_matching_template(&self, tokens: &[String]) -> bool {
        if tokens.is_empty() {
            return self.index.contains_key(&(0, String::new()));
        }
        let key = (tokens.len(), tokens[0].clone());
        let Some(candidates) = self.index.get(&key) else {
            return false;
        };
        candidates
            .iter()
            .any(|&cid| similarity(&self.clusters[cid].tokens, tokens) >= self.threshold)
    }
}

/// Fraction of the positions that say something which match, treating a `<*>` in `template`
/// as an automatic match. Both slices must be the same length (callers only compare within a
/// `token_count` bucket).
///
/// A position where *both* sides equal the same masking placeholder (`<TS>`, `<NUM>`, ...)
/// says nothing either way, and is left out of the fraction altogether, unless the template
/// has no literal (non-placeholder) tokens at all. Two lines that share nothing but "both
/// happen to have a timestamp somewhere" are not evidence they're the same kind of log line
/// (almost every line does), and counting it as a match let two genuinely unrelated lines
/// (an access-log line and a JSON error event, say) accumulate enough incidental
/// placeholder-vs-placeholder and boilerplate-prefix matches to clear the similarity
/// threshold and merge into a single, uselessly over-generalized template.
///
/// Leaving such a position out is not the same as counting it as a mismatch, which is what
/// this function did until 0.3.0: a line that is mostly placeholders, `<NUM> <NUM> <HEX> ok`,
/// then scored 1/4 against *itself*, never joined the template it had just started, and a
/// log of a hundred thousand such lines became a hundred thousand templates, each compared
/// against every one before it.
///
/// The exception (no literal tokens anywhere) keeps short, fully-templated lines like
/// `<TS> <NUM>` able to cluster with themselves: there, the placeholder sequence *is* the
/// only signal available.
fn similarity(template: &[String], line: &[String]) -> f64 {
    debug_assert_eq!(template.len(), line.len());
    if template.is_empty() {
        return 1.0;
    }
    let has_literal_content = template.iter().any(|t| t != "<*>" && !is_placeholder(t));
    let mut counted = 0usize;
    let mut matches = 0usize;
    for (t, l) in template.iter().zip(line.iter()) {
        if t == "<*>" {
            counted += 1;
            matches += 1;
        } else if t != l {
            counted += 1;
        } else if !(has_literal_content && is_placeholder(t)) {
            counted += 1;
            matches += 1;
        }
    }
    if counted == 0 {
        // Unreachable while `has_literal_content` means a literal token exists (it is counted
        // whether or not it matches); kept so the division below can never be 0/0.
        return 1.0;
    }
    matches as f64 / counted as f64
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
