//! `/api/export.ttl?condition=…`: a condition's subgraph as RDF 1.2 Turtle with PROV-O.
//!
//! Nodes use their upstream IRIs (OBO PURLs, identifiers.org, ClinicalTrials.gov, PubMed, ORCID)
//! and schema.org classes; every edge is an asserted triple with an RDF 1.2 reifier
//! (`~ edge:… {| … |}`) carrying its kind, level, reason, `prov:wasGeneratedBy` and
//! `prov:wasDerivedFrom` source records (with sha256, locator, fetched_at). Activities and source
//! files follow as `prov:Activity` / `prov:Entity`. Generated in chunks on a blocking thread and
//! streamed.

use std::collections::BTreeSet;
use std::fmt::Write;

use atlas_core::graph::{GraphEdge, RecIdx, RecordWithhold, Relation, hex};
use atlas_core::node::NodeKind;
use atlas_core::provenance::Provenance;
use atlas_core::{Atlas, DiseaseIdx, Graph, curie};

use crate::nodes;

const ATLAS: &str = "https://w3id.org/rare-disease-atlas/";

const PREFIXES: &str = r#"VERSION "1.2"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix prov: <http://www.w3.org/ns/prov#> .
@prefix dcterms: <http://purl.org/dc/terms/> .
@prefix schema: <https://schema.org/> .
@prefix ra: <https://w3id.org/rare-disease-atlas/vocab#> .
@prefix edge: <https://w3id.org/rare-disease-atlas/edge/> .
@prefix rec: <https://w3id.org/rare-disease-atlas/record/> .
@prefix act: <https://w3id.org/rare-disease-atlas/activity/> .
@prefix src: <https://w3id.org/rare-disease-atlas/source/> .

"#;

/// Percent-encode everything but unreserved characters (local names inside `<…>`).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// IRI of a node id.
pub fn iri(id: &str) -> String {
    let (prefix, local) = curie::split(id);
    let url = match prefix {
        "MONDO" | "HP" | "GO" => format!("http://purl.obolibrary.org/obo/{prefix}_{local}"),
        "HGNC" => format!("https://identifiers.org/hgnc:{local}"),
        "NCBIGene" => format!("https://identifiers.org/ncbigene:{local}"),
        "OMIM" => format!("https://omim.org/entry/{local}"),
        "ORPHA" => format!("https://www.orpha.net/en/disease/detail/{local}"),
        "PMID" => format!("https://pubmed.ncbi.nlm.nih.gov/{local}/"),
        "ORCID" => format!("https://orcid.org/{local}"),
        _ if id.starts_with("NCT") => format!("https://clinicaltrials.gov/study/{id}"),
        _ => format!("{ATLAS}id/{}", enc(id)),
    };
    format!("<{url}>")
}

fn class(kind: NodeKind, graph: &Graph, id: &str) -> &'static str {
    match kind {
        NodeKind::Disease => "schema:MedicalCondition",
        NodeKind::Gene => "schema:Gene",
        NodeKind::Study => match graph.node(id).map(|k| graph.study(k.idx).kind) {
            Some(atlas_core::graph::StudyKind::Trial) => "schema:MedicalTrial",
            _ => "schema:MedicalObservationalStudy",
        },
        NodeKind::Paper => "schema:ScholarlyArticle",
        NodeKind::Grant => "schema:Grant",
        NodeKind::Person => "schema:Person",
        NodeKind::Organisation => "schema:Organization",
        _ => "rdfs:Resource",
    }
}

/// schema.org property where one fits, else the atlas vocabulary; `inverse` swaps subject/object.
fn predicate(r: Relation) -> (&'static str, bool) {
    match r {
        Relation::AuthorOf => ("schema:author", true),
        Relation::SponsoredBy => ("schema:sponsor", false),
        Relation::SameAs => ("schema:sameAs", false),
        Relation::StudiesCondition => ("schema:healthCondition", false),
        Relation::PrincipalInvestigatorOf => ("ra:principalInvestigatorOf", false),
        Relation::NamesGene => ("ra:namesGene", false),
        Relation::AboutGene => ("ra:aboutGene", false),
        Relation::AboutCondition => ("ra:aboutCondition", false),
        Relation::AwardedTo => ("ra:awardedTo", false),
        Relation::ServesCondition => ("ra:servesCondition", false),
        Relation::ServesGene => ("ra:servesGene", false),
        Relation::HeldBy => ("ra:heldBy", false),
        Relation::ModelOf => ("ra:modelOf", false),
        Relation::StudiedFor => ("ra:studiedFor", false),
        Relation::Targets => ("ra:targets", false),
        Relation::ResourceFor => ("ra:resourceFor", false),
        Relation::Funds => ("ra:funds", false),
        Relation::ClaimsAbout => ("ra:claimsAbout", false),
        Relation::HasPhenotype => ("ra:hasPhenotype", false),
        Relation::OrthologousTo => ("ra:orthologousTo", false),
        Relation::GeneAssociatedWithCondition => ("ra:geneAssociatedWithCondition", false),
        Relation::CandidateSameAs => ("ra:candidateSameAs", false),
        Relation::RelatedTo => ("ra:relatedTo", false),
    }
}

/// Edges of the subgraph: everything touching the condition, its causal genes and its umbrella
/// groups' studies; then authorship, PI, identity and sponsor edges of the nodes reached.
pub fn edges(atlas: &Atlas, graph: &Graph, d: DiseaseIdx) -> Vec<u32> {
    let cid = &atlas.disease_at(d).id;
    let mut core: Vec<String> = vec![cid.clone()];
    core.extend(nodes::causal_genes(atlas, d).into_iter().map(|g| g.0));
    core.extend(graph.umbrella(cid).iter().map(|(a, _)| a.clone()));
    let mut set: BTreeSet<u32> = BTreeSet::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for id in &core {
        for inc in graph.incident(id) {
            if set.insert(inc.idx) {
                reached.insert(inc.other.to_owned());
            }
        }
    }
    for id in &reached {
        for inc in graph.incident(id) {
            if matches!(
                inc.edge.relation,
                Relation::AuthorOf
                    | Relation::PrincipalInvestigatorOf
                    | Relation::SameAs
                    | Relation::SponsoredBy
                    | Relation::AwardedTo
            ) {
                set.insert(inc.idx);
            }
        }
    }
    // quarantined records never reach an export (fail-closed)
    set.into_iter()
        .filter(|&i| {
            let e = graph.edge(i);
            let hidden = |id: &str| graph.node(id).is_some_and(|k| graph.node_withheld(k).is_some());
            graph.records_withheld(&e.records).is_none() && !hidden(&e.from) && !hidden(&e.to)
        })
        .collect()
}

pub struct Export<'a> {
    atlas: &'a Atlas,
    graph: &'a Graph,
    d: DiseaseIdx,
    edges: Vec<u32>,
    pos: usize,
    stage: u8,
    records: BTreeSet<RecIdx>,
    nodes: BTreeSet<String>,
}

impl<'a> Export<'a> {
    pub fn new(atlas: &'a Atlas, graph: &'a Graph, d: DiseaseIdx) -> Self {
        Self {
            atlas,
            graph,
            d,
            edges: edges(atlas, graph, d),
            pos: 0,
            stage: 0,
            records: BTreeSet::new(),
            nodes: BTreeSet::new(),
        }
    }

    fn edge(&mut self, out: &mut String, e: &GraphEdge) {
        let (p, inverse) = predicate(e.relation);
        let (s, o) = if inverse { (&e.to, &e.from) } else { (&e.from, &e.to) };
        self.nodes.insert(e.from.clone());
        self.nodes.insert(e.to.clone());
        let _ = writeln!(out, "{} {p} {} ~ edge:{} {{|", iri(s), iri(o), enc(&e.id()));
        let _ = writeln!(
            out,
            "    a ra:Link ; ra:relation {} ; ra:edgeKind {} ; ra:level {} ;",
            lit(e.relation.as_str()),
            lit(e.kind.as_str()),
            lit(e.level.as_str())
        );
        let _ = writeln!(out, "    ra:reason {} ;", lit(&e.reason));
        let _ = write!(
            out,
            "    prov:wasGeneratedBy act:{}",
            enc(&self.graph.provenance().activity(e.activity).id)
        );
        let act = enc(&self.graph.provenance().activity(e.activity).id);
        for r in &e.records {
            self.records.insert(*r);
            let _ = write!(
                out,
                " ;\n    prov:wasDerivedFrom rec:{r} ;\n    prov:qualifiedDerivation [ a prov:Derivation ; prov:entity rec:{r} ; prov:hadActivity act:{act} ]"
            );
        }
        for endpoint in [&e.from, &e.to] {
            if let Some(merge) = self.graph.identity_merge(endpoint) {
                if merge
                    .members
                    .iter()
                    .any(|m| self.graph.records_withheld(&m.derived_from).is_some())
                {
                    continue;
                }
                for mapping in &merge.mappings {
                    if !mapping.decision_id.is_empty() {
                        let _ = write!(
                            out,
                            " ;\n    prov:qualifiedDerivation [ a prov:Derivation ; prov:entity <{}> ; prov:hadActivity act:{} ]",
                            mapping.decision_id,
                            enc(&merge.activity.id)
                        );
                    }
                }
            }
        }
        out.push_str(" |} .\n");
    }

    fn atlas_genes(&self, out: &mut String) {
        let d = self.atlas.disease_at(self.d);
        let _ = writeln!(
            out,
            "{} a schema:MedicalCondition ; rdfs:label {} .",
            iri(&d.id),
            lit(&d.name)
        );
        for (node, links, edge) in crate::views::gene_edges(self.atlas, &d.id, &d.genes) {
            let _ = writeln!(
                out,
                "{} a schema:Gene ; rdfs:label {} .",
                iri(&node.id),
                lit(&node.label)
            );
            let _ = writeln!(
                out,
                "{} ra:hasAssociatedGene {} ~ edge:{} {{|",
                iri(&d.id),
                iri(&node.id),
                enc(&edge.id)
            );
            let _ = write!(out, "    a ra:Link ; ra:edgeKind \"observed\"");
            for l in links {
                let cite = self.atlas.provenance.cite(&l.record);
                let e = self.atlas.provenance.entity(l.record.entity);
                let _ = write!(
                    out,
                    " ;\n    ra:association {} ;\n    prov:wasDerivedFrom [ a prov:Entity ; ra:locator {} ; dcterms:isPartOf src:{} ]",
                    lit(&l.association),
                    lit(&cite),
                    enc(&e.id)
                );
            }
            out.push_str(" |} .\n");
        }
    }

    fn node_lines(&self, out: &mut String) {
        for id in &self.nodes {
            let Some(r) = nodes::any_ref(self.atlas, self.graph, id) else {
                continue;
            };
            let _ = writeln!(
                out,
                "{} a {} ; rdfs:label {} ; dcterms:identifier {} .",
                iri(id),
                class(r.kind, self.graph, id),
                lit(&r.label),
                lit(id)
            );
        }
    }

    fn record_lines(&self, out: &mut String) {
        for &r in &self.records {
            let rec = self.graph.record(r);
            let e = self.graph.provenance().entity(rec.entity);
            let _ = writeln!(
                out,
                "rec:{r} a prov:Entity ; dcterms:identifier {} ; ra:locator {} ;",
                lit(&rec.id),
                lit(&rec.locator.to_string())
            );
            let _ = writeln!(
                out,
                "    ra:sha256 {} ; ra:hashForm {} ;",
                lit(&hex(&rec.sha256)),
                lit(&format!("{:?}", rec.hash))
            );
            if let Some(u) = &rec.url {
                let _ = writeln!(
                    out,
                    "    prov:atLocation <{}> ;",
                    u.replace('>', "%3E").replace(' ', "%20")
                );
            }
            if let Some(t) = &rec.fetched_at {
                let _ = writeln!(out, "    prov:generatedAtTime {}^^xsd:dateTime ;", lit(t));
            }
            let _ = writeln!(
                out,
                "    prov:wasDerivedFrom src:{id} ;\n    dcterms:isPartOf src:{id} .",
                id = enc(&e.id)
            );
        }
    }

    fn identity_lines(&mut self, out: &mut String) {
        let mut ids = self.nodes.clone();
        ids.insert(self.atlas.disease_at(self.d).id.clone());
        ids.extend(nodes::causal_genes(self.atlas, self.d).into_iter().map(|g| g.0));
        for merge in &self.graph.data().identity_merges {
            if !ids.contains(&merge.canonical)
                || merge
                    .members
                    .iter()
                    .any(|m| self.graph.records_withheld(&m.derived_from).is_some())
                || self
                    .graph
                    .node(&merge.canonical)
                    .is_some_and(|k| self.graph.node_withheld(k).is_some())
            {
                continue;
            }
            let act = enc(&merge.activity.id);
            let _ = writeln!(
                out,
                "act:{act} a prov:Activity ; rdfs:label \"identity merge\" ; prov:wasAssociatedWith [ a prov:SoftwareAgent ; rdfs:label {} ; ra:version {} ; ra:commit {} ] .",
                lit(&merge.activity.agent.name),
                lit(&merge.activity.agent.version),
                lit(merge.activity.agent.commit.as_deref().unwrap_or(""))
            );
            if let Some(t) = &merge.activity.started_at {
                let _ = writeln!(out, "act:{act} prov:startedAtTime {}^^xsd:dateTime .", lit(t));
            }
            if let Some(t) = &merge.activity.ended_at {
                let _ = writeln!(out, "act:{act} prov:endedAtTime {}^^xsd:dateTime .", lit(t));
            }
            let _ = writeln!(out, "{} prov:wasGeneratedBy act:{act} .", iri(&merge.canonical));
            for mapping in &merge.mappings {
                let r = mapping.record;
                self.records.insert(r);
                let _ = writeln!(
                    out,
                    "act:{act} prov:used rec:{r} .\nrec:{r} ra:ruleId {} ; ra:ruleVersion {} ; ra:mappingSetId {} ; ra:mappingSetVersion {} ; ra:evidenceLocator {} ; ra:evidenceSha256 {} ; ra:mappingTool {} .",
                    lit(&mapping.rule_id),
                    lit(&mapping.rule_version),
                    lit(&mapping.mapping_set_id),
                    lit(&mapping.mapping_set_version),
                    lit(&mapping.evidence_locator),
                    lit(&mapping.evidence_sha256),
                    lit(&mapping.mapping_tool)
                );
                if !mapping.decision_id.is_empty() {
                    let decision = format!("<{}>", mapping.decision_id);
                    let assertion = format!("<{}>", mapping.assertion_id);
                    let _ = writeln!(
                        out,
                        "rec:{r} prov:wasDerivedFrom {decision} .\nact:{act} prov:used {decision} .\n{decision} a prov:Entity ; prov:wasDerivedFrom {assertion} ; ra:gateManifestSha256 {} .",
                        lit(&mapping.gate_manifest_sha256)
                    );
                }
            }
            for member in &merge.members {
                let id = iri(&member.id);
                let _ = writeln!(out, "{id} a prov:Entity .");
                for &r in &member.derived_from {
                    self.records.insert(r);
                    let _ = writeln!(
                        out,
                        "{id} prov:wasDerivedFrom rec:{r} ; prov:qualifiedDerivation [ a prov:Derivation ; prov:entity rec:{r} ; prov:hadActivity act:{act} ] ."
                    );
                }
            }
        }
    }

    fn prov_lines(out: &mut String, prov: &Provenance) {
        for e in &prov.entities {
            let _ = writeln!(
                out,
                "src:{} a prov:Entity ; dcterms:identifier {} ; ra:file {} ;",
                enc(&e.id),
                lit(&e.id),
                lit(&e.file)
            );
            if e.url.starts_with("http") {
                let _ = writeln!(out, "    prov:atLocation <{}> ;", e.url.replace(' ', "%20"));
            }
            if let Some(v) = &e.version {
                let _ = writeln!(out, "    dcterms:hasVersion {} ;", lit(v));
            }
            if let Some(t) = &e.retrieved_at {
                let _ = writeln!(out, "    ra:retrievedAt {} ;", lit(t));
            }
            if let Some(l) = &e.licence {
                let _ = writeln!(out, "    dcterms:license {} ;", lit(l));
            }
            let _ = writeln!(out, "    ra:sha256 {} .", lit(e.sha256.as_deref().unwrap_or("")));
        }
        for a in &prov.activities {
            let _ = write!(out, "act:{} a prov:Activity ; rdfs:label {}", enc(&a.id), lit(&a.label));
            if let Some(t) = &a.started_at {
                let _ = write!(out, " ;\n    prov:startedAtTime {}^^xsd:dateTime", lit(t));
            }
            if let Some(t) = &a.ended_at {
                let _ = write!(out, " ;\n    prov:endedAtTime {}^^xsd:dateTime", lit(t));
            }
            for &u in &a.used {
                let src = enc(&prov.entity(u).id);
                let _ = write!(
                    out,
                    " ;\n    prov:used src:{src} ;\n    prov:qualifiedUsage [ a prov:Usage ; prov:entity src:{src} ; prov:hadRole ra:sourceInput ]"
                );
            }
            let release = format!(
                "{}@{}",
                a.agent.name,
                a.agent.commit.as_deref().unwrap_or(&a.agent.version)
            );
            let _ = write!(
                out,
                " ;\n    prov:used <{ATLAS}software/{rel}> ;\n    prov:qualifiedUsage [ a prov:Usage ; prov:entity <{ATLAS}software/{rel}> ; prov:hadRole ra:softwareRelease ]",
                rel = enc(&release)
            );
            for (k, v) in &a.parameters {
                let _ = write!(
                    out,
                    " ;\n    ra:parameter [ rdfs:label {} ; prov:value {} ]",
                    lit(k),
                    lit(v)
                );
            }
            for (k, n) in &a.counts {
                let _ = write!(out, " ;\n    ra:count [ rdfs:label {} ; prov:value {n} ]", lit(k));
            }
            let _ = writeln!(
                out,
                " ;\n    prov:wasAssociatedWith [ a prov:SoftwareAgent ; rdfs:label {} ; ra:version {} ; ra:commit {} ] .",
                lit(&a.agent.name),
                lit(&a.agent.version),
                lit(a.agent.commit.as_deref().unwrap_or(""))
            );
            let _ = writeln!(
                out,
                "<{ATLAS}software/{}> a prov:Entity ; dcterms:identifier {} .",
                enc(&release),
                lit(&release)
            );
        }
    }
}

/// Chunks of Turtle, ~64 KB each.
impl Iterator for Export<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let mut out = String::with_capacity(1 << 16);
        loop {
            match self.stage {
                0 => {
                    out.push_str(PREFIXES);
                    let d = self.atlas.disease_at(self.d);
                    let _ = writeln!(out, "# Subgraph of {} ({}): {} edges", d.name, d.id, self.edges.len());
                    self.atlas_genes(&mut out);
                    self.stage = 1;
                }
                1 => {
                    while self.pos < self.edges.len() && out.len() < 1 << 16 {
                        let e = self.graph.edge(self.edges[self.pos]).clone();
                        self.edge(&mut out, &e);
                        self.pos += 1;
                    }
                    if self.pos == self.edges.len() {
                        self.stage = 2;
                    }
                    return Some(out);
                }
                2 => {
                    self.node_lines(&mut out);
                    self.identity_lines(&mut out);
                    self.record_lines(&mut out);
                    Self::prov_lines(&mut out, self.graph.provenance());
                    Self::prov_lines(&mut out, &self.atlas.provenance);
                    self.stage = 3;
                    return Some(out);
                }
                _ => return None,
            }
        }
    }
}
