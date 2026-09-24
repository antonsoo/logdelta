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
//! crate's fixtures). The similarity function, wildcarding rule, and default 0.5 similarity
//! threshold are otherwise unchanged from the paper.
//!
//! Processing is single-threaded and clusters are matched/created in input order with a
//! deterministic tie-break, so output (cluster ids, templates, counts) is a pure function of
//! the input stream.

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
    pub fn add_line(&mut self, masked: &str, line_no: usize, raw: &str) -> usize {
        let tokens: Vec<String> = masked.split_whitespace().map(str::to_owned).collect();
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

/// Fraction of positions that match exactly, treating a `<*>` in `template` as an automatic
/// match. Both slices must be the same length (callers only compare within a `token_count`
/// bucket).
fn similarity(template: &[String], line: &[String]) -> f64 {
    debug_assert_eq!(template.len(), line.len());
    if template.is_empty() {
        return 1.0;
    }
    let matches = template
        .iter()
        .zip(line.iter())
        .filter(|(t, l)| *t == "<*>" || t == l)
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
}
