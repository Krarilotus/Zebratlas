//! Search over ids, disease names/synonyms/abbreviations, HPO labels/synonyms (incl. layperson)
//! and gene symbols. Tiers: exact normalised match, then prefix, then token match (`fuzzy`).
//!
//! Keys are [`normalize_label`] forms (ids too: `HP:0001250` -> `hp 0001250`), held in sorted
//! arrays so exact and prefix lookups are binary searches.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::atlas::Gene;
use crate::curie;
use crate::disease::Disease;
use crate::node::{NodeKey, NodeKind};
use crate::ontology::Ontology;
use crate::text::normalize_label;

pub mod domain;
pub mod messages;

/// Which field matched (API.md `match_kind`); `Fuzzy` for token-level matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    Id,
    Name,
    Abbreviation,
    Synonym,
    Layperson,
    Fuzzy,
}

/// Match strength, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Exact,
    Prefix,
    Token,
}

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    pub limit: usize,
    /// Also return retired diseases.
    pub include_retired: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            include_retired: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit<'a> {
    pub node: NodeKey,
    /// The indexed text that matched, as written in the source.
    pub matched: &'a str,
    pub match_kind: MatchKind,
    pub tier: Tier,
}

#[derive(Debug)]
struct Entry {
    node: NodeKey,
    text: Box<str>,
    key: Box<str>,
    field: MatchKind,
    retired: bool,
}

#[derive(Debug, Default)]
pub struct SearchIndex {
    entries: Vec<Entry>,
    /// Entry indexes sorted by key.
    by_key: Vec<u32>,
    /// (token, entry), sorted.
    tokens: Vec<(Box<str>, u32)>,
    /// Unique tokens bucketed by Unicode length; corrections visit only feasible lengths.
    corrections: BTreeMap<usize, Vec<CorrectionToken>>,
}

#[derive(Debug)]
struct CorrectionToken {
    text: Box<str>,
    chars: Box<[char]>,
}

/// Short all-caps names without spaces (`DEE4`, `STXBP1-DEE`) count as abbreviations.
fn looks_like_abbreviation(text: &str) -> bool {
    let n = text.chars().count();
    (2..=12).contains(&n)
        && !text.contains(char::is_whitespace)
        && text.chars().any(char::is_uppercase)
        && !text.chars().any(char::is_lowercase)
}

struct Builder {
    entries: Vec<Entry>,
    seen: HashSet<(NodeKey, Box<str>)>,
}

impl Builder {
    fn add(&mut self, node: NodeKey, text: &str, field: MatchKind, retired: bool) {
        let key: Box<str> = normalize_label(text).into();
        if key.is_empty() || !self.seen.insert((node, key.clone())) {
            return;
        }
        self.entries.push(Entry {
            node,
            text: text.into(),
            key,
            field,
            retired,
        });
    }
}

impl SearchIndex {
    /// Index every live HPO term, every disease (retired flagged) and every gene.
    pub fn build(hpo: &Ontology, diseases: &[Disease], genes: &[Gene]) -> Self {
        let mut b = Builder {
            entries: Vec::new(),
            seen: HashSet::new(),
        };
        for (i, d) in diseases.iter().enumerate() {
            let node = NodeKey {
                kind: NodeKind::Disease,
                idx: i as u32,
            };
            let retired = !d.is_active();
            b.add(node, &d.id, MatchKind::Id, retired);
            for s in &d.source_ids {
                b.add(node, s, MatchKind::Id, retired);
            }
            b.add(node, &d.name, MatchKind::Name, retired);
            for n in &d.synonyms {
                let abbreviation = n
                    .kind
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case("abbreviation"))
                    || looks_like_abbreviation(&n.text);
                let field = if abbreviation {
                    MatchKind::Abbreviation
                } else {
                    MatchKind::Synonym
                };
                b.add(node, &n.text, field, retired);
            }
        }
        for (i, g) in genes.iter().enumerate() {
            let node = NodeKey {
                kind: NodeKind::Gene,
                idx: i as u32,
            };
            b.add(node, &g.symbol, MatchKind::Name, false);
            for id in [&g.hgnc, &g.ncbi_gene].into_iter().flatten() {
                b.add(node, id, MatchKind::Id, false);
            }
        }
        for (i, t) in hpo.terms().iter().enumerate().filter(|(_, t)| !t.obsolete) {
            let node = NodeKey {
                kind: NodeKind::Phenotype,
                idx: i as u32,
            };
            b.add(node, &t.id, MatchKind::Id, false);
            for a in &t.alt_ids {
                b.add(node, a, MatchKind::Id, false);
            }
            b.add(node, &t.name, MatchKind::Name, false);
            for s in &t.synonyms {
                let field = if s.is_kind("layperson") {
                    MatchKind::Layperson
                } else if s.is_kind("abbreviation") {
                    MatchKind::Abbreviation
                } else {
                    MatchKind::Synonym
                };
                b.add(node, &s.text, field, false);
            }
        }
        let entries = b.entries;
        let mut by_key: Vec<u32> = (0..entries.len() as u32).collect();
        by_key.sort_unstable_by(|&a, &b| entries[a as usize].key.cmp(&entries[b as usize].key));
        let mut tokens: Vec<(Box<str>, u32)> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            for t in e.key.split(' ') {
                tokens.push((t.into(), i as u32));
            }
        }
        tokens.sort_unstable();
        tokens.dedup();
        let mut corrections: BTreeMap<usize, Vec<CorrectionToken>> = BTreeMap::new();
        let mut previous = "";
        for (text, _) in &tokens {
            if text.as_ref() == previous {
                continue;
            }
            previous = text;
            let chars: Box<[char]> = text.chars().collect();
            if chars.len() >= 3 {
                corrections.entry(chars.len()).or_default().push(CorrectionToken {
                    text: text.clone(),
                    chars,
                });
            }
        }
        Self {
            entries,
            by_key,
            tokens,
            corrections,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bounded exact entity linking for text intake. Unlike broad autocomplete, this never
    /// visits prefix or token ranges; even common words cost only two binary searches.
    pub fn exact(&self, query: &str, limit: usize) -> Vec<Hit<'_>> {
        let q = curie::normalize_query(query)
            .map(|id| normalize_label(&id))
            .unwrap_or_else(|| normalize_label(query));
        let lo = self
            .by_key
            .partition_point(|&i| self.entries[i as usize].key.as_ref() < q.as_str());
        let hi = self
            .by_key
            .partition_point(|&i| self.entries[i as usize].key.as_ref() <= q.as_str());
        let mut nodes = HashSet::new();
        self.by_key[lo..hi]
            .iter()
            .filter_map(|&i| {
                let e = &self.entries[i as usize];
                (!e.retired && nodes.insert(e.node)).then_some(Hit {
                    node: e.node,
                    matched: &e.text,
                    match_kind: e.field,
                    tier: Tier::Exact,
                })
            })
            .take(limit)
            .collect()
    }

    /// Best hit per node, strongest first: tier, field, node kind, shorter text, index.
    pub fn search(&self, query: &str, opts: SearchOptions) -> Vec<Hit<'_>> {
        let q = match curie::normalize_query(query) {
            Some(id) => normalize_label(&id),
            None => normalize_label(query),
        };
        if q.is_empty() || opts.limit == 0 {
            return Vec::new();
        }
        let mut hits: Vec<Hit<'_>> = Vec::new();
        let mut nodes: HashSet<NodeKey> = HashSet::new();
        let mut taken: HashSet<u32> = HashSet::new();
        for tier in [Tier::Exact, Tier::Prefix, Tier::Token] {
            let mut found: Vec<u32> = self
                .candidates(&q, tier)
                .into_iter()
                .filter(|&e| (opts.include_retired || !self.entries[e as usize].retired) && taken.insert(e))
                .collect();
            found.sort_by_key(|&e| {
                let e = &self.entries[e as usize];
                (e.field, e.node.kind, e.text.len(), e.node.idx)
            });
            for e in found {
                let entry = &self.entries[e as usize];
                if !nodes.insert(entry.node) {
                    continue;
                }
                let match_kind = if tier == Tier::Token {
                    MatchKind::Fuzzy
                } else {
                    entry.field
                };
                hits.push(Hit {
                    node: entry.node,
                    matched: &entry.text,
                    match_kind,
                    tier,
                });
                if hits.len() == opts.limit {
                    return hits;
                }
            }
        }
        hits
    }

    fn candidates(&self, q: &str, tier: Tier) -> Vec<u32> {
        let key = |e: &u32| &*self.entries[*e as usize].key;
        match tier {
            Tier::Exact => {
                let lo = self.by_key.partition_point(|e| key(e) < q);
                let hi = self.by_key.partition_point(|e| key(e) <= q);
                self.by_key[lo..hi].to_vec()
            }
            Tier::Prefix => {
                let lo = self.by_key.partition_point(|e| key(e) <= q);
                let n = self.by_key[lo..].partition_point(|e| key(e).starts_with(q));
                self.by_key[lo..lo + n].to_vec()
            }
            Tier::Token => {
                let words: Vec<&str> = q.split(' ').collect();
                let ranges: Vec<&[(Box<str>, u32)]> = words.iter().map(|w| self.token_range(w)).collect();
                let (best, _) = ranges.iter().enumerate().min_by_key(|(_, r)| r.len()).unwrap();
                let mut out: Vec<u32> = ranges[best].iter().map(|(_, e)| *e).collect();
                out.sort_unstable();
                out.dedup();
                out.retain(|&e| {
                    let entry_tokens: Vec<&str> = self.entries[e as usize].key.split(' ').collect();
                    words.iter().all(|w| entry_tokens.iter().any(|t| t.starts_with(w)))
                });
                out
            }
        }
    }

    /// Typo correction: each query word that starts no indexed token is replaced by the closest
    /// indexed token (Damerau-Levenshtein ≤ 1 for 4-7 characters, ≤ 2 from 8). `None` when nothing
    /// changed. Words shorter than 4 characters are kept.
    pub fn correct(&self, query: &str) -> Option<String> {
        let q = normalize_label(query);
        let mut changed = false;
        let mut out: Vec<String> = Vec::new();
        for w in q.split(' ').filter(|w| !w.is_empty()) {
            let n = w.chars().count();
            if n < 4 || !self.token_range(w).is_empty() {
                out.push(w.to_owned());
                continue;
            }
            let max = if n >= 8 { 2 } else { 1 };
            let mut best: Option<(usize, usize, &str)> = None;
            let chars: Vec<char> = w.chars().collect();
            for (&m, tokens) in self.corrections.range(n.saturating_sub(max)..=n + max) {
                for token in tokens {
                    let t: &str = &token.text;
                    let d = edit_distance_chars(&chars, &token.chars, max);
                    if d <= max {
                        let key = (d, m.abs_diff(n), t);
                        if best.is_none_or(|b| key < b) {
                            best = Some(key);
                        }
                    }
                }
            }
            match best {
                Some((_, _, t)) => {
                    changed = true;
                    out.push(t.to_owned());
                }
                None => out.push(w.to_owned()),
            }
        }
        changed.then(|| out.join(" "))
    }

    /// Tokens starting with `prefix`.
    fn token_range(&self, prefix: &str) -> &[(Box<str>, u32)] {
        let lo = self.tokens.partition_point(|(t, _)| &**t < prefix);
        let n = self.tokens[lo..].partition_point(|(t, _)| t.starts_with(prefix));
        &self.tokens[lo..lo + n]
    }
}

/// Optimal-string-alignment distance, giving up (returns `max + 1`) once a row exceeds `max`.
fn edit_distance(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    edit_distance_chars(&a, &b, max)
}

fn edit_distance_chars(a: &[char], b: &[char], max: usize) -> usize {
    let mut prev2: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::ActivityIdx;
    use crate::term::{Scope, Synonym, Term};

    fn index() -> SearchIndex {
        let mut seizure = Term::new("HP:0001250");
        seizure.name = "Seizure".into();
        seizure.synonyms.push(Synonym {
            text: "Fits".into(),
            scope: Scope::Exact,
            kind: Some("layperson".into()),
        });
        let hpo = Ontology::new(vec![seizure]);
        let mut d = Disease::new("MONDO:0012812", ActivityIdx(0));
        d.name = "developmental and epileptic encephalopathy 4".into();
        d.add_name("DEE4", None);
        d.add_name("STXBP1 encephalopathy with epilepsy", None);
        let mut retired = Disease::new("ORPHA:1", ActivityIdx(0));
        retired.name = "OBSOLETE: Seizure".into();
        retired.status = crate::disease::Status::Retired;
        let gene = Gene {
            symbol: "STXBP1".into(),
            hgnc: Some("HGNC:11444".into()),
            ncbi_gene: None,
            diseases: vec![0],
        };
        SearchIndex::build(&hpo, &[d, retired], &[gene])
    }

    fn first(idx: &SearchIndex, q: &str) -> (NodeKind, MatchKind) {
        let h = &idx.search(q, SearchOptions::default())[0];
        (h.node.kind, h.match_kind)
    }

    #[test]
    fn tiers_and_kinds() {
        let idx = index();
        assert_eq!(first(&idx, "mondo:0012812"), (NodeKind::Disease, MatchKind::Id));
        assert_eq!(first(&idx, "fits"), (NodeKind::Phenotype, MatchKind::Layperson));
        assert_eq!(first(&idx, "dee4"), (NodeKind::Disease, MatchKind::Abbreviation));
        assert_eq!(first(&idx, "stxbp1"), (NodeKind::Gene, MatchKind::Name));
        assert_eq!(first(&idx, "HGNC:11444"), (NodeKind::Gene, MatchKind::Id));
        assert_eq!(first(&idx, "Seiz"), (NodeKind::Phenotype, MatchKind::Name));
        assert_eq!(first(&idx, "epilep enceph"), (NodeKind::Disease, MatchKind::Fuzzy));
        // retired entries only on request
        assert_eq!(idx.search("obsolete", SearchOptions::default()).len(), 0);
        let all = SearchOptions {
            include_retired: true,
            ..SearchOptions::default()
        };
        assert_eq!(idx.search("obsolete", all).len(), 1);
    }

    #[test]
    fn corrects_typos() {
        let idx = index();
        assert_eq!(idx.correct("encephalopaty").as_deref(), Some("encephalopathy"));
        assert_eq!(
            idx.correct("stxbp1 encefalopathy").as_deref(),
            Some("stxbp1 encephalopathy")
        );
        assert_eq!(idx.correct("seizure"), None);
        assert_eq!(edit_distance("dravet", "darvet", 1), 1);
    }

    #[test]
    fn exact_linking_never_expands_prefixes_or_retired_entities() {
        let idx = index();
        assert_eq!(idx.exact("stxbp1", 8)[0].node.kind, NodeKind::Gene);
        assert!(idx.exact("seiz", 8).is_empty());
        assert!(idx.exact("obsolete seizure", 8).is_empty());
        assert!(idx.exact("STXBP1", 0).is_empty());
        assert_eq!(idx.exact("HGNC:11444", 8)[0].match_kind, MatchKind::Id);
    }
}
