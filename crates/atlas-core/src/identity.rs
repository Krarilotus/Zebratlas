//! Disease identity across OMIM, Orphanet and MONDO.
//!
//! Only *authoritative exact* equivalences merge nodes (MONDO equivalentTo, validated Orphanet E
//! mappings). Broader/narrower mappings become typed links, never merges, so OMIM genetic subtypes
//! and Orphanet clinical groups keep their own granularity. Contradictions are recorded.
//!
//! A name match only *nominates* (D21, D30.1): an unmapped source id whose name equals the name or
//! an exact synonym of one MONDO term keeps its own node and gets a typed `candidate_same_as` link
//! carrying the matched labels and the guard that passed. [`LabelPolicy::Merge`] reproduces the
//! pre-D30 behaviour (merge on the label) for frozen historical evaluations only.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::curie::{self, normalize};
use crate::provenance::activity;
use crate::term::{Scope, Term};
use crate::text::normalize_label;

/// Orphanet mapping relation code -> link relation (source id relative to target).
const RELATION: &[(&str, &str)] = &[
    ("E", "exact"),
    ("NTBT", "narrower"),
    ("BTNT", "broader"),
    ("ND", "related"),
];

/// One Orphanet external reference (`ExternalReferenceList/ExternalReference`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    /// OMIM, MONDO, ICD-10, ICD-11, UMLS, MeSH, MedDRA, GARD.
    pub source: String,
    pub reference: String,
    /// E exact, NTBT narrower, BTNT broader, ND not decided, W wrong (deprecated).
    pub relation: String,
    pub validated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdLink {
    pub target: String,
    /// exact / narrower / broader / related (source id relative to target).
    pub relation: String,
    /// MONDO / Orphanet.
    pub asserted_by: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub source_id: String,
    pub candidates: Vec<String>,
    pub asserted_by: String,
}

/// Why a source id was merged into a canonical node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeBasis {
    Mondo,
    Orphanet,
    /// Only under [`LabelPolicy::Merge`] (pre-D30 reproduction).
    Label,
}

impl MergeBasis {
    /// Python spelling: `MONDO` / `Orphanet` / `label`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mondo => "MONDO",
            Self::Orphanet => "Orphanet",
            Self::Label => "label",
        }
    }

    /// The public mapping basis of a merge.
    pub fn mapping_basis(self) -> MappingBasis {
        match self {
            Self::Mondo => MappingBasis::ExactMondo,
            Self::Orphanet => MappingBasis::OrphanetE,
            Self::Label => MappingBasis::LabelMerge,
        }
    }

    /// The provenance activity that performs this kind of merge.
    pub fn activity_id(self) -> &'static str {
        match self {
            Self::Mondo => activity::IDENTITY_MONDO_EXACT,
            Self::Orphanet => activity::IDENTITY_ORPHANET_EXACT,
            Self::Label => activity::IDENTITY_LABEL,
        }
    }
}

/// How a source id relates to a MONDO term, as published per id (D30.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingBasis {
    /// Merged: MONDO asserts `equivalentTo` for this id.
    ExactMondo,
    /// Merged: a validated Orphanet `E` (exact) mapping.
    OrphanetE,
    /// Not merged: the id keeps its own node; a name match nominates a MONDO term.
    Candidate,
    /// Merged on a name match; only under [`LabelPolicy::Merge`] (pre-D30 reproduction).
    LabelMerge,
}

impl MappingBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactMondo => "exact_mondo",
            Self::OrphanetE => "orphanet_e",
            Self::Candidate => "candidate",
            Self::LabelMerge => "label_merge",
        }
    }
}

/// What a name match does (D30.1). `Candidate` is the atlas policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelPolicy {
    /// Keep the source id as its own node; record a `candidate_same_as` link.
    #[default]
    Candidate,
    /// Merge into the MONDO term (pre-D30). Only to reproduce frozen historical runs.
    Merge,
}

/// Relation name of a label-nominated link (source id -> MONDO candidate).
pub const CANDIDATE_SAME_AS: &str = "candidate_same_as";

/// The guard every candidate passed (key), and the rule it stands for.
pub const LABEL_GUARD: &str = "unique_label_one_per_source";
pub const LABEL_GUARD_RULE: &str = "a name or synonym of the source id equals (normalised) the name or a \
     non-abbreviation exact synonym of exactly one live MONDO term, and that term has no exact mapping or \
     earlier candidate from the same source prefix";

/// A label-nominated, *unconfirmed* identity: `source_id candidate_same_as target`. Never a merge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub source_id: String,
    /// The MONDO candidate.
    pub target: String,
    /// The MONDO term's name.
    pub target_label: String,
    /// Source names (as written) whose normalised form matched a MONDO name or exact synonym.
    pub matched_labels: Vec<String>,
    /// [`LABEL_GUARD`].
    pub guard: String,
}

/// A merge and the record that asserted it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergeRecord {
    pub basis: MergeBasis,
    /// MONDO term carrying the xref, Orphanet disorder carrying the mapping, or the matched label.
    pub asserted_in: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiseaseIdentity {
    /// Live (non-obsolete) MONDO ids.
    mondo: HashSet<String>,
    canonical: HashMap<String, String>,
    merged_by: HashMap<String, MergeRecord>,
    links: HashMap<String, Vec<IdLink>>,
    conflicts: Vec<Conflict>,
    /// MONDO id -> prefixes already merged into it.
    exact_prefixes: HashMap<String, HashSet<String>>,
    /// Normalised name or exact non-abbreviation synonym -> MONDO ids.
    labels: HashMap<String, BTreeSet<String>>,
    conflicted: HashSet<String>,
    tried: HashSet<String>,
    /// Live MONDO id -> name.
    mondo_names: HashMap<String, String>,
    label_policy: LabelPolicy,
    /// Source id -> label-nominated candidate (never in `canonical`).
    candidates: BTreeMap<String, Candidate>,
    /// MONDO id -> prefixes already nominated onto it (the one-per-source guard).
    candidate_prefixes: HashMap<String, HashSet<String>>,
}

impl DiseaseIdentity {
    /// `mondo`: every term of mondo.obo in file order; `orphanet`: (ORPHA id, mappings) per disorder.
    pub fn new<'a>(mondo: &[Term], orphanet: impl IntoIterator<Item = (&'a str, &'a [Mapping])>) -> Self {
        Self::from_sources(mondo, orphanet, None)
    }

    /// Build original disease identities using only the verified accepted mapping closure.
    /// Revocation is a fresh construction, never an attempted in-place split of merged records.
    pub fn new_gated<'a>(
        mondo: &[Term],
        orphanet: impl IntoIterator<Item = (&'a str, &'a [Mapping])>,
        accepted: &crate::identity_policy::AcceptedIdentity,
    ) -> Self {
        Self::from_sources(mondo, orphanet, Some(accepted))
    }

    fn from_sources<'a>(
        mondo: &[Term],
        orphanet: impl IntoIterator<Item = (&'a str, &'a [Mapping])>,
        accepted: Option<&crate::identity_policy::AcceptedIdentity>,
    ) -> Self {
        let mut s = Self::default();
        let live = |t: &&Term| t.id.starts_with("MONDO:") && !t.obsolete;
        let mut exact: IndexMap<String, BTreeSet<&str>> = IndexMap::new();
        for term in mondo.iter().filter(live) {
            s.mondo.insert(term.id.clone());
            s.mondo_names.insert(term.id.clone(), term.name.clone());
            s.canonical.insert(term.id.clone(), term.id.clone());
        }
        for term in mondo.iter().filter(live) {
            let mid = &term.id;
            for x in &term.xrefs {
                if x.sources.iter().any(|src| src == "MONDO:equivalentTo")
                    && accepted.is_none_or(|a| a.equivalent(mid, &x.id))
                {
                    exact.entry(normalize(&x.id)).or_default().insert(mid);
                }
            }
            s.labels
                .entry(normalize_label(&term.name))
                .or_default()
                .insert(mid.clone());
            for syn in &term.synonyms {
                if syn.scope == Scope::Exact && syn.kind.as_deref() != Some("ABBREVIATION") {
                    s.labels
                        .entry(normalize_label(&syn.text))
                        .or_default()
                        .insert(mid.clone());
                }
            }
        }
        for t in mondo {
            // retired MONDO ids point at their replacement
            if let Some(r) = &t.replaced_by
                && t.obsolete
                && s.mondo.contains(r)
                && accepted.is_none_or(|a| a.equivalent(&t.id, r))
            {
                s.canonical.insert(t.id.clone(), r.clone());
            }
        }

        for (source_id, targets) in exact {
            if targets.len() == 1 {
                let target = targets.first().unwrap().to_string();
                s.merge(&source_id, &target, MergeBasis::Mondo, &target);
            } else {
                let candidates = targets.into_iter().map(str::to_owned).collect();
                s.conflicts.push(Conflict {
                    source_id,
                    candidates,
                    asserted_by: "MONDO".into(),
                });
            }
        }

        for (orpha, mappings) in orphanet {
            for m in mappings {
                if !matches!(m.source.as_str(), "OMIM" | "MONDO") || !m.validated {
                    continue;
                }
                let other = normalize(&format!("{}:{}", m.source, m.reference));
                let Some(&(_, relation)) = RELATION.iter().find(|(code, _)| *code == m.relation) else {
                    continue;
                };
                if relation != "exact" {
                    let link = IdLink {
                        target: other,
                        relation: relation.into(),
                        asserted_by: "Orphanet".into(),
                    };
                    s.links.entry(orpha.to_owned()).or_default().push(link);
                    continue;
                }
                if accepted.is_some_and(|a| !a.equivalent(orpha, &other)) {
                    continue;
                }
                let a = s.canonical.get(orpha).filter(|v| !v.is_empty()).cloned();
                let b = s.canonical.get(&other).filter(|v| !v.is_empty()).cloned();
                match (a, b) {
                    (Some(a), Some(b)) if a != b => s.conflicts.push(Conflict {
                        source_id: orpha.to_owned(),
                        candidates: vec![a, b],
                        asserted_by: "Orphanet".into(),
                    }),
                    (Some(a), None) => s.merge(&other, &a, MergeBasis::Orphanet, orpha),
                    (None, Some(b)) => s.merge(orpha, &b, MergeBasis::Orphanet, orpha),
                    _ => {}
                }
            }
        }
        s.conflicted = s.conflicts.iter().map(|c| c.source_id.clone()).collect();
        s
    }

    fn merge(&mut self, source_id: &str, target: &str, basis: MergeBasis, asserted_in: &str) {
        self.canonical.insert(source_id.to_owned(), target.to_owned());
        let record = MergeRecord {
            basis,
            asserted_in: asserted_in.to_owned(),
        };
        self.merged_by.insert(source_id.to_owned(), record);
        self.exact_prefixes
            .entry(target.to_owned())
            .or_default()
            .insert(curie::prefix(source_id).to_owned());
    }

    /// Choose what a name match does; set before the label pass.
    pub fn with_label_policy(mut self, policy: LabelPolicy) -> Self {
        self.label_policy = policy;
        self
    }

    pub fn label_policy(&self) -> LabelPolicy {
        self.label_policy
    }

    /// Second, weaker pass for ids no exact mapping covers; returns the node id for `source_id`.
    ///
    /// When a name equals the name or an exact synonym of exactly one MONDO term, and that term has
    /// no exact mapping (or earlier nomination) from the same source yet, the id is *nominated*: it
    /// keeps its own node and gets a [`Candidate`] link (D30.1). Under [`LabelPolicy::Merge`] it is
    /// merged instead (pre-D30). Otherwise the id is a distinct concept that merely shares a name,
    /// e.g. a retired or regional Orphanet entry.
    pub fn nominate_by_label<S: AsRef<str>>(&mut self, source_id: &str, names: &[S]) -> String {
        let source_id = normalize(source_id);
        if self.canonical.contains_key(&source_id)
            || self.conflicted.contains(&source_id)
            || self.tried.contains(&source_id)
        {
            return self.resolve(&source_id);
        }
        self.tried.insert(source_id.clone());
        let prefix = curie::prefix(&source_id).to_owned();
        let mut targets: BTreeSet<&String> = BTreeSet::new();
        let mut matched: Vec<String> = Vec::new();
        for name in names {
            if let Some(ids) = self.labels.get(&normalize_label(name.as_ref())) {
                targets.extend(ids);
                if !matched.iter().any(|m| m == name.as_ref()) {
                    matched.push(name.as_ref().to_owned());
                }
            }
        }
        if targets.len() != 1 {
            return source_id;
        }
        let target = (*targets.first().unwrap()).clone();
        let taken = |m: &HashMap<String, HashSet<String>>| m.get(&target).is_some_and(|p| p.contains(&prefix));
        if taken(&self.exact_prefixes) || taken(&self.candidate_prefixes) {
            return source_id;
        }
        if self.label_policy == LabelPolicy::Merge {
            let asserted = format!("{target} label \"{}\"", matched.last().map_or("", String::as_str));
            self.merge(&source_id, &target, MergeBasis::Label, &asserted);
            return target;
        }
        self.candidate_prefixes
            .entry(target.clone())
            .or_default()
            .insert(prefix);
        let candidate = Candidate {
            source_id: source_id.clone(),
            target_label: self.mondo_names.get(&target).cloned().unwrap_or_default(),
            target,
            matched_labels: matched,
            guard: LABEL_GUARD.into(),
        };
        self.candidates.insert(source_id.clone(), candidate);
        source_id
    }

    /// Canonical node id: the MONDO term if an exact equivalence exists, else the source id.
    pub fn resolve(&self, id: &str) -> String {
        let id = normalize(id);
        match self.canonical.get(&id) {
            Some(c) => c.clone(),
            None => id,
        }
    }

    pub fn is_live_mondo(&self, id: &str) -> bool {
        self.mondo.contains(id)
    }

    /// How `source_id` was merged, if it was.
    pub fn merged_by(&self, source_id: &str) -> Option<&MergeRecord> {
        self.merged_by.get(&normalize(source_id))
    }

    /// The label-nominated MONDO candidate of `source_id`, if any (never a merge).
    pub fn candidate(&self, source_id: &str) -> Option<&Candidate> {
        self.candidates.get(&normalize(source_id))
    }

    /// Every candidate link, by source id.
    pub fn candidates(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates.values()
    }

    /// Candidates nominating `mondo` (incoming `candidate_same_as` links), by source id.
    pub fn candidates_for<'a>(&'a self, mondo: &'a str) -> impl Iterator<Item = &'a Candidate> {
        self.candidates.values().filter(move |c| c.target == mondo)
    }

    /// Public mapping basis of a source id: exact MONDO / Orphanet E (merged) or candidate (not).
    /// `None`: a MONDO term itself, an id resolved via `replaced_by`/normalisation, or unmapped.
    pub fn mapping_basis(&self, source_id: &str) -> Option<MappingBasis> {
        let id = normalize(source_id);
        match self.merged_by.get(&id) {
            Some(m) => Some(m.basis.mapping_basis()),
            None => self.candidates.contains_key(&id).then_some(MappingBasis::Candidate),
        }
    }

    /// Number of merged source ids per basis.
    pub fn merge_counts(&self) -> [(MergeBasis, usize); 3] {
        let n = |b| self.merged_by.values().filter(|r| r.basis == b).count();
        [MergeBasis::Mondo, MergeBasis::Orphanet, MergeBasis::Label].map(|b| (b, n(b)))
    }

    /// Typed non-exact links asserted for an Orphanet id.
    pub fn links(&self, orpha: &str) -> &[IdLink] {
        self.links.get(orpha).map_or(&[], Vec::as_slice)
    }

    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Conflicts naming `id` as source or candidate.
    pub fn conflicts_of<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Conflict> {
        self.conflicts
            .iter()
            .filter(move |c| c.source_id == id || c.candidates.iter().any(|x| x == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::{Synonym, Xref};

    fn mondo() -> Vec<Term> {
        let xref = |id: &str, src: &str| Xref {
            id: id.into(),
            sources: vec![src.into()],
        };
        let mut a = Term::new("MONDO:0000001");
        a.name = "some disease".into();
        a.xrefs = vec![
            xref("OMIM:100100", "MONDO:equivalentTo"),
            xref("Orphanet:42", "MONDO:equivalentTo"),
            xref("MESH:D0001", "MONDO:MEDGEN"),
        ];
        let mut b = Term::new("MONDO:0000002");
        b.name = "subtype".into();
        b.xrefs = vec![xref("OMIM:100200", "MONDO:equivalentTo")];
        b.synonyms = vec![Synonym {
            text: "Sub type".into(),
            scope: Scope::Exact,
            kind: None,
        }];
        vec![a, b]
    }

    fn mapping(source: &str, reference: &str, relation: &str) -> Mapping {
        Mapping {
            source: source.into(),
            reference: reference.into(),
            relation: relation.into(),
            validated: true,
        }
    }

    #[test]
    fn revoked_native_equivalence_does_not_reappear_on_rebuild() {
        use crate::identity_policy::AcceptedIdentity;
        let terms = mondo();
        let gate = AcceptedIdentity::new(
            [(String::from("MONDO:0000001"), String::from("OMIM:100100"))],
            "accepted".into(),
        );
        let identity = DiseaseIdentity::new_gated(&terms, std::iter::empty(), &gate);
        assert_eq!(identity.resolve("OMIM:100100"), "MONDO:0000001");
        assert_eq!(identity.resolve("ORPHA:42"), "ORPHA:42");
        let revoked = DiseaseIdentity::new_gated(&terms, std::iter::empty(), &AcceptedIdentity::default());
        assert_eq!(revoked.resolve("OMIM:100100"), "OMIM:100100");
        assert!(revoked.merged_by("OMIM:100100").is_none());
    }

    #[test]
    fn merges_exact_only() {
        let maps = [mapping("OMIM", "100200", "BTNT"), mapping("OMIM", "100100", "E")];
        let ident = DiseaseIdentity::new(&mondo(), [("ORPHA:7", &maps[..])]);
        assert_eq!(ident.resolve("OMIM:100100"), "MONDO:0000001");
        assert_eq!(ident.resolve("Orphanet:42"), "MONDO:0000001");
        assert_eq!(ident.resolve("MESH:D0001"), "MESH:D0001");
        // ORPHA:7 is exact to OMIM:100100, which MONDO equates with MONDO:0000001 -> adopt
        assert_eq!(ident.resolve("ORPHA:7"), "MONDO:0000001");
        assert_eq!(ident.merged_by("ORPHA:7").unwrap().basis, MergeBasis::Orphanet);
        assert_eq!(ident.links("ORPHA:7")[0].relation, "broader");
    }

    #[test]
    fn label_match_nominates_never_merges() {
        let mut ident = DiseaseIdentity::new(&mondo(), []);
        assert_eq!(ident.nominate_by_label("OMIM:9", &["SUB-TYPE"]), "OMIM:9"); // OMIM already exact on it
        assert!(ident.candidate("OMIM:9").is_none());
        assert_eq!(ident.nominate_by_label("ORPHA:9", &["x", "sub type"]), "ORPHA:9"); // own node
        assert_eq!(ident.resolve("ORPHA:9"), "ORPHA:9");
        assert!(ident.merged_by("ORPHA:9").is_none());
        let c = ident.candidate("ORPHA:9").unwrap();
        assert_eq!(
            (c.target.as_str(), c.target_label.as_str()),
            ("MONDO:0000002", "subtype")
        );
        assert_eq!(c.matched_labels, ["sub type"]);
        assert_eq!(c.guard, LABEL_GUARD);
        assert_eq!(ident.mapping_basis("ORPHA:9"), Some(MappingBasis::Candidate));
        assert_eq!(ident.mapping_basis("OMIM:100200"), Some(MappingBasis::ExactMondo));
        assert_eq!(ident.nominate_by_label("ORPHA:10", &["sub type"]), "ORPHA:10"); // one per source
        assert!(ident.candidate("ORPHA:10").is_none());
        assert_eq!(ident.candidates_for("MONDO:0000002").count(), 1);
        assert_eq!(ident.merge_counts()[2].1, 0);
    }

    #[test]
    fn legacy_policy_merges_on_label() {
        let mut ident = DiseaseIdentity::new(&mondo(), []).with_label_policy(LabelPolicy::Merge);
        assert_eq!(ident.nominate_by_label("ORPHA:9", &["sub type"]), "MONDO:0000002");
        assert_eq!(ident.nominate_by_label("ORPHA:10", &["sub type"]), "ORPHA:10"); // one per source
        assert_eq!(ident.merged_by("ORPHA:9").unwrap().basis, MergeBasis::Label);
        assert_eq!(ident.mapping_basis("ORPHA:9"), Some(MappingBasis::LabelMerge));
        assert_eq!(ident.candidates().count(), 0);
    }
}
