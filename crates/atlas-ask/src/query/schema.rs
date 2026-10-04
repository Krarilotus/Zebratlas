use super::*;
use oxrdfio::{RdfFormat, RdfParser};
use serde::{Deserialize, Serialize};
use spargebra::term::Term;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Read},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lineage {
    pub source_url: String,
    pub retrieved_at: String,
    pub version: String,
    pub sha256: String,
    pub record_locator: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Predicate {
    pub count: u64,
    /// Observed subject/object types, NOT OWL domain/range axioms.
    pub observed_domain: BTreeSet<String>,
    pub observed_range: BTreeSet<String>,
    pub example: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaCard {
    pub schema_version: u32,
    pub activity: serde_json::Value,
    pub graph: Lineage,
    pub statements: u64,
    pub classes: BTreeMap<String, String>,
    pub predicates: BTreeMap<String, Predicate>,
    /// Vocabulary seeded by verified queries but absent in this release. Never inferred facts.
    pub known_absent: BTreeSet<String>,
    pub prefixes: BTreeMap<String, String>,
    pub semantic_units: BTreeMap<String, String>,
    pub provenance_pattern: String,
}

impl SchemaCard {
    /// Two strict streaming passes; no dataset rewrite, skipped records, or retained triple store.
    pub fn generate(path: &Path, mut graph: Lineage) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut buf = [0; 64 * 1024];
        loop {
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        let actual = format!("{:x}", hash.finalize());
        if !graph.sha256.is_empty() && graph.sha256 != actual {
            return Err("graph checksum mismatch".into());
        }
        graph.sha256 = actual;
        let mut types: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut classes = BTreeMap::new();
        let parser = || -> Result<_, String> {
            Ok(RdfParser::from_format(RdfFormat::Turtle)
                .for_reader(BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?)))
        };
        for q in parser()? {
            let q = q.map_err(|e| e.to_string())?;
            if q.predicate.as_str() == format!("{RDF}type") {
                if let Term::NamedNode(class) = q.object {
                    classes
                        .entry(class.as_str().to_owned())
                        .or_insert_with(|| q.subject.to_string());
                    types
                        .entry(q.subject.to_string())
                        .or_default()
                        .insert(class.as_str().into());
                }
            } else if q.predicate.as_str() == format!("{RA}nodeKind")
                && let Term::Literal(kind) = q.object
            {
                types
                    .entry(q.subject.to_string())
                    .or_default()
                    .insert(format!("nodeKind:{}", kind.value()));
            }
        }
        let activity = serde_json::json!({"@type":"prov:Activity","@id":format!("urn:atlas:schema:{}",graph.sha256),"prov:used":{"@type":"prov:Entity","source_url":graph.source_url,"sha256":graph.sha256,"version":graph.version,"retrieved_at":graph.retrieved_at,"record_locator":graph.record_locator},"prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","name":"atlas-ask/schema","version":env!("CARGO_PKG_VERSION")},"parameters":{"passes":2,"parser":"oxrdfio strict RDF 1.2","skipped":0}});
        let mut card = Self { schema_version: 1, activity, graph, statements: 0, classes,
            predicates: BTreeMap::new(), known_absent: BTreeSet::new(),
            prefixes: [ ("ra", RA), ("prov", PROV), ("rdf", RDF), ("rdfs", RDFS), ("dcterms", DCT),
                ("owl", "http://www.w3.org/2002/07/owl#"), ("skos", "http://www.w3.org/2004/02/skos/core#"), ("id", "https://w3id.org/rare-disease-atlas/id/") ]
                .into_iter().map(|(k,v)| (k.into(),v.into())).collect(),
            semantic_units: BTreeMap::new(), provenance_pattern:
                "edge prov:qualifiedDerivation derivation; derivation prov:entity record, prov:hadActivity activity; record dcterms:source URL, ra:recordLocator locator, ra:retrievedAt time, dcterms:hasVersion version, ra:sha256 digest, dcterms:isPartOf source; activity prov:wasAssociatedWith agent. Missing fields remain unverified.".into() };
        for q in parser()? {
            let q = q.map_err(|e| e.to_string())?;
            card.statements += 1;
            let p = card.predicates.entry(q.predicate.as_str().into()).or_default();
            p.count += 1;
            if p.example.is_empty() {
                p.example = vec![q.subject.to_string(), q.object.to_string()];
            }
            if let Some(t) = types.get(&q.subject.to_string()) {
                p.observed_domain.extend(t.clone());
            }
            match q.object {
                Term::Literal(l) => {
                    p.observed_range.insert(l.datatype().as_str().into());
                }
                Term::NamedNode(_) | Term::BlankNode(_) => {
                    if let Some(t) = types.get(&q.object.to_string()) {
                        p.observed_range.extend(t.clone());
                    }
                }
                Term::Triple(_) => {
                    p.observed_range.insert(format!("{RDF}TripleTerm"));
                }
            }
        }
        // Query-seed vocabulary is explicitly distinguished from observed predicates.
        let seed_relations = [
            "serves_condition",
            "names_gene",
            "studies_condition",
            "participates_in",
            "model_of",
            "resource_for",
            "funds",
            "has_associated_gene",
        ];
        for p in seed_relations
            .into_iter()
            .chain(atlas_core::graph::Relation::ALL.into_iter().map(|r| r.as_str()))
        {
            let p = format!("{RA}{p}");
            if !card.predicates.contains_key(&p) {
                card.known_absent.insert(p);
            }
        }
        for p in [
            format!("{RDFS}subClassOf"),
            "http://www.w3.org/2002/07/owl#sameAs".into(),
            "http://www.w3.org/2004/02/skos/core#exactMatch".into(),
        ] {
            if !card.predicates.contains_key(&p) {
                card.known_absent.insert(p);
            }
        }
        if card.classes.contains_key(&format!("{RA}Edge")) {
            card.semantic_units.insert("evidence_connection".into(),
                "Derived view: asserted relation + ra:Edge reifier + source chain; edgeKind distinguishes observed/extracted/inferred/hypothesis. Never recover endpoints of link_only edges.".into());
        }
        Ok(card)
    }

    pub fn allows_predicate(&self, p: &str) -> bool {
        self.predicates.contains_key(p) || self.known_absent.contains(p)
    }
    /// Import only the semantic-unit contract's identity and definitions, never pretend its
    /// separate Turtle export is already loaded into this release.
    pub fn with_semantic_profile(mut self, path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let hash = digest(&bytes);
        let url = format!(
            "file:///{}",
            std::fs::canonicalize(path)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .trim_start_matches("\\\\?\\")
                .replace('\\', "/")
        );
        self.semantic_units.insert("profile_source".into(),format!("{url}; version:snapshot-sha256:{hash}; sha256:{hash}; retrieved_at:{}; locator:Implemented profile and boundaries",atlas_core::provenance::rfc3339(std::fs::metadata(path).map_err(|e|e.to_string())?.modified().map_err(|e|e.to_string())?)));
        for (kind, class, definition) in [
            (
                "statement",
                "StatementUnit",
                "one sourced relationship or qualified shared-pathway proposition; derived connections retain both membership proofs",
            ),
            ("item", "ItemUnit", "statements sharing one subject"),
            (
                "community",
                "CommunityUnit",
                "condition-linked or causal-gene-linked resources with sourced official contacts; compound assembly is inferred",
            ),
            (
                "mechanism_group",
                "MechanismItemGroupUnit",
                "direct Reactome pathway with at least two gene items; co-membership does not establish therapeutic compatibility",
            ),
        ] {
            let state = if self.classes.contains_key(&format!("{RA}{class}")) {
                "observed"
            } else {
                "NOT LOADED in this release; separate /api/units.ttl export required"
            };
            self.semantic_units
                .insert(kind.into(), format!("ra:{class}: {definition}; {state}"));
        }
        Ok(self)
    }
    /// Planning vocabulary, not a record dump: keep every observed predicate and
    /// its direction/types while excluding arbitrary example literals and URLs.
    pub fn compact(&self) -> String {
        let short = |s: &str| {
            let mut out = s.to_owned();
            for (k, v) in &self.prefixes {
                out = out.replace(v, &format!("{k}:"));
            }
            out
        };
        let mut out = format!(
            "schema v{}; graph {} sha256:{}\nPrefixes: {:?}\nClasses:\n",
            self.schema_version, self.graph.version, self.graph.sha256, self.prefixes
        );
        for c in self.classes.keys() {
            out.push_str(&format!("{}\n", short(c)));
        }
        out.push_str("Predicates (observed domain/range; empty = untyped; not OWL axioms):\n");
        for (p, d) in &self.predicates {
            out.push_str(&format!(
                "{}: {:?} -> {:?}; count={}\n",
                short(p),
                d.observed_domain.iter().map(|s| short(s)).collect::<Vec<_>>(),
                d.observed_range.iter().map(|s| short(s)).collect::<Vec<_>>(),
                d.count
            ));
        }
        out.push_str(&format!(
            "Known vocabulary ABSENT from release: {:?}\nUnits: {:?}\n{}\n",
            self.known_absent.iter().map(|s| short(s)).collect::<Vec<_>>(),
            self.semantic_units
                .iter()
                .filter(|(key, _)| key.as_str() != "profile_source")
                .collect::<BTreeMap<_, _>>(),
            self.provenance_pattern
        ));
        out
    }
}
