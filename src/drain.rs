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
//! [`similarity`]'s doc comment): a position where both sides are the *same masking
//! placeholder* doesn't count as a match unless the line has no literal content at all,
//! because otherwise two genuinely unrelated lines that each merely contain a timestamp (or
//! any other masked field) can accumulate enough placeholder-vs-placeholder and
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
        if tokens.is_empty() {
            return self.upsert_empty(line_no, raw);
        }
        let key = (tokens.len(), tokens[0].clone());
        let candidates = self.index.entry(key.clone()).or_default();

        let mut best: Option<(usize, f64)> = None;
        for &cid in candidates.iter() {
            let sim = similarity(&self.clusters[cid].tokens, &tokens);
            if sim >= self.threshold && best.map(|(_, bs)| sim > bs).unwrap_or(true) {
                best = Some((cid, sim));
            }
        }

        if let Some((cid, _)) = best {
            let cluster = &mut self.clusters[cid];
            for (t, l) in cluster.tokens.iter_mut().zip(tokens.iter()) {
                if t != l && t != "<*>" {
                    *t = "<*>".to_string();
                }
            }
            cluster.count += 1;
            cid
        } else {
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

    fn upsert_empty(&mut self, line_no: usize, raw: &str) -> usize {
        let key = (0usize, String::new());
        if let Some(&cid) = self.index.get(&key).and_then(|v| v.first()) {
            self.clusters[cid].count += 1;
            return cid;
        }
        let cid = self.clusters.len();
        self.clusters.push(Cluster {
            id: cid,
            tokens: Vec::new(),
            count: 1,
            first_line_no: line_no,
            first_line_raw: raw.to_string(),
        });
        self.index.entry(key).or_default().push(cid);
        cid
    }
}

/// Fraction of positions that match, treating a `<*>` in `template` as an automatic match.
/// Both slices must be the same length (callers only compare within a `token_count` bucket).
///
/// A position where *both* sides equal the same masking placeholder (`<TS>`, `<NUM>`, ...)
/// does **not** count as a match, unless the compared lines have no literal (non-placeholder)
/// tokens at all. Two lines that share nothing but "both happen to have a timestamp
/// somewhere" are not evidence they're the same kind of log line — almost every line does —
/// and counting it as one let two genuinely unrelated lines (an access-log line and a JSON
/// error event, say) accumulate enough incidental placeholder-vs-placeholder and
/// boilerplate-prefix matches to clear the similarity threshold and merge into a single,
/// uselessly over-generalized template. The exception (no literal tokens anywhere) keeps
/// short, fully-templated lines like `<TS> <NUM>` still able to cluster with themselves —
/// there, the placeholder sequence *is* the only signal available.
fn similarity(template: &[String], line: &[String]) -> f64 {
    debug_assert_eq!(template.len(), line.len());
    if template.is_empty() {
        return 1.0;
    }
    let has_literal_content = template.iter().any(|t| t != "<*>" && !is_placeholder(t));
    let matches = template
        .iter()
        .zip(line.iter())
        .filter(|(t, l)| {
            if *t == "<*>" {
                return true;
            }
            if t != l {
                return false;
            }
            !(has_literal_content && is_placeholder(t))
        })
        .count();
    matches as f64 / template.len() as f64
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
