//! Deterministic executors: one per intent, read-only over the atlas (conditions, genes, HPO) and
//! the connected graph (studies, grants, papers, people, organisations).
//!
//! Every statement an executor returns is a [`NewFact`] carrying the edge or node id it comes
//! from; nothing is generated. Slot text is resolved here (ids, names, aliases, typos), and the
//! chip records what it resolved to and the alternatives.
//!
//! Thin adapters over atlas-core live in [`view`]; they mirror logic that today sits in the
//! atlas-server binary (causal genes, gene edge ids, connection collection) and should move into a
//! library crate (see the crate docs).

pub mod condition;
pub mod connections;
pub mod related;
pub mod view;

use atlas_core::node::{NodeKey, NodeKind, NodeRef};
use atlas_core::search::SearchOptions;
use atlas_core::text::normalize_label;
use atlas_core::{Atlas, DiseaseIdx, GeneIdx, Graph, TermIdx};

use crate::facts::{FactBook, NewFact};
use crate::intent::{Chip, ChipStatus, IntentKind, Resolved};

/// Read-only view of the data an executor may use.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub atlas: &'a Atlas,
    pub graph: &'a Graph,
}

/// A resolved slot value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Disease(DiseaseIdx),
    Gene(GeneIdx),
    Phenotype(TermIdx),
    Graph(NodeKey),
}

/// One chip being run: collects facts and notes onto the chip.
pub struct Run<'c> {
    pub ctx: Ctx<'c>,
    pub chip: &'c mut Chip,
    pub book: &'c mut FactBook,
}

impl Run<'_> {
    pub fn fact(&mut self, f: NewFact) -> String {
        let key = self.book.add(&self.chip.id, f);
        if !self.chip.facts.contains(&key) {
            self.chip.facts.push(key.clone());
        }
        key
    }

    pub fn note(&mut self, msg: serde_json::Value) {
        self.chip
            .notes
            .push(msg["fallback"].as_str().unwrap_or_default().to_owned());
        self.chip.notes_msg.push(msg);
    }

    pub fn slot(&self, name: &str) -> Option<String> {
        self.chip.slot(name).map(str::to_owned)
    }

    fn record(&mut self, slot: &str, node: NodeRef, alternatives: Vec<NodeRef>) {
        self.chip
            .resolved
            .insert(slot.to_owned(), Resolved { node, alternatives });
    }

    /// Resolve a condition slot (a gene picks its best-matching condition).
    pub fn condition(&mut self, slot: &str) -> Result<DiseaseIdx, String> {
        let text = self.slot(slot).ok_or_else(|| format!("slot '{slot}' is empty"))?;
        let (d, alts) = self.ctx.resolve_condition(&text)?;
        let node = self.ctx.atlas.disease_ref(d);
        self.record(slot, node, alts);
        Ok(d)
    }

    /// Resolve any node slot.
    pub fn node(&mut self, slot: &str) -> Result<Target, String> {
        let text = self.slot(slot).ok_or_else(|| format!("slot '{slot}' is empty"))?;
        let (t, alts) = self
            .ctx
            .resolve_any(&text)
            .ok_or_else(|| format!("nothing in the atlas matches '{text}'"))?;
        let node = self.ctx.target_ref(t);
        self.record(slot, node, alts);
        Ok(t)
    }

    /// Resolve a gene slot (symbol, alias, HGNC id).
    pub fn gene(&mut self, slot: &str) -> Result<GeneIdx, String> {
        let text = self.slot(slot).ok_or_else(|| format!("slot '{slot}' is empty"))?;
        let g = self
            .ctx
            .resolve_gene(&text)
            .ok_or_else(|| format!("no gene in the atlas matches '{text}'"))?;
        let node = self.ctx.target_ref(Target::Gene(g));
        self.record(slot, node, vec![]);
        Ok(g)
    }

    /// Resolve a symptom slot (HPO id, name, layperson synonym).
    pub fn symptom(&mut self, slot: &str) -> Result<TermIdx, String> {
        let text = self.slot(slot).ok_or_else(|| format!("slot '{slot}' is empty"))?;
        let (t, alts) = self
            .ctx
            .resolve_symptom(&text)
            .ok_or_else(|| format!("no symptom (HPO term) matches '{text}'"))?;
        let node = self.ctx.atlas.term_ref(t);
        self.record(slot, node, alts);
        Ok(t)
    }
}

/// Run one chip: validate its slots, execute, record status, facts and notes on it.
pub fn execute(ctx: Ctx<'_>, chip: &mut Chip, book: &mut FactBook) {
    chip.reset();
    let issues = chip.check();
    if !issues.is_empty() {
        chip.notes = issues;
        chip.notes_msg = chip
            .notes
            .iter()
            .map(|_| crate::copy::msg("ask.chip.invalid", serde_json::json!({"intent": chip.intent.as_str()})))
            .collect();
        chip.status = Some(ChipStatus::Invalid);
        return;
    }
    let intent = chip.intent;
    let mut run = Run { ctx, chip, book };
    let result = match intent {
        IntentKind::Resolve => condition::resolve(&mut run),
        IntentKind::ConditionSummaryFacts => condition::summary(&mut run),
        IntentKind::Gaps => condition::gaps(&mut run),
        IntentKind::NodeDetails => condition::details(&mut run),
        IntentKind::Connections => connections::connections(&mut run),
        IntentKind::Assets => connections::assets(&mut run),
        IntentKind::SharedPeople => connections::shared_people(&mut run),
        IntentKind::Related => related::related(&mut run),
        IntentKind::DiseasesWith => related::diseases_with(&mut run),
        IntentKind::Path => related::path(&mut run),
    };
    chip.status = Some(match result {
        Err(e) => {
            chip.notes.push(e);
            chip.notes_msg.push(crate::copy::msg(
                "ask.chip.invalid",
                serde_json::json!({"intent": chip.intent.as_str()}),
            ));
            ChipStatus::Invalid
        }
        Ok(()) if chip.facts.is_empty() => ChipStatus::Empty,
        Ok(()) => ChipStatus::Found,
    });
}

/// Search hits of these kinds, best first; retries once with the typo-corrected query.
fn search_nodes(atlas: &Atlas, text: &str, limit: usize) -> Vec<NodeKey> {
    let opts = SearchOptions {
        limit,
        include_retired: false,
    };
    let mut hits: Vec<NodeKey> = atlas.search().search(text, opts).into_iter().map(|h| h.node).collect();
    if hits.is_empty()
        && let Some(fixed) = atlas.search().correct(text)
    {
        hits = atlas
            .search()
            .search(&fixed, opts)
            .into_iter()
            .map(|h| h.node)
            .collect();
    }
    hits
}

impl<'a> Ctx<'a> {
    pub fn new(atlas: &'a Atlas, graph: &'a Graph) -> Self {
        Self { atlas, graph }
    }

    pub fn target_ref(&self, t: Target) -> NodeRef {
        match t {
            Target::Disease(d) => self.atlas.disease_ref(d),
            Target::Gene(g) => self.atlas.node_ref(NodeKey {
                kind: NodeKind::Gene,
                idx: g,
            }),
            Target::Phenotype(p) => self.atlas.term_ref(p),
            Target::Graph(k) => self.graph.node_ref(k),
        }
    }

    /// Node id of a target (the id graph edges use).
    pub fn target_id(&self, t: Target) -> String {
        self.target_ref(t).id
    }

    /// Gene by symbol, HGNC/NCBI id, or HGNC alias / previous symbol.
    pub fn resolve_gene(&self, text: &str) -> Option<GeneIdx> {
        let t = text.trim();
        self.atlas
            .gene(t)
            .or_else(|| self.atlas.gene(&t.to_uppercase()))
            .or_else(|| {
                self.graph
                    .genes_by_alias(t)
                    .find_map(|a| self.atlas.gene(&a.symbol).or_else(|| self.atlas.gene(&a.hgnc)))
            })
    }

    /// Symptom by HPO id or by search (name, synonym, layperson word).
    pub fn resolve_symptom(&self, text: &str) -> Option<(TermIdx, Vec<NodeRef>)> {
        if let Some(t) = self.atlas.hpo.canonical(text.trim()) {
            return Some((t, vec![]));
        }
        let terms: Vec<TermIdx> = search_nodes(self.atlas, text, 20)
            .into_iter()
            .filter(|k| k.kind == NodeKind::Phenotype)
            .map(|k| k.idx)
            .collect();
        let (&first, rest) = terms.split_first()?;
        Some((first, rest.iter().take(3).map(|&t| self.atlas.term_ref(t)).collect()))
    }

    /// Condition for free text: id, name, alias, typo, gene symbol or alias.
    pub fn resolve_condition(&self, text: &str) -> Result<(DiseaseIdx, Vec<NodeRef>), String> {
        let text = text.trim();
        if let Some(d) = self.atlas.disease_idx(text) {
            return if self.atlas.disease_at(d).is_active() {
                Ok((d, vec![]))
            } else {
                Err(format!("{text} is a retired entry in the atlas"))
            };
        }
        if let Some(g) = self.atlas.gene(text).or_else(|| self.atlas.gene(&text.to_uppercase())) {
            return self.condition_for_gene(g);
        }
        let hits = search_nodes(self.atlas, text, 12);
        let diseases: Vec<DiseaseIdx> = hits
            .iter()
            .filter(|k| k.kind == NodeKind::Disease)
            .map(|k| k.idx)
            .collect();
        let first_gene = hits.iter().find(|k| k.kind == NodeKind::Gene).map(|k| k.idx);
        match (hits.first(), diseases.split_first()) {
            (Some(k), _) if k.kind == NodeKind::Gene => self.condition_for_gene(k.idx),
            (_, Some((&d, rest))) => Ok((d, rest.iter().take(3).map(|&r| self.atlas.disease_ref(r)).collect())),
            _ => match first_gene.or_else(|| self.resolve_gene(text)) {
                Some(g) => self.condition_for_gene(g),
                None => Err(format!("no condition in the atlas matches '{text}'")),
            },
        }
    }

    /// The condition a gene symbol most likely stands for: one named after the gene, else the
    /// best-annotated rare one. Alternatives: the gene's other conditions.
    pub fn condition_for_gene(&self, g: GeneIdx) -> Result<(DiseaseIdx, Vec<NodeRef>), String> {
        let gene = self.atlas.gene_at(g);
        let symbol = normalize_label(&gene.symbol);
        let named = |d: DiseaseIdx| {
            let dis = self.atlas.disease_at(d);
            std::iter::once(dis.name.as_str())
                .chain(dis.synonyms.iter().map(|n| n.text.as_str()))
                .any(|n| normalize_label(n).split(' ').any(|w| w == symbol))
        };
        let causal = |d: DiseaseIdx| {
            view::causal_genes(self.atlas, d)
                .iter()
                .any(|c| c.symbol == gene.symbol)
        };
        let mut ranked: Vec<DiseaseIdx> = gene
            .diseases
            .iter()
            .copied()
            .filter(|&d| self.atlas.disease_at(d).is_active())
            .collect();
        ranked.sort_by_key(|&d| {
            let dis = self.atlas.disease_at(d);
            (
                !named(d),
                !causal(d),
                !dis.rare,
                std::cmp::Reverse(dis.phenotypes.len()),
                d,
            )
        });
        let (&best, rest) = ranked
            .split_first()
            .ok_or_else(|| format!("the gene {} has no condition in the atlas", gene.symbol))?;
        Ok((best, rest.iter().take(4).map(|&d| self.atlas.disease_ref(d)).collect()))
    }

    /// Any node: graph id, condition, gene, HPO term, then names (search, organisations, people).
    pub fn resolve_any(&self, text: &str) -> Option<(Target, Vec<NodeRef>)> {
        let t = text.trim();
        if let Some(k) = self.graph.node(t) {
            return Some((Target::Graph(k), vec![]));
        }
        if let Some(d) = self.atlas.disease_idx(t) {
            return Some((Target::Disease(d), vec![]));
        }
        if let Some(g) = self.resolve_gene(t) {
            return Some((Target::Gene(g), vec![]));
        }
        if let Some(p) = self.atlas.hpo.canonical(t) {
            return Some((Target::Phenotype(p), vec![]));
        }
        if let Some(k) = self.graph_by_name(t) {
            return Some((Target::Graph(k), vec![]));
        }
        let hits = search_nodes(self.atlas, t, 6);
        let (first, rest) = hits.split_first()?;
        let to_target = |k: &NodeKey| match k.kind {
            NodeKind::Disease => Target::Disease(k.idx),
            NodeKind::Gene => Target::Gene(k.idx),
            _ => Target::Phenotype(k.idx),
        };
        Some((
            to_target(first),
            rest.iter().take(3).map(|k| self.atlas.node_ref(*k)).collect(),
        ))
    }

    /// Organisation or person whose name equals the text (normalised).
    fn graph_by_name(&self, text: &str) -> Option<NodeKey> {
        let q = normalize_label(text);
        if q.is_empty() {
            return None;
        }
        let data = self.graph.data();
        data.orgs
            .iter()
            .position(|o| normalize_label(&o.name) == q)
            .map(|i| NodeKey {
                kind: NodeKind::Organisation,
                idx: i as u32,
            })
            .or_else(|| {
                data.people
                    .iter()
                    .position(|p| normalize_label(&p.name) == q)
                    .map(|i| NodeKey {
                        kind: NodeKind::Person,
                        idx: i as u32,
                    })
            })
    }

    /// Label of any id (graph node, condition, gene, HPO term), else the id.
    pub fn label(&self, id: &str) -> String {
        if let Some(k) = self.graph.node(id) {
            return self.graph.node_ref(k).label;
        }
        if let Some(d) = self.atlas.disease_idx(id) {
            return self.atlas.disease_at(d).name.clone();
        }
        if let Some(g) = self.atlas.gene(id) {
            return self.atlas.gene_at(g).symbol.clone();
        }
        if let Some(t) = self.atlas.hpo.canonical(id) {
            return self.atlas.hpo.term(t).name.clone();
        }
        id.to_owned()
    }
}
