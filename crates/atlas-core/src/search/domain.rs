//! Global retrieval over the aligned atlas and connected graph. No labels or model calls:
//! token specificity is learned from node document frequencies; phenotype ranking uses HPO IC.
//! Retrieval does not assert identity, diagnosis, eligibility or mechanism compatibility.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::messages::Message;
use serde::Serialize;
use serde_json::json;
mod spelling;

use super::{MatchKind, SearchOptions};
use crate::graph::{RecIdx, RecordWithhold, Relation};
use crate::node::{EdgeKind, NodeKey, NodeKind, NodeRef};
use crate::provenance::{RecordRef, SourceEntity};
use crate::text::normalize_label;
use crate::{Atlas, DiseaseIdx, Graph, TermIdx, curie};

pub const METHOD: &str = "atlas-domain-ir-v1";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Source {
    pub source_url: String,
    pub retrieved_at: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub locator: String,
}

/// Evidence references point at existing PROV chains, never at invented search records.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Evidence {
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
    pub activities: Vec<String>,
    pub sources: Vec<Source>,
}

/// Mechanism scores come from the analytics owner, which checks variant-effect conflicts.
pub struct MechanismCandidate {
    pub source: DiseaseIdx,
    pub disease: DiseaseIdx,
    pub score: f64,
    pub shared: String,
    pub sources: Vec<Source>,
}

pub struct PhenotypeCandidate {
    pub disease: DiseaseIdx,
    /// Ranking is supplied by the analytics owner, with its tie-aware mid-rank.
    pub midrank: f64,
}

#[derive(Default, Clone)]
pub struct QueryMatches {
    pub hits: Vec<Hit>,
    pub present: Vec<TermIdx>,
    pub excluded: Vec<TermIdx>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ExactId,
    ExactName,
    Synonym,
    Abbreviation,
    Layperson,
    GeneAlias,
    PreviousSymbol,
    TranslatedName,
    TranslatedAlias,
    Prefix,
    Words,
    Mention,
    Typo,
    CausalGene,
    Phenotypes,
    BroaderCondition,
    NarrowerCondition,
    Neighbour,
    Mechanism,
    Nearest,
}

impl Reason {
    fn key(self) -> &'static str {
        match self {
            Self::ExactId => "exact_id",
            Self::ExactName => "exact_name",
            Self::Synonym => "synonym",
            Self::Abbreviation => "abbreviation",
            Self::Layperson => "layperson",
            Self::GeneAlias => "gene_alias",
            Self::PreviousSymbol => "previous_symbol",
            Self::TranslatedName => "translated_name",
            Self::TranslatedAlias => "translated_alias",
            Self::Prefix => "prefix",
            Self::Words => "words",
            Self::Mention => "mention",
            Self::Typo => "typo",
            Self::CausalGene => "causal_gene",
            Self::Phenotypes => "phenotypes",
            Self::BroaderCondition => "broader_condition",
            Self::NarrowerCondition => "narrower_condition",
            Self::Neighbour => "neighbour",
            Self::Mechanism => "mechanism",
            Self::Nearest => "nearest",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub node: NodeRef,
    pub matched: String,
    pub match_kind: &'static str,
    pub reason: Reason,
    pub why: String,
    pub why_messages: Vec<Message>,
    pub evidence: Evidence,
    /// Only whole-input authoritative lexical matches may be used for automatic resolution.
    #[serde(skip)]
    pub strong: bool,
    #[serde(skip)]
    pub key: NodeKey,
    #[serde(skip)]
    rank: f64,
}

#[derive(Debug)]
struct Entry {
    node: NodeKey,
    slot: usize,
    text: String,
    key: String,
    word_count: usize,
    field: Reason,
    records: Vec<RecIdx>,
    /// Wikidata labels with several possible targets are choices too.
    strong: bool,
}

#[derive(Debug, Default)]
pub struct Index {
    entries: Vec<Entry>,
    node_count: usize,
    exact: BTreeMap<String, Vec<u32>>,
    tokens: BTreeMap<String, Vec<u32>>,
    idf: HashMap<String, f64>,
    /// Positive HPO closures, keyed by disease. NOT annotations never enter these profiles.
    profiles: Vec<Vec<TermIdx>>,
    children: HashMap<DiseaseIdx, Vec<DiseaseIdx>>,
    spelling: spelling::Spelling,
}

impl Hit {
    fn explain(&mut self, key: &str, params: serde_json::Value) {
        self.why_messages = vec![Message::new(key, params)];
        self.localise("en");
    }
    fn append(&mut self, key: &str, params: serde_json::Value) {
        self.why_messages.push(Message::new(key, params));
        self.localise("en");
    }
    pub fn localise(&mut self, lang: &str) {
        self.why = self
            .why_messages
            .iter()
            .map(|m| m.text(lang))
            .collect::<Vec<_>>()
            .join("; ");
    }
}

fn atlas_kind(k: NodeKind) -> bool {
    matches!(k, NodeKind::Disease | NodeKind::Gene | NodeKind::Phenotype)
}

pub fn node_ref(atlas: &Atlas, graph: &Graph, key: NodeKey) -> NodeRef {
    if atlas_kind(key.kind) {
        atlas.node_ref(key)
    } else {
        graph.node_ref(key)
    }
}

pub fn node_key(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeKey> {
    let id = graph.canonical_id(id);
    if let Some(idx) = atlas.disease_idx(id) {
        Some(NodeKey {
            kind: NodeKind::Disease,
            idx,
        })
    } else if let Some(idx) = atlas.gene(id) {
        Some(NodeKey {
            kind: NodeKind::Gene,
            idx,
        })
    } else if let Some(idx) = atlas.hpo.canonical(id) {
        Some(NodeKey {
            kind: NodeKind::Phenotype,
            idx,
        })
    } else {
        graph.node(id)
    }
}

fn from_entity(e: &SourceEntity, locator: String) -> Source {
    Source {
        source_url: e.url.clone(),
        retrieved_at: e.retrieved_at.clone(),
        version: e.version.clone(),
        sha256: e.sha256.clone(),
        locator,
    }
}

fn record_sources(graph: &Graph, records: &[RecIdx]) -> Vec<Source> {
    records
        .iter()
        .filter_map(|&r| {
            let record = graph.data().records.get(r as usize)?;
            let entity = graph.provenance().entities.get(record.entity.0 as usize)?;
            let mut source = from_entity(entity, format!("{}#{}", entity.file, record.locator));
            source.source_url = record.url.clone().unwrap_or(source.source_url);
            source.retrieved_at = record.fetched_at.clone().or(source.retrieved_at);
            source.sha256 = Some(crate::graph::hex(&record.sha256));
            Some(source)
        })
        .collect()
}

fn core_sources(atlas: &Atlas, refs: &[RecordRef]) -> Vec<Source> {
    refs.iter()
        .filter_map(|r| {
            atlas
                .provenance
                .entities
                .get(r.entity.0 as usize)
                .map(|e| from_entity(e, format!("{}#{}", e.file, r.locator)))
        })
        .collect()
}

fn visible(atlas: &Atlas, graph: &Graph, key: NodeKey, opts: SearchOptions) -> bool {
    if key.kind == NodeKind::Disease {
        opts.include_retired || atlas.disease_at(key.idx).is_active()
    } else if atlas_kind(key.kind) {
        true
    } else {
        graph.node_withheld(key).is_none()
    }
}

fn closure(atlas: &Atlas, terms: impl IntoIterator<Item = TermIdx>) -> Vec<TermIdx> {
    terms
        .into_iter()
        .flat_map(|t| atlas.hpo.ancestors(t).iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl Index {
    /// Lightweight entity choices for type-ahead and spelling. No graph traversal,
    /// phenotype scoring or model call. A fuzzy/phonetic suggestion is never certain.
    pub fn suggest(&self, atlas: &Atlas, graph: &Graph, query: &str, opts: SearchOptions) -> Vec<Hit> {
        let q = normalize_label(&curie::normalize_query(query).unwrap_or_else(|| query.into()));
        if q.is_empty() || opts.limit == 0 {
            return vec![];
        }
        let allowed = |i: u32| {
            let e = &self.entries[i as usize];
            visible(atlas, graph, e.node, opts) && graph.records_withheld(&e.records).is_none()
        };
        let mut hits = HashMap::new();
        if let Some(ids) = self.exact.get(&q) {
            for &i in ids.iter().filter(|&&i| allowed(i)) {
                let e = &self.entries[i as usize];
                insert(
                    &mut hits,
                    self.entry_hit(atlas, graph, i, e.field, 1000.0 - field_order(e.field) as f64),
                );
            }
        }
        for &i in self.spelling.genes(&q).iter().filter(|&&i| allowed(i)) {
            let e = &self.entries[i as usize];
            insert(
                &mut hits,
                self.entry_hit(atlas, graph, i, e.field, 1000.0 - field_order(e.field) as f64),
            );
        }
        let exact = !hits.is_empty();
        let letters = query.chars().filter(char::is_ascii_alphabetic).count();
        let symbol_shape = (3..=12).contains(&q.chars().count())
            && query
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-'))
            && (query.chars().any(|c| c.is_ascii_digit())
                || (letters >= 2 && query.chars().filter(char::is_ascii_uppercase).count() * 2 >= letters));
        if !exact {
            // A bounded sorted-index prefix lookup works from the first character.
            let mut visited = 0;
            for (key, ids) in self.exact.range(q.clone()..) {
                if !key.starts_with(&q) || visited >= 512 {
                    break;
                }
                for &i in ids.iter().filter(|&&i| allowed(i)).take(512 - visited) {
                    visited += 1;
                    insert(
                        &mut hits,
                        self.entry_hit(
                            atlas,
                            graph,
                            i,
                            Reason::Prefix,
                            700.0 - (key.chars().count().saturating_sub(q.chars().count())) as f64 * 0.2,
                        ),
                    );
                }
            }
        }
        // Exact identifiers stay fast; short symbols also offer alternative spellings
        // when an alias collision would otherwise hide them. Early prefixes stay cheap.
        if ((!exact && hits.len() < opts.limit.min(8))
            || (symbol_shape && q.chars().count() >= 4 && !exact)
            || (exact
                && symbol_shape
                && hits.len() < opts.limit.min(8)
                && !hits.values().any(|h| h.reason == Reason::ExactId)))
            && (q.chars().count() >= 2 || hits.is_empty())
        {
            for (i, d, phonetic) in self
                .spelling
                .nearest(&self.entries, &q, opts.limit.min(8), symbol_shape, allowed)
            {
                let max = if q.chars().count() >= 8 { 2 } else { 1 };
                let reason = if q.chars().count() <= 128 && (d <= max || phonetic) {
                    Reason::Typo
                } else {
                    Reason::Nearest
                };
                let mut h = self.entry_hit(
                    atlas,
                    graph,
                    i,
                    reason,
                    if reason == Reason::Typo {
                        let e = &self.entries[i as usize];
                        let symbol = symbol_shape && e.node.kind == NodeKind::Gene;
                        (if symbol && q.chars().count() >= 4 { 720.0 } else { 650.0 }) - d as f64
                            + if symbol { 0.3 } else { 0.0 }
                            - field_order(e.field) as f64 * 0.01
                    } else {
                        100.0 - d as f64
                    },
                );
                if phonetic {
                    h.match_kind = "phonetic";
                }
                insert(&mut hits, h);
            }
        }
        let mut hits = sorted(hits.into_values().collect());
        hits.truncate(opts.limit.min(100));
        for h in &mut hits {
            self.hydrate(atlas, graph, h);
        }
        hits
    }
    pub fn build(atlas: &Atlas, graph: &Graph) -> Self {
        let mut index = Self::default();
        // Reuse the canonical lexical owner's indexing rules (abbreviations, ids and lay terms).
        for e in &atlas.search().entries {
            let field = match e.field {
                MatchKind::Id => Reason::ExactId,
                MatchKind::Name => Reason::ExactName,
                MatchKind::Abbreviation => Reason::Abbreviation,
                MatchKind::Synonym | MatchKind::Fuzzy => Reason::Synonym,
                MatchKind::Layperson => Reason::Layperson,
            };
            index.add(e.node, &e.text, field, &[], true);
        }
        for g in &graph.data().gene_aliases {
            let Some(idx) = atlas.gene(&g.symbol) else { continue };
            let key = NodeKey {
                kind: NodeKind::Gene,
                idx,
            };
            index.add(key, &g.name, Reason::ExactName, &[g.record], true);
            for name in &g.previous {
                index.add(key, name, Reason::PreviousSymbol, &[g.record], true);
            }
            for name in &g.aliases {
                index.add(key, name, Reason::GeneAlias, &[g.record], true);
            }
        }
        for name in &graph.data().wiki_names {
            let Some(item) = graph.data().wiki_items.get(name.item as usize) else {
                continue;
            };
            for target in &item.targets {
                if let Some(key) = node_key(atlas, graph, target) {
                    index.add(
                        key,
                        &name.text,
                        if name.alias {
                            Reason::TranslatedAlias
                        } else {
                            Reason::TranslatedName
                        },
                        &[item.record],
                        !name.alias && item.targets.len() == 1,
                    );
                }
            }
        }
        for kind in [
            NodeKind::Study,
            NodeKind::Grant,
            NodeKind::Paper,
            NodeKind::Person,
            NodeKind::Organisation,
            NodeKind::Asset,
        ] {
            for idx in 0..graph.node_count(kind) as u32 {
                let key = NodeKey { kind, idx };
                let node = graph.node_ref(key);
                index.add(key, &node.id, Reason::ExactId, graph.node_records(key), true);
                index.add(key, &node.label, Reason::ExactName, graph.node_records(key), true);
                if kind == NodeKind::Person {
                    for name in &graph.person(idx).name_variants {
                        index.add(key, name, Reason::Synonym, graph.node_records(key), false);
                    }
                }
            }
        }
        for (alias, target) in &graph.data().aliases {
            if let Some(key) = node_key(atlas, graph, target) {
                // The SSSOM activity owns mapping lineage; original alias ids stay inspectable.
                index.add(key, alias, Reason::ExactId, &[], true);
            }
        }
        // Restore HPO replacement ids omitted by the legacy live-only lexical index.
        for t in atlas.hpo.terms().iter().filter(|t| t.obsolete) {
            if let Some(idx) = atlas.hpo.canonical(&t.id) {
                index.add(
                    NodeKey {
                        kind: NodeKind::Phenotype,
                        idx,
                    },
                    &t.id,
                    Reason::ExactId,
                    &[],
                    true,
                );
            }
        }
        index.finish(atlas);
        index
    }

    fn add(&mut self, node: NodeKey, text: &str, field: Reason, records: &[RecIdx], strong: bool) {
        let key = normalize_label(text);
        if key.is_empty() {
            return;
        }
        self.entries.push(Entry {
            node,
            slot: 0,
            text: text.into(),
            word_count: key.split_whitespace().count(),
            key,
            field,
            records: records.to_vec(),
            strong,
        });
    }

    fn finish(&mut self, atlas: &Atlas) {
        let mut documents = HashMap::new();
        let mut token_nodes: HashMap<String, HashSet<NodeKey>> = HashMap::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            self.exact.entry(e.key.clone()).or_default().push(i as u32);
            let next_slot = documents.len();
            e.slot = *documents.entry(e.node).or_insert(next_slot);
            for token in e.key.split_whitespace().collect::<HashSet<_>>() {
                self.tokens.entry(token.into()).or_default().push(i as u32);
                token_nodes.entry(token.into()).or_default().insert(e.node);
            }
        }
        self.node_count = documents.len();
        let n = self.node_count as f64;
        self.idf = token_nodes
            .into_iter()
            .map(|(token, nodes)| {
                (
                    token,
                    (1.0 + (n - nodes.len() as f64 + 0.5) / (nodes.len() as f64 + 0.5)).ln(),
                )
            })
            .collect();
        self.spelling = spelling::Spelling::build(&self.entries);
        self.profiles = atlas
            .diseases()
            .iter()
            .map(|d| closure(atlas, d.phenotypes.iter().map(|p| p.term)))
            .collect();
        for (idx, d) in atlas.active() {
            for parent in &d.parents {
                if let Some(p) = atlas.disease_idx(parent) {
                    self.children.entry(p).or_default().push(idx);
                }
            }
        }
    }

    fn entry_hit(&self, atlas: &Atlas, graph: &Graph, idx: u32, reason: Reason, rank: f64) -> Hit {
        let e = &self.entries[idx as usize];
        let kind = match e.field {
            Reason::ExactId => "id",
            Reason::Abbreviation => "abbreviation",
            Reason::Layperson => "layperson",
            Reason::GeneAlias | Reason::PreviousSymbol => "alias",
            Reason::TranslatedName => "wikidata_label",
            Reason::TranslatedAlias => "wikidata_alias",
            Reason::Synonym => "synonym",
            _ => "name",
        };
        let ortholog =
            e.node.kind == NodeKind::Asset && graph.asset(e.node.idx).kind == crate::graph::AssetKind::OrthologGene;
        let mut hit = self.hit(
            atlas,
            graph,
            e.node,
            &e.text,
            reason,
            if ortholog && reason != Reason::ExactId {
                rank - 400.0
            } else {
                rank
            },
        );
        if ortholog {
            hit.explain("ortholog", json!({}));
        }
        hit.match_kind = if matches!(reason, Reason::Typo | Reason::Nearest) {
            "typo"
        } else if reason == Reason::Words {
            "fuzzy"
        } else {
            kind
        };
        hit.strong = e.strong && reason == e.field;
        hit.evidence.sources.extend(record_sources(graph, &e.records));
        if e.field == Reason::ExactId && graph.data().aliases.iter().any(|(a, _)| a == &e.text) {
            hit.evidence.nodes.push(e.text.clone());
            hit.evidence
                .activities
                .push(crate::graph::activity::IDENTITY_SSSOM.into());
        }
        hit
    }

    fn hit(&self, atlas: &Atlas, graph: &Graph, key: NodeKey, matched: &str, reason: Reason, rank: f64) -> Hit {
        let node = node_ref(atlas, graph, key);
        let sources = vec![];
        let message = Message::new(reason.key(), json!({"matched":matched}));
        Hit {
            evidence: Evidence {
                nodes: vec![node.id.clone()],
                edges: vec![],
                activities: vec![],
                sources,
            },
            node,
            key,
            matched: matched.into(),
            match_kind: "fuzzy",
            reason,
            why: message.text("en"),
            why_messages: vec![message],
            strong: false,
            rank,
        }
    }

    /// Ranked choices across kinds. Every expansion is below a direct lexical match.
    pub fn search(&self, atlas: &Atlas, graph: &Graph, query: &str, opts: SearchOptions) -> Vec<Hit> {
        self.search_context(atlas, graph, query, opts).hits
    }

    pub fn search_context(&self, atlas: &Atlas, graph: &Graph, query: &str, opts: SearchOptions) -> QueryMatches {
        self.retrieve(atlas, graph, query, opts, true)
    }

    /// The server supplies the analytics-owned phenotype scorer after lexical retrieval.
    pub fn lexical_context(&self, atlas: &Atlas, graph: &Graph, query: &str, opts: SearchOptions) -> QueryMatches {
        self.retrieve(atlas, graph, query, opts, false)
    }

    fn retrieve(&self, atlas: &Atlas, graph: &Graph, query: &str, opts: SearchOptions, fallback: bool) -> QueryMatches {
        if opts.limit == 0 || query.trim().is_empty() {
            return QueryMatches::default();
        }
        let q = normalize_label(&curie::normalize_query(query).unwrap_or_else(|| query.into()));
        let words: Vec<&str> = q.split_whitespace().collect();
        if words.is_empty() {
            return QueryMatches::default();
        }
        let mut visibility = vec![0_u8; self.entries.len()];
        let mut allowed = |i: u32| {
            if visibility[i as usize] != 0 {
                return visibility[i as usize] == 1;
            }
            let e = &self.entries[i as usize];
            let allowed = visible(atlas, graph, e.node, opts) && graph.records_withheld(&e.records).is_none();
            visibility[i as usize] = if allowed { 1 } else { 2 };
            allowed
        };
        let mut hits = HashMap::<NodeKey, Hit>::new();
        let mut present = BTreeSet::new();
        let mut excluded = BTreeSet::new();
        if let Some(entries) = self.exact.get(&q) {
            for &i in entries.iter().filter(|&&i| allowed(i)) {
                let e = &self.entries[i as usize];
                insert(
                    &mut hits,
                    self.entry_hit(atlas, graph, i, e.field, 1000.0 - field_order(e.field) as f64),
                );
                if e.node.kind == NodeKind::Phenotype {
                    present.insert(e.node.idx);
                }
            }
        }
        // Boundary-aware phrase mentions; all equally named targets remain candidates. Unambiguous
        // phenotype spans alone contribute to the profile. Local explicit negation enters NOT.
        for start in 0..words.len() {
            let Some(candidates) = self.tokens.get(words[start]) else {
                continue;
            };
            for &i in candidates {
                let e = &self.entries[i as usize];
                if e.word_count == words.len()
                    || start + e.word_count > words.len()
                    || !e
                        .key
                        .split_whitespace()
                        .eq(words[start..start + e.word_count].iter().copied())
                {
                    continue;
                }
                if e.word_count == 1 && e.key.chars().count() < 3 {
                    continue;
                }
                // Check visibility only after the phrase matches, without allocating token vectors.
                if !allowed(i) {
                    continue;
                }
                let context = &words[start.saturating_sub(3)..start];
                let boundary = context
                    .iter()
                    .rposition(|w| ["and", "but", "with", "und", "aber", "mit"].contains(w))
                    .map_or(0, |i| i + 1);
                let negated = context[boundary..]
                    .iter()
                    .any(|w| ["no", "not", "without", "kein", "keine", "keinen", "ohne", "sans"].contains(w));
                if e.node.kind == NodeKind::Phenotype && negated {
                    excluded.insert(e.node.idx);
                    continue;
                }
                let mut h = self.entry_hit(atlas, graph, i, Reason::Mention, 800.0 - field_order(e.field) as f64);
                h.explain("mention", json!({"matched":e.text}));
                insert(&mut hits, h);
                if e.node.kind == NodeKind::Phenotype {
                    let targets: HashSet<_> = self.exact[&e.key]
                        .iter()
                        .filter(|&&j| allowed(j))
                        .map(|&j| self.entries[j as usize].node)
                        .filter(|n| n.kind == NodeKind::Phenotype)
                        .collect();
                    if targets.len() == 1 {
                        present.insert(e.node.idx);
                    }
                }
            }
        }
        for t in &excluded {
            present.remove(t);
        }
        // One focal seizure is one symptom, even when the phrase also contains "seizure".
        let specific = present.clone();
        present.retain(|&t| {
            !specific
                .iter()
                .any(|&s| s != t && atlas.hpo.ancestors(s).binary_search(&t).is_ok())
        });
        for (key, entries) in self.exact.range(q.clone()..) {
            if !key.starts_with(&q) {
                break;
            }
            if key == &q {
                continue;
            }
            for &i in entries.iter().filter(|&&i| allowed(i)) {
                insert(&mut hits, self.entry_hit(atlas, graph, i, Reason::Prefix, 700.0));
            }
        }
        // Corpus-only IDF; duplicate aliases do not increase a token's document frequency.
        let useful: BTreeSet<&str> = words.iter().copied().filter(|w| !stopword(w)).collect();
        let total: f64 = useful.iter().map(|w| self.idf.get(*w).copied().unwrap_or(1.0)).sum();
        // Dense entry ids let postings accumulate without per-token hash sets or rehashing.
        let mut weights = vec![0.0; self.entries.len()];
        let mut seen = vec![usize::MAX; self.entries.len()];
        let mut weighted = Vec::new();
        let identifier = self.exact.get(&q).is_some_and(|entries| {
            entries
                .iter()
                .any(|&i| self.entries[i as usize].field == Reason::ExactId)
        });
        for (word_no, word) in useful.into_iter().filter(|_| !identifier).enumerate() {
            let weight = self.idf.get(word).copied().unwrap_or(1.0);
            for (token, entries) in self.tokens.range(word.to_owned()..) {
                if !token.starts_with(word) {
                    break;
                }
                if token != word && word.chars().count() < 4 {
                    continue;
                }
                for &i in entries.iter().filter(|&&i| allowed(i)) {
                    if seen[i as usize] != word_no {
                        seen[i as usize] = word_no;
                        if weights[i as usize] == 0.0 {
                            weighted.push(i);
                        }
                        weights[i as usize] += weight;
                    }
                }
            }
        }
        let mut word_candidates = vec![None; self.node_count];
        let mut candidate_slots = Vec::new();
        for i in weighted {
            let weight = weights[i as usize];
            let e = &self.entries[i as usize];
            // A shared registry prefix or identifier number is not a semantic match.
            if e.field == Reason::ExactId {
                continue;
            }
            if e.node.kind == NodeKind::Phenotype && excluded.contains(&e.node.idx) {
                continue;
            }
            let mut rank = 500.0 + 100.0 * weight / total.max(1.0);
            if e.node.kind == NodeKind::Asset && graph.asset(e.node.idx).kind == crate::graph::AssetKind::OrthologGene {
                rank -= 400.0;
            }
            if hits
                .get(&e.node)
                .is_some_and(|h| h.rank > rank || (h.rank == rank && h.matched <= e.text))
            {
                continue;
            }
            if word_candidates[e.slot].is_none() {
                candidate_slots.push(e.slot);
                word_candidates[e.slot] = Some((i, rank));
            }
            let best = word_candidates[e.slot].as_mut().unwrap();
            if rank > best.1 || (rank == best.1 && e.text < self.entries[best.0 as usize].text) {
                *best = (i, rank);
            }
        }
        let mut candidates: Vec<_> = candidate_slots
            .into_iter()
            .map(|slot| word_candidates[slot].unwrap())
            .collect();
        if !fallback && candidates.len() > opts.limit {
            // Word-only hits cannot seed graph expansion (rank <= 600). Selecting their exact
            // top K first preserves the final ranking without materialising discarded evidence.
            candidates.select_nth_unstable_by(opts.limit, |&(a, ar), &(b, br)| {
                let a = &self.entries[a as usize];
                let b = &self.entries[b as usize];
                br.total_cmp(&ar)
                    .then(kind_priority(a.node.kind).cmp(&kind_priority(b.node.kind)))
                    .then(node_id(atlas, graph, a.node).cmp(node_id(atlas, graph, b.node)))
                    .then(a.text.cmp(&b.text))
            });
            candidates.truncate(opts.limit);
        }
        for (i, mut rank) in candidates {
            let e = &self.entries[i as usize];
            // entry_hit applies the ortholog penalty itself.
            if e.node.kind == NodeKind::Asset && graph.asset(e.node.idx).kind == crate::graph::AssetKind::OrthologGene {
                rank += 400.0;
            }
            insert(&mut hits, self.entry_hit(atlas, graph, i, Reason::Words, rank));
        }
        // Suggestions are separate choices even when partial word retrieval succeeded.
        if !hits.values().any(|h| h.strong) && words.len() <= 8 {
            for mut hit in self.suggest(atlas, graph, query, SearchOptions { limit: 8, ..opts }) {
                if hit.reason == Reason::Prefix {
                    continue;
                }
                hit.strong = false;
                insert(&mut hits, hit);
            }
        }
        // Space and hyphen variants of an existing gene symbol/alias have the same
        // source-backed identity; every colliding entry remains a choice.
        for &i in self.spelling.genes(&q).iter().filter(|&&i| allowed(i)) {
            let e = &self.entries[i as usize];
            insert(
                &mut hits,
                self.entry_hit(atlas, graph, i, e.field, 1000.0 - field_order(e.field) as f64),
            );
        }
        // Stable lexical seeds; do not let arbitrary hash iteration choose expansion paths.
        let seeds = top_hits(hits.values().filter(|h| h.rank >= 700.0), 32);
        for seed in &seeds {
            if seed.key.kind == NodeKind::Gene {
                let gene = atlas.gene_at(seed.key.idx);
                for &d in &gene.diseases {
                    let links: Vec<_> = atlas
                        .disease_at(d)
                        .genes
                        .iter()
                        .filter(|g| g.symbol == gene.symbol && g.is_causal())
                        .collect();
                    if links.is_empty() {
                        continue;
                    }
                    let mut hit = self.hit(
                        atlas,
                        graph,
                        NodeKey {
                            kind: NodeKind::Disease,
                            idx: d,
                        },
                        &gene.symbol,
                        Reason::CausalGene,
                        650.0,
                    );
                    hit.explain("causal_gene", json!({"matched":gene.symbol}));
                    hit.evidence.nodes.push(seed.node.id.clone());
                    hit.evidence.sources.extend(core_sources(
                        atlas,
                        &links.iter().map(|g| g.record.clone()).collect::<Vec<_>>(),
                    ));
                    insert(&mut hits, hit);
                }
            }
            if seed.key.kind == NodeKind::Disease {
                for (reason, neighbours) in [
                    (
                        Reason::BroaderCondition,
                        atlas
                            .disease_at(seed.key.idx)
                            .parents
                            .iter()
                            .filter_map(|p| atlas.disease_idx(p))
                            .collect::<Vec<_>>(),
                    ),
                    (
                        Reason::NarrowerCondition,
                        self.children.get(&seed.key.idx).cloned().unwrap_or_default(),
                    ),
                ] {
                    for d in neighbours {
                        let mut hit = self.hit(
                            atlas,
                            graph,
                            NodeKey {
                                kind: NodeKind::Disease,
                                idx: d,
                            },
                            &seed.node.label,
                            reason,
                            280.0,
                        );
                        hit.explain(reason.key(), json!({"matched":seed.node.label}));
                        hit.evidence.nodes.push(seed.node.id.clone());
                        insert(&mut hits, hit);
                    }
                }
            }
        }
        let present = present.into_iter().collect::<Vec<_>>();
        let excluded = excluded.into_iter().collect::<Vec<_>>();
        if fallback {
            self.rank_phenotypes(atlas, graph, &present, &excluded, &mut hits);
        }
        // Neighbours of direct matches and causal-gene candidates, using only inspectable edges.
        let seeds = top_hits(hits.values().filter(|h| h.rank >= 650.0), 32);
        for seed in &seeds {
            for edge in graph.incident(&seed.node.id) {
                if matches!(
                    edge.edge.relation,
                    Relation::CandidateSameAs | Relation::SameAs | Relation::RelatedTo
                ) || edge.edge.kind == EdgeKind::Hypothesis
                    || graph.records_withheld(&edge.edge.records).is_some()
                {
                    continue;
                }
                let Some(key) = node_key(atlas, graph, edge.other) else {
                    continue;
                };
                if !visible(atlas, graph, key, opts) {
                    continue;
                }
                let edge_rank = match edge.edge.level {
                    crate::graph::LinkLevel::Exact
                    | crate::graph::LinkLevel::Curated
                    | crate::graph::LinkLevel::Mesh => 210.0,
                    crate::graph::LinkLevel::Gene => 200.0,
                    _ => 170.0,
                };
                let mut hit = self.hit(atlas, graph, key, &seed.node.label, Reason::Neighbour, edge_rank);
                hit.explain(relation_key(edge.edge.relation), json!({"matched":seed.node.label}));
                hit.evidence.nodes.push(seed.node.id.clone());
                hit.evidence.edges.push(edge.edge.id());
                hit.evidence.sources.extend(record_sources(graph, &edge.edge.records));
                insert(&mut hits, hit);
            }
        }
        // A paper/grant connects a gene to its researchers; an asset connects to its holder.
        let bridges = top_hits(
            hits.values().filter(|h| {
                h.reason == Reason::Neighbour
                    && matches!(h.key.kind, NodeKind::Paper | NodeKind::Grant | NodeKind::Asset)
            }),
            512,
        );
        for bridge in &bridges {
            for edge in graph.incident(&bridge.node.id) {
                if !matches!(
                    edge.edge.relation,
                    Relation::AuthorOf | Relation::PrincipalInvestigatorOf | Relation::HeldBy
                ) || edge.edge.kind == EdgeKind::Hypothesis
                    || graph.records_withheld(&edge.edge.records).is_some()
                {
                    continue;
                }
                let Some(key) = node_key(atlas, graph, edge.other) else {
                    continue;
                };
                if !matches!(key.kind, NodeKind::Person | NodeKind::Organisation) || !visible(atlas, graph, key, opts) {
                    continue;
                }
                let mut hit = self.hit(atlas, graph, key, &bridge.matched, Reason::Neighbour, 160.0);
                hit.explain(
                    if key.kind == NodeKind::Person {
                        "researcher"
                    } else {
                        "holder"
                    },
                    json!({"matched":bridge.matched}),
                );
                hit.evidence.nodes.extend(bridge.evidence.nodes.clone());
                hit.evidence.edges.extend(bridge.evidence.edges.clone());
                hit.evidence.edges.push(edge.edge.id());
                hit.evidence.sources.extend(bridge.evidence.sources.clone());
                hit.evidence.sources.extend(record_sources(graph, &edge.edge.records));
                insert(&mut hits, hit);
            }
        }
        let mut out = sorted(
            hits.into_values()
                .filter(|h| visible(atlas, graph, h.key, opts))
                .collect(),
        );
        out.truncate(opts.limit);
        for hit in &mut out {
            self.hydrate(atlas, graph, hit);
            let mut seen = HashSet::new();
            hit.evidence
                .sources
                .retain(|s| seen.insert((s.source_url.clone(), s.locator.clone(), s.sha256.clone())));
        }
        QueryMatches {
            hits: out,
            present,
            excluded,
        }
    }

    fn hydrate(&self, atlas: &Atlas, graph: &Graph, hit: &mut Hit) {
        let key = hit.key;
        let sources = if key.kind == NodeKind::Disease {
            core_sources(atlas, &atlas.disease_at(key.idx).derived_from)
        } else if key.kind == NodeKind::Gene {
            let gene = atlas.gene_at(key.idx);
            core_sources(
                atlas,
                &gene
                    .diseases
                    .iter()
                    .flat_map(|&d| atlas.disease_at(d).genes.iter())
                    .filter(|g| g.symbol == gene.symbol)
                    .map(|g| g.record.clone())
                    .collect::<Vec<_>>(),
            )
        } else if key.kind == NodeKind::Phenotype {
            atlas
                .provenance
                .entity_by_file("hp.obo")
                .map(|e| {
                    vec![from_entity(
                        atlas.provenance.entity(e),
                        format!("hp.obo#{}", hit.node.id),
                    )]
                })
                .unwrap_or_default()
        } else {
            record_sources(graph, graph.node_records(key))
        };
        hit.evidence.sources.extend(sources);
        if key.kind == NodeKind::Disease && hit.reason == Reason::Phenotypes {
            let disease = atlas.disease_at(key.idx);
            hit.evidence.sources.extend(core_sources(
                atlas,
                &disease
                    .phenotypes
                    .iter()
                    .chain(&disease.excluded)
                    .flat_map(|p| p.annotations.iter().map(|a| a.record.clone()))
                    .collect::<Vec<_>>(),
            ));
        }
    }

    /// Add analytics-owned mechanism leads below direct identity, phenotype and hierarchy hits.
    pub fn with_mechanisms(
        &self,
        atlas: &Atlas,
        graph: &Graph,
        hits: Vec<Hit>,
        candidates: Vec<MechanismCandidate>,
        opts: SearchOptions,
    ) -> Vec<Hit> {
        let mut all: HashMap<_, _> = hits.into_iter().map(|h| (h.key, h)).collect();
        for candidate in candidates {
            let key = NodeKey {
                kind: NodeKind::Disease,
                idx: candidate.disease,
            };
            if !visible(atlas, graph, key, opts)
                || candidate.score <= 0.0
                || !candidate.score.is_finite()
                || candidate.shared.is_empty()
            {
                continue;
            }
            let source = atlas.disease_ref(candidate.source);
            let mut hit = self.hit(
                atlas,
                graph,
                key,
                &candidate.shared,
                Reason::Mechanism,
                180.0 + 10.0 * candidate.score.clamp(0.0, 1.0),
            );
            hit.explain("mechanism", json!({"matched":source.label}));
            hit.evidence.nodes.push(source.id);
            hit.evidence.sources.extend(candidate.sources);
            self.hydrate(atlas, graph, &mut hit);
            insert(&mut all, hit);
        }
        let mut out = sorted(all.into_values().collect());
        out.truncate(opts.limit);
        out
    }

    /// Global ranking consumes phenotype scores from analytics without depending on that crate.
    /// The simGIC fallback remains available for standalone core users and evaluation ablations.
    pub fn with_phenotypes(
        &self,
        atlas: &Atlas,
        graph: &Graph,
        query: QueryMatches,
        candidates: Vec<PhenotypeCandidate>,
        opts: SearchOptions,
    ) -> Vec<Hit> {
        if query.present.is_empty() {
            return query.hits.into_iter().take(opts.limit).collect();
        }
        let mut hits: HashMap<_, _> = query
            .hits
            .into_iter()
            .filter(|h| h.reason != Reason::Phenotypes)
            .map(|h| (h.key, h))
            .collect();
        for hit in hits.values_mut() {
            if hit.key.kind == NodeKind::Disease && hit.reason == Reason::Words {
                hit.rank = 250.0;
            }
        }
        let matched_text = query
            .present
            .iter()
            .map(|&t| atlas.hpo.term(t).name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        for candidate in candidates {
            if !candidate.midrank.is_finite() || candidate.midrank < 1.0 {
                continue;
            }
            let disease = atlas.disease_at(candidate.disease);
            let key = NodeKey {
                kind: NodeKind::Disease,
                idx: candidate.disease,
            };
            let profile = &self.profiles[candidate.disease as usize];
            if !visible(atlas, graph, key, opts)
                || !query.present.iter().any(|&t| {
                    atlas
                        .hpo
                        .ancestors(t)
                        .iter()
                        .any(|a| atlas.hpo.ic(*a) > 0.0 && profile.binary_search(a).is_ok())
                })
            {
                continue;
            }
            let shared = query
                .present
                .iter()
                .filter(|&&t| profile.binary_search(&t).is_ok())
                .count();
            let conflicts = query
                .excluded
                .iter()
                .filter(|&&t| profile.binary_search(&t).is_ok())
                .count()
                + query
                    .present
                    .iter()
                    .filter(|&&t| {
                        disease
                            .excluded
                            .iter()
                            .any(|p| atlas.hpo.ancestors(t).binary_search(&p.term).is_ok())
                    })
                    .count();
            let mut hit = self.hit(
                atlas,
                graph,
                key,
                &matched_text,
                Reason::Phenotypes,
                350.0 + 100.0 / candidate.midrank,
            );
            if shared == 0 {
                hit.explain("phenotypes", json!({}));
            } else {
                hit.explain("phenotype_overlap", json!({"n":shared,"total":query.present.len()}));
            }
            if conflicts > 0 {
                hit.append("phenotype_conflict", json!({"n":conflicts}));
            }
            hit.evidence.nodes.extend(
                query
                    .present
                    .iter()
                    .chain(&query.excluded)
                    .map(|&t| atlas.hpo.term(t).id.clone()),
            );
            if let Some(old) = hits.get_mut(&key) {
                if matches!(old.reason, Reason::Words | Reason::Nearest) {
                    *old = hit;
                    continue;
                }
                if old.reason == Reason::CausalGene {
                    old.rank = 650.0 + 10.0 / candidate.midrank;
                }
                // The core fallback already attached the same source-derived symptom explanation.
                if !old.why_messages.iter().any(|m| m.key.contains("phenotype")) {
                    old.why_messages.extend(hit.why_messages.clone());
                    old.localise("en");
                }
            } else {
                insert(&mut hits, hit);
            }
        }
        let mut out = sorted(hits.into_values().collect());
        out.truncate(opts.limit);
        for hit in &mut out {
            if hit.reason == Reason::Phenotypes {
                self.hydrate(atlas, graph, hit);
            }
        }
        out
    }

    /// Typed HPO retrieval, sharing the exact scorer used for text-derived symptoms.
    pub fn phenotypes(
        &self,
        atlas: &Atlas,
        graph: &Graph,
        present: &[TermIdx],
        excluded: &[TermIdx],
        limit: usize,
    ) -> Vec<Hit> {
        let mut hits = HashMap::new();
        self.rank_phenotypes(atlas, graph, present, excluded, &mut hits);
        let mut out = sorted(hits.into_values().collect());
        out.truncate(limit);
        for hit in &mut out {
            self.hydrate(atlas, graph, hit);
        }
        out
    }

    /// Mid-rank tie policy for evaluation; missing/no-shared-information targets return None.
    pub fn phenotype_rank(
        &self,
        atlas: &Atlas,
        present: &[TermIdx],
        excluded: &[TermIdx],
        target: DiseaseIdx,
    ) -> Option<(f64, usize, usize)> {
        let scores = self.phenotype_scores(atlas, present, excluded);
        let truth = scores.iter().find(|s| s.0 == target)?.1;
        let above = scores.iter().filter(|s| s.1 > truth + 1e-9).count();
        let tied = scores.iter().filter(|s| (s.1 - truth).abs() <= 1e-9).count();
        Some(((above + 1 + above + tied) as f64 / 2.0, above + 1, above + tied))
    }

    fn phenotype_scores(
        &self,
        atlas: &Atlas,
        present: &[TermIdx],
        excluded: &[TermIdx],
    ) -> Vec<(DiseaseIdx, f64, usize, usize, usize)> {
        let present: BTreeSet<_> = present.iter().copied().filter(|t| !excluded.contains(t)).collect();
        if present.is_empty() {
            return vec![];
        }
        let query = closure(atlas, present.iter().copied());
        let query_ic: f64 = query.iter().map(|&t| atlas.hpo.ic(t)).sum();
        let mut out = Vec::new();
        for (d, disease) in atlas.active().filter(|(_, d)| !d.phenotypes.is_empty()) {
            let profile = &self.profiles[d as usize];
            let shared: f64 = query
                .iter()
                .filter(|t| profile.binary_search(t).is_ok())
                .map(|&t| atlas.hpo.ic(t))
                .sum();
            if shared <= 0.0 {
                continue;
            }
            let union = query_ic
                + profile
                    .iter()
                    .filter(|t| query.binary_search(t).is_err())
                    .map(|&t| atlas.hpo.ic(t))
                    .sum::<f64>();
            let matched = present
                .iter()
                .filter(|&&t| {
                    disease
                        .phenotypes
                        .iter()
                        .any(|p| atlas.hpo.ancestors(p.term).binary_search(&t).is_ok())
                })
                .count();
            let conflicts = excluded.iter().filter(|&&t| profile.binary_search(&t).is_ok()).count()
                + present
                    .iter()
                    .filter(|&&t| {
                        disease
                            .excluded
                            .iter()
                            .any(|p| atlas.hpo.ancestors(t).binary_search(&p.term).is_ok())
                    })
                    .count();
            let score = shared / union.max(f64::EPSILON) - conflicts as f64 / present.len().max(1) as f64;
            out.push((d, score, matched, conflicts, present.len()));
        }
        out
    }

    fn rank_phenotypes(
        &self,
        atlas: &Atlas,
        graph: &Graph,
        present: &[TermIdx],
        excluded: &[TermIdx],
        hits: &mut HashMap<NodeKey, Hit>,
    ) {
        let matched_text = present
            .iter()
            .map(|&t| atlas.hpo.term(t).name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        for (d, score, matched, conflicts, total) in self.phenotype_scores(atlas, present, excluded) {
            let key = NodeKey {
                kind: NodeKind::Disease,
                idx: d,
            };
            let mut hit = self.hit(
                atlas,
                graph,
                key,
                &matched_text,
                Reason::Phenotypes,
                350.0 + 100.0 * score,
            );
            if matched == 0 {
                hit.explain("phenotypes", json!({}));
            } else {
                hit.explain("phenotype_overlap", json!({"n":matched,"total":total}));
            }
            if conflicts > 0 {
                hit.append("phenotype_conflict", json!({"n":conflicts}));
            }
            hit.evidence
                .nodes
                .extend(present.iter().chain(excluded).map(|&t| atlas.hpo.term(t).id.clone()));
            if let Some(existing) = hits.get_mut(&key) {
                if matches!(existing.reason, Reason::Nearest) {
                    *existing = hit;
                    continue;
                }
                // Symptoms disambiguate equally ranked gene-linked conditions without overtaking identity.
                if matches!(existing.reason, Reason::CausalGene | Reason::Words) {
                    existing.rank += 10.0 * score.clamp(-1.0, 1.0);
                }
                existing.why_messages.extend(hit.why_messages.clone());
                existing.localise("en");
                existing.evidence.sources.extend(hit.evidence.sources);
                existing.evidence.nodes.extend(hit.evidence.nodes);
            } else {
                insert(hits, hit);
            }
        }
    }
}

fn field_order(reason: Reason) -> u8 {
    match reason {
        Reason::ExactId => 0,
        Reason::ExactName => 1,
        Reason::PreviousSymbol => 2,
        Reason::GeneAlias => 3,
        Reason::Abbreviation => 4,
        Reason::Synonym => 5,
        Reason::Layperson => 6,
        Reason::TranslatedName => 7,
        Reason::TranslatedAlias => 8,
        _ => 9,
    }
}

fn insert(hits: &mut HashMap<NodeKey, Hit>, hit: Hit) {
    match hits.entry(hit.key) {
        std::collections::hash_map::Entry::Vacant(e) => {
            e.insert(hit);
        }
        std::collections::hash_map::Entry::Occupied(mut e) => {
            let old = e.get();
            if hit.rank > old.rank || (hit.rank == old.rank && hit.matched < old.matched) {
                e.insert(hit);
            }
        }
    }
}

fn compare_hits(a: &Hit, b: &Hit) -> std::cmp::Ordering {
    b.rank
        .total_cmp(&a.rank)
        .then(kind_priority(a.node.kind).cmp(&kind_priority(b.node.kind)))
        .then(a.node.id.cmp(&b.node.id))
        .then(a.matched.cmp(&b.matched))
}

fn top_hits<'a>(hits: impl Iterator<Item = &'a Hit>, limit: usize) -> Vec<Hit> {
    let mut hits: Vec<_> = hits.collect();
    if hits.len() > limit {
        hits.select_nth_unstable_by(limit, |a, b| compare_hits(a, b));
        hits.truncate(limit);
    }
    hits.sort_by(|a, b| compare_hits(a, b));
    hits.into_iter().cloned().collect()
}

fn sorted(mut hits: Vec<Hit>) -> Vec<Hit> {
    hits.sort_by(compare_hits);
    hits
}

fn node_id<'a>(atlas: &'a Atlas, graph: &'a Graph, key: NodeKey) -> &'a str {
    match key.kind {
        NodeKind::Disease => &atlas.disease_at(key.idx).id,
        NodeKind::Gene => atlas.gene_at(key.idx).id(),
        NodeKind::Phenotype => &atlas.hpo.term(key.idx).id,
        NodeKind::Study => &graph.study(key.idx).id,
        NodeKind::Grant => &graph.grant(key.idx).id,
        NodeKind::Paper => &graph.paper(key.idx).id,
        NodeKind::Person => &graph.person(key.idx).id,
        NodeKind::Organisation => &graph.org(key.idx).id,
        NodeKind::Asset => &graph.asset(key.idx).id,
        NodeKind::Pathway => unreachable!("pathways are not lexical entries"),
    }
}

fn kind_priority(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Disease => 0,
        NodeKind::Gene => 1,
        NodeKind::Phenotype => 2,
        NodeKind::Organisation => 3,
        NodeKind::Study => 4,
        NodeKind::Asset => 5,
        NodeKind::Person => 6,
        NodeKind::Grant => 7,
        NodeKind::Paper => 8,
        NodeKind::Pathway => 9,
    }
}

fn stopword(w: &str) -> bool {
    [
        "a", "an", "and", "are", "as", "at", "by", "for", "from", "has", "have", "i", "in", "is", "it", "my", "of",
        "on", "or", "the", "to", "was", "with", "find", "please", "no", "not", "without", "der", "die", "das", "eine",
        "einen", "ein", "hat", "mit", "und", "meine", "mein", "ohne", "kein", "keine", "keinen",
    ]
    .contains(&w)
}

fn relation_key(r: Relation) -> &'static str {
    match r {
        Relation::ServesCondition | Relation::ServesGene => "group",
        Relation::StudiesCondition | Relation::StudiedFor => "study",
        Relation::ModelOf => "model",
        Relation::ResourceFor => "resource",
        Relation::Funds => "funding",
        Relation::AboutGene | Relation::AboutCondition | Relation::NamesGene | Relation::ClaimsAbout => "research",
        _ => "neighbour",
    }
}

#[cfg(test)]
mod tests;
