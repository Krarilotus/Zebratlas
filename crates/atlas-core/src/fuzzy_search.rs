//! Compact lexical candidate retrieval. Similar names are choices, never diagnoses or graph edges.
//! Build offline from serving-visible source labels/synonyms. Runtime callers MUST recheck current
//! visibility through `search` before ranking/paging; snapshots do not authorize disclosure.
use crate::node::{NodeKey, NodeKind};
use crate::text::normalize_label;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub const MAX_INDEX_BYTES: usize = 40 * 1024 * 1024;
const MAX_DOCS: usize = 700_000;
const MAX_POSTINGS: usize = 5_000_000;
const MAX_VISITS: usize = 100_000;
const MAX_QUERY_WORDS: usize = 12;
const MAX_WORD_CHARS: usize = 64;
const FORMAT: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    kind: NodeKind,
    idx: u32,
    length: u16,
}
impl Document {
    fn key(&self) -> NodeKey {
        NodeKey {
            kind: self.kind,
            idx: self.idx,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Word {
    text: Box<str>,
    start: u32,
    length: u32,
}
#[derive(Serialize, Deserialize)]
struct Gram {
    hash: u32,
    start: u32,
    length: u32,
}
#[derive(Serialize, Deserialize)]
pub struct CompactFuzzyIndex {
    format: u32,
    documents: Vec<Document>,
    words: Vec<Word>,
    postings: Vec<u32>,
    grams: Vec<Gram>,
    gram_postings: Vec<u32>,
    average_length: f32,
    truncated_documents: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct FuzzyStats {
    pub documents: usize,
    pub vocabulary: usize,
    pub postings: usize,
    pub gram_postings: usize,
    pub heap_bytes: usize,
    pub truncated_documents: usize,
}
#[derive(Debug, Clone)]
pub struct FuzzyCandidate {
    pub node: NodeKey,
    pub score: f32,
    pub matched_words: usize,
    pub query_words: usize,
    pub typo: bool,
}
#[derive(Debug, Default)]
pub struct FuzzyCandidates {
    pub candidates: Vec<FuzzyCandidate>,
    pub truncated: bool,
    pub visited: usize,
}
#[derive(Default)]
pub struct FuzzyBuilder {
    documents: Vec<Document>,
    words: HashMap<Box<str>, Vec<u32>>,
    postings: usize,
    truncated_documents: usize,
}

fn meaningful(word: &str) -> bool {
    word.chars().count() >= 2
        && !matches!(
            word,
            "a" | "an"
                | "and"
                | "are"
                | "as"
                | "at"
                | "be"
                | "by"
                | "can"
                | "could"
                | "do"
                | "does"
                | "for"
                | "from"
                | "has"
                | "have"
                | "how"
                | "i"
                | "in"
                | "is"
                | "it"
                | "looking"
                | "me"
                | "my"
                | "of"
                | "on"
                | "or"
                | "our"
                | "please"
                | "search"
                | "that"
                | "the"
                | "their"
                | "there"
                | "this"
                | "to"
                | "was"
                | "we"
                | "what"
                | "where"
                | "which"
                | "who"
                | "will"
                | "with"
                | "would"
                | "you"
                | "your"
                | "bitte"
                | "das"
                | "der"
                | "die"
                | "ein"
                | "eine"
                | "einen"
                | "einer"
                | "für"
                | "hat"
                | "ich"
                | "im"
                | "ist"
                | "mit"
                | "oder"
                | "suche"
                | "und"
                | "von"
                | "welche"
                | "wie"
                | "wir"
                | "zu"
        )
}
fn grams(text: &str) -> Vec<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<u32> = chars
        .windows(3)
        .map(|window| {
            window.iter().fold(2_166_136_261u32, |hash, c| {
                (hash ^ (*c as u32)).wrapping_mul(16_777_619)
            })
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
fn distance(left: &str, right: &str, max: usize) -> usize {
    let a: Vec<char> = left.chars().collect();
    let b: Vec<char> = right.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut two = vec![0; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        row[0] = i;
        let mut minimum = i;
        for j in 1..=b.len() {
            let mut cost = (previous[j] + 1)
                .min(row[j - 1] + 1)
                .min(previous[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cost = cost.min(two[j - 2] + 1);
            }
            row[j] = cost;
            minimum = minimum.min(cost);
        }
        if minimum > max {
            return max + 1;
        }
        std::mem::swap(&mut two, &mut previous);
        std::mem::swap(&mut previous, &mut row);
    }
    previous[b.len()]
}
impl FuzzyBuilder {
    pub fn add(&mut self, node: NodeKey, text: &str) -> Result<(), String> {
        let normalized = normalize_label(text);
        let mut words: Vec<&str> = normalized
            .split(' ')
            .filter(|word| meaningful(word) && word.chars().count() <= MAX_WORD_CHARS)
            .collect();
        words.sort_unstable();
        words.dedup();
        if words.is_empty() {
            return Ok(());
        }
        if words.len() > 32 {
            words.truncate(32);
            self.truncated_documents += 1;
        }
        if self.documents.len() >= MAX_DOCS || self.postings + words.len() > MAX_POSTINGS {
            return Err("Fuzzy index exceeds bounded document/posting budget".into());
        }
        let index = self.documents.len() as u32;
        self.documents.push(Document {
            kind: node.kind,
            idx: node.idx,
            length: words.len() as u16,
        });
        self.postings += words.len();
        for word in words {
            self.words.entry(word.into()).or_default().push(index);
        }
        Ok(())
    }
    pub fn finish(self) -> Result<CompactFuzzyIndex, String> {
        let mut terms: Vec<_> = self.words.into_iter().collect();
        terms.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let mut words = Vec::with_capacity(terms.len());
        let mut postings = Vec::with_capacity(self.postings);
        let mut by_gram: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (index, (text, documents)) in terms.into_iter().enumerate() {
            for hash in grams(&text) {
                by_gram.entry(hash).or_default().push(index as u32);
            }
            words.push(Word {
                text,
                start: postings.len() as u32,
                length: documents.len() as u32,
            });
            postings.extend(documents);
        }
        let mut gram_postings = Vec::new();
        let mut gram_ranges = Vec::with_capacity(by_gram.len());
        for (hash, indexes) in by_gram {
            gram_ranges.push(Gram {
                hash,
                start: gram_postings.len() as u32,
                length: indexes.len() as u32,
            });
            gram_postings.extend(indexes);
        }
        gram_postings.shrink_to_fit();
        let average_length = self.documents.iter().map(|doc| doc.length as usize).sum::<usize>() as f32
            / self.documents.len().max(1) as f32;
        let mut index = CompactFuzzyIndex {
            format: FORMAT,
            documents: self.documents,
            words,
            postings,
            grams: gram_ranges,
            gram_postings,
            average_length,
            truncated_documents: self.truncated_documents,
        };
        index.documents.shrink_to_fit();
        index.postings.shrink_to_fit();
        if index.stats().heap_bytes > MAX_INDEX_BYTES {
            return Err("Fuzzy index exceeds 40 MiB heap budget".into());
        }
        Ok(index)
    }
}
impl CompactFuzzyIndex {
    pub fn stats(&self) -> FuzzyStats {
        FuzzyStats {
            documents: self.documents.len(),
            vocabulary: self.words.len(),
            postings: self.postings.len(),
            gram_postings: self.gram_postings.len(),
            heap_bytes: self.documents.capacity() * std::mem::size_of::<Document>()
                + self.words.capacity() * std::mem::size_of::<Word>()
                + self.words.iter().map(|word| word.text.len()).sum::<usize>()
                + self.postings.capacity() * 4
                + self.grams.capacity() * std::mem::size_of::<Gram>()
                + self.gram_postings.capacity() * 4,
            truncated_documents: self.truncated_documents,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|error| error.to_string())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        use bincode::Options;
        if bytes.len() > MAX_INDEX_BYTES {
            return Err("Fuzzy artifact exceeds bounded input budget".into());
        }
        let index: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_INDEX_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|error| error.to_string())?;
        if index.format != FORMAT
            || index.documents.len() > MAX_DOCS
            || index.postings.len() > MAX_POSTINGS
            || index.stats().heap_bytes > MAX_INDEX_BYTES
        {
            return Err("Invalid fuzzy artifact version or bounds".into());
        }
        if index.words.windows(2).any(|pair| pair[0].text >= pair[1].text)
            || index.grams.windows(2).any(|pair| pair[0].hash >= pair[1].hash)
            || index.words.iter().any(|word| {
                (word.start as usize)
                    .checked_add(word.length as usize)
                    .is_none_or(|end| end > index.postings.len())
            })
            || index.grams.iter().any(|gram| {
                (gram.start as usize)
                    .checked_add(gram.length as usize)
                    .is_none_or(|end| end > index.gram_postings.len())
            })
            || index.postings.iter().any(|&doc| doc as usize >= index.documents.len())
            || index
                .gram_postings
                .iter()
                .any(|&word| word as usize >= index.words.len())
        {
            return Err("Invalid fuzzy artifact offsets".into());
        }
        Ok(index)
    }
    fn word_index(&self, text: &str) -> Option<usize> {
        self.words.binary_search_by(|word| word.text.as_ref().cmp(text)).ok()
    }
    fn symbol_word(&self, index: usize, weight: f32) -> bool {
        let word = &self.words[index];
        weight >= 0.78
            && (4..=12).contains(&word.text.chars().count())
            && self.postings[word.start as usize..(word.start + word.length) as usize]
                .iter()
                .take(1024)
                .any(|&doc| {
                    self.documents[doc as usize].kind == NodeKind::Gene && self.documents[doc as usize].length == 1
                })
    }
    fn expansions(&self, text: &str) -> Vec<(usize, f32, bool)> {
        if let Some(index) = self.word_index(text) {
            return vec![(index, 1.0, false)];
        }
        let start = self.words.partition_point(|word| word.text.as_ref() < text);
        let prefixes: Vec<_> = self.words[start..]
            .iter()
            .take_while(|word| word.text.starts_with(text))
            .take(8)
            .enumerate()
            .map(|(offset, _)| (start + offset, 0.9, false))
            .collect();
        if text.chars().count() >= 4 && !prefixes.is_empty() {
            return prefixes;
        }
        if text.chars().count() < 4 {
            return vec![];
        }
        // Common inflections are lexical only; no clinical or mechanistic inference.
        for suffix in ["es", "s", "ed", "ing"] {
            if let Some(stem) = text.strip_suffix(suffix).filter(|stem| stem.chars().count() >= 4) {
                if let Some(index) = self.word_index(stem) {
                    return vec![(index, 0.86, true)];
                }
            }
        }
        let query_grams = grams(text);
        let mut candidates: HashMap<u32, usize> = HashMap::new();
        let mut ranges: Vec<_> = query_grams
            .iter()
            .filter_map(|hash| self.grams.binary_search_by_key(hash, |gram| gram.hash).ok())
            .map(|index| &self.grams[index])
            .collect();
        ranges.sort_unstable_by_key(|gram| gram.length);
        for gram in ranges.into_iter().take(6) {
            for &index in self.gram_postings[gram.start as usize..(gram.start + gram.length) as usize]
                .iter()
                .take(20_000)
            {
                *candidates.entry(index).or_default() += 1;
            }
        }
        let max = if text.chars().count() >= 8 { 2 } else { 1 };
        let mut ranked: Vec<_> = candidates
            .into_iter()
            .filter_map(|(index, overlap)| {
                let word = &self.words[index as usize];
                let required = query_grams.len().saturating_sub(max * 3).max(1).min(3);
                if overlap < required {
                    return None;
                }
                let edit = distance(text, &word.text, max);
                (edit <= max).then_some((index as usize, edit, overlap))
            })
            .collect();
        ranked.sort_unstable_by_key(|&(index, edit, overlap)| {
            (
                edit,
                !self.symbol_word(index, 0.78),
                std::cmp::Reverse(overlap),
                self.words[index].length,
                index,
            )
        });
        ranked
            .into_iter()
            .take(4)
            .map(|(index, edit, _)| (index, if edit == 1 { 0.78 } else { 0.65 }, true))
            .collect()
    }
    /// Candidate scores are lexical relevance, not probabilities or scientific evidence.
    /// Invisible documents are excluded before accumulation and top-N selection.
    pub fn search(&self, query: &str, limit: usize, is_visible: impl Fn(NodeKey) -> bool) -> FuzzyCandidates {
        if query.len() > 2048 {
            return FuzzyCandidates {
                truncated: true,
                ..Default::default()
            };
        }
        if limit == 0 {
            return FuzzyCandidates::default();
        }
        let normalized = normalize_label(query);
        let mut query_words: Vec<&str> = normalized
            .split(' ')
            .filter(|word| meaningful(word) && word.chars().count() <= MAX_WORD_CHARS)
            .collect();
        query_words.sort_unstable();
        query_words.dedup();
        let all_words = query_words.len();
        if all_words == 0 {
            return FuzzyCandidates::default();
        }
        let mut terms: Vec<_> = query_words
            .iter()
            .filter_map(|word| {
                let expansions = self.expansions(word);
                (!expansions.is_empty()).then_some((*word, expansions))
            })
            .collect();
        terms.sort_unstable_by_key(|(_, expansions)| {
            (
                !expansions
                    .iter()
                    .any(|(index, weight, _)| self.symbol_word(*index, *weight)),
                expansions
                    .iter()
                    .map(|(index, _, _)| self.words[*index].length)
                    .min()
                    .unwrap_or(u32::MAX),
            )
        });
        let mut truncated = terms.len() > MAX_QUERY_WORDS;
        terms.truncate(MAX_QUERY_WORDS);
        // Unknown words count against coverage: one accidental word cannot answer a long query.
        let has_symbol = terms.iter().any(|(_, expansions)| {
            expansions
                .iter()
                .any(|(index, weight, _)| self.symbol_word(*index, *weight))
        });
        if terms.is_empty() {
            return FuzzyCandidates::default();
        }
        let vocabulary_covered = terms.len() as f32 / (all_words as f32) >= 0.4;
        #[derive(Default)]
        struct Score {
            value: f32,
            mask: u16,
            typo: bool,
        }
        let mut scores: HashMap<u32, Score> = HashMap::new();
        let mut visibility: HashMap<NodeKey, bool> = HashMap::new();
        let mut visited = 0;
        'term: for (position, (_, expansions)) in terms.iter().enumerate() {
            for &(word_index, weight, typo) in expansions {
                let word = &self.words[word_index];
                let idf =
                    (1.0 + (self.documents.len() as f32 - word.length as f32 + 0.5) / (word.length as f32 + 0.5)).ln();
                for &doc_index in &self.postings[word.start as usize..(word.start + word.length) as usize] {
                    if visited >= MAX_VISITS {
                        truncated = true;
                        break 'term;
                    }
                    visited += 1;
                    let doc = &self.documents[doc_index as usize];
                    if !*visibility.entry(doc.key()).or_insert_with(|| is_visible(doc.key())) {
                        continue;
                    }
                    let score = scores.entry(doc_index).or_default();
                    let bit = 1u16 << position;
                    if score.mask & bit != 0 {
                        continue;
                    }
                    score.value += weight * idf * 2.2
                        / (1.0 + 1.2 * (0.25 + 0.75 * doc.length as f32 / self.average_length.max(1.0)));
                    score.mask |= bit;
                    score.typo |= typo;
                }
            }
        }
        let required = if all_words <= 2 {
            1
        } else {
            ((all_words as f32) * 0.45).ceil() as usize
        };
        let mut nodes: HashMap<NodeKey, (bool, FuzzyCandidate)> = HashMap::new();
        for (index, score) in scores {
            let matched = score.mask.count_ones() as usize;
            let doc = &self.documents[index as usize];
            let symbol_candidate = doc.kind == NodeKind::Gene && doc.length == 1 && has_symbol;
            // A complete indexed clinical name/alias stays selectable inside prose, never a diagnosis.
            let clinical_name = matches!(doc.kind, NodeKind::Disease | NodeKind::Phenotype)
                && doc.length >= 2
                && matched >= doc.length as usize;
            if (!(symbol_candidate || clinical_name) && (!vocabulary_covered || matched < required))
                || !score.value.is_finite()
                || score.value < 0.6
            {
                continue;
            }
            let hit = FuzzyCandidate {
                node: doc.key(),
                score: score.value,
                matched_words: matched,
                query_words: all_words,
                typo: score.typo,
            };
            if nodes
                .get(&hit.node)
                .is_none_or(|(old_symbol, old): &(bool, FuzzyCandidate)| {
                    (!*old_symbol && symbol_candidate) || (*old_symbol == symbol_candidate && old.score < hit.score)
                })
            {
                nodes.insert(hit.node, (symbol_candidate, hit));
            }
        }
        let mut candidates: Vec<_> = nodes.into_values().collect();
        candidates.sort_unstable_by(|(a_symbol, a), (b_symbol, b)| {
            b_symbol
                .cmp(a_symbol)
                .then(b.score.total_cmp(&a.score))
                .then(a.node.cmp(&b.node))
        });
        candidates.truncate(limit.min(10));
        FuzzyCandidates {
            candidates: candidates.into_iter().map(|(_, hit)| hit).collect(),
            truncated,
            visited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(kind: NodeKind, idx: u32) -> NodeKey {
        NodeKey { kind, idx }
    }
    fn index() -> CompactFuzzyIndex {
        let mut builder = FuzzyBuilder::default();
        for (node, text) in [
            (key(NodeKind::Gene, 0), "STXBP1"),
            (key(NodeKind::Gene, 0), "MUNC18-1"),
            (key(NodeKind::Disease, 0), "developmental and epileptic encephalopathy"),
            (key(NodeKind::Phenotype, 0), "Seizure"),
            (key(NodeKind::Phenotype, 0), "Fits"),
            (key(NodeKind::Phenotype, 1), "Delayed development"),
            (key(NodeKind::Disease, 1), "Rett syndrome"),
            (
                key(NodeKind::Organisation, 0),
                "Organisation für Entwicklungsverzögerung",
            ),
            (key(NodeKind::Asset, 0), "Antisense protein silencing model"),
        ] {
            builder.add(node, text).unwrap();
        }
        builder.finish().unwrap()
    }
    #[test]
    fn typo_alias_phrase_unicode_and_noise() {
        let index = index();
        for (query, node) in [
            ("stxbpi", key(NodeKind::Gene, 0)),
            ("MUNC18-1", key(NodeKind::Gene, 0)),
            ("epileptic encefalopathy", key(NodeKind::Disease, 0)),
            ("fits", key(NodeKind::Phenotype, 0)),
            ("delayed development", key(NodeKind::Phenotype, 1)),
            ("Entwicklungsverzögerung", key(NodeKind::Organisation, 0)),
            ("protein silencing", key(NodeKind::Asset, 0)),
        ] {
            let found = index.search(query, 10, |_| true);
            assert!(
                found.candidates.iter().any(|hit| hit.node == node),
                "Missing candidate for {query}"
            );
        }
        assert!(index
            .search("zqxvplkj astronaut dishwasher pineapple", 10, |_| true)
            .candidates
            .is_empty());
        assert!(
            index
                .search("please find patient support for stxbpi", 10, |_| true)
                .candidates
                .iter()
                .any(|hit| hit.node == key(NodeKind::Gene, 0)),
            "A gene-name typo inside a question stays an explicit name candidate, never an assigned diagnosis"
        );
        let long_question = "I am looking for patient researchers organisations studies papers resources support collaborators development epilepsy therapy funding treatments natural history registries and information about stxbpi";
        assert!(
            index
                .search(long_question, 10, |_| true)
                .candidates
                .iter()
                .any(|hit| hit.node == key(NodeKind::Gene, 0)),
            "Late source symbols survive bounded whole-input selection"
        );
        assert!(index.search("Could you find researchers patient support resources funding studies treatments and shared pathways relevant to Rett syndrom",10, |_|true).candidates.iter().any(|hit|hit.node==key(NodeKind::Disease,1)), "A late complete disease-name typo stays a candidate inside prose");
    }
    #[test]
    fn current_visibility_precedes_ranking_and_paging() {
        let index = index();
        assert!(index
            .search("stxbp1", 1, |node| node.kind != NodeKind::Gene)
            .candidates
            .is_empty());
        assert!(index.search("protein", 10, |_| false).candidates.is_empty());
        assert_eq!(index.search("stxbp1", 0, |_| true).candidates.len(), 0);
    }
    #[test]
    fn artifact_roundtrip_and_invalid_bounds() {
        let index = index();
        let bytes = index.encode().unwrap();
        let loaded = CompactFuzzyIndex::decode(&bytes).unwrap();
        assert_eq!(loaded.stats().documents, index.stats().documents);
        assert_eq!(
            loaded.search("stxbpi", 1, |_| true).candidates[0].node,
            key(NodeKind::Gene, 0)
        );
        let mut bad = bytes.clone();
        bad[0] = 255;
        assert!(CompactFuzzyIndex::decode(&bad).is_err());
        assert!(CompactFuzzyIndex::decode(&vec![0; MAX_INDEX_BYTES + 1]).is_err());
    }
}
