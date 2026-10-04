//! Private operational RDF projection. Never a public release or licence certification.
use anyhow::Result;
use atlas_core::graph::{RecordWithhold, hex};
use atlas_core::node::NodeKind;
use atlas_core::provenance::{Provenance, RecordRef};
use atlas_core::{Atlas, Graph};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

const BASE: &str = "https://w3id.org/rare-disease-atlas/";
#[derive(Default, Debug, Serialize)]
pub struct Report {
    pub nodes: usize,
    pub edges: usize,
    pub hierarchy_assertions: usize,
    pub excluded_nodes: usize,
    pub excluded_edges: usize,
    pub records: usize,
    pub bytes: u64,
}
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn local(area: &str, id: &str) -> String {
    format!("<{BASE}{area}/{}>", enc(id))
}
fn lit(s: &str) -> String {
    serde_json::to_string(s).expect("string")
}
fn safe_url(s: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    // Keep meaningful source record locators while removing credential parameters.
    let sensitive = |key: &str| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "token" | "api_key" | "key" | "access_token" | "password" | "auth" | "signature"
        )
    };
    let pairs: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(key, _)| !sensitive(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if u.query().is_some() {
        u.set_query(None);
        if !pairs.is_empty() {
            u.query_pairs_mut().extend_pairs(pairs);
        }
    }
    // Fragments normally identify sections. Drop a fragment only when it carries
    // one of the same explicitly named credential parameters.
    if u.fragment().is_some_and(|f| {
        f.split('&')
            .any(|pair| pair.split_once('=').is_some_and(|(key, _)| sensitive(key)))
    }) {
        u.set_fragment(None);
    }
    Some(format!("<{}>", u.as_str()))
}
fn hidden(graph: &Graph, id: &str) -> bool {
    graph.node(id).is_some_and(|k| graph.node_withheld(k).is_some())
}
struct Writer<'a> {
    out: BufWriter<std::fs::File>,
    atlas: &'a Atlas,
    graph: &'a Graph,
    report: Report,
    nodes: BTreeSet<String>,
    records: BTreeSet<String>,
    sources: BTreeSet<(String, u16)>,
    activities: BTreeSet<(String, u16)>,
    node_activities: BTreeMap<String, String>,
    edge_activity: Option<(String, String)>,
}
impl Writer<'_> {
    fn node(&mut self, id: &str, label: &str, kind: NodeKind) -> Result<()> {
        if !self.nodes.insert(id.to_owned()) {
            return Ok(());
        }
        writeln!(
            self.out,
            "{} a ra:Node ; rdfs:label {} ; dcterms:identifier {} ; ra:nodeKind {} ; ra:exportStatus \"projected\" .",
            local("id", id),
            lit(label),
            lit(id),
            lit(kind.as_str())
        )?;
        if kind == NodeKind::Study {
            if let Some(key) = self.graph.node(id).filter(|key| key.kind == NodeKind::Study) {
                let status = &self.graph.study(key.idx).status;
                if !status.is_empty() {
                    writeln!(self.out, "{} ra:status {} .", local("id", id), lit(status))?;
                }
            }
        }
        if let Some(key) = self.graph.node(id) {
            let (countries, entity_kind): (Vec<&str>, Option<&str>) = match key.kind {
                NodeKind::Study => {
                    let s = self.graph.study(key.idx);
                    (s.countries.iter().map(String::as_str).collect(), Some(s.kind.as_str()))
                }
                NodeKind::Organisation => {
                    let o = self.graph.org(key.idx);
                    (o.country.as_deref().into_iter().collect(), Some(o.kind.as_str()))
                }
                NodeKind::Grant => (vec![self.graph.grant(key.idx).country.as_str()], None),
                NodeKind::Asset => {
                    let a = self.graph.asset(key.idx);
                    (a.fact("country").into_iter().collect(), Some(a.kind.as_str()))
                }
                _ => (vec![], None),
            };
            for country in countries
                .into_iter()
                .filter(|c| !c.trim().is_empty())
                .collect::<BTreeSet<_>>()
            {
                writeln!(self.out, "{} ra:country {} .", local("id", id), lit(country))?;
            }
            if let Some(entity_kind) = entity_kind.filter(|k| !k.trim().is_empty()) {
                writeln!(self.out, "{} ra:kind {} .", local("id", id), lit(entity_kind))?;
            }
        }
        self.report.nodes += 1;
        Ok(())
    }
    fn edge(&mut self, from: &str, relation: &str, to: &str, kind: &str) -> Result<String> {
        self.edge_activity = None;
        let id = atlas_core::node::edge_id(from, relation, to);
        let edge = local("edge", &id);
        let predicate = if relation == "subclass_of" {
            "http://www.w3.org/2000/01/rdf-schema#subClassOf".to_owned()
        } else {
            format!("{BASE}vocab#{relation}")
        };
        writeln!(
            self.out,
            "{} <{predicate}> {} ~ {edge} .\n{edge} a ra:Edge ; dcterms:identifier {} ; ra:edgeKind {} ; ra:exportStatus \"asserted\" .",
            local("id", from),
            local("id", to),
            lit(&id),
            lit(kind)
        )?;
        self.report.edges += 1;
        Ok(edge)
    }
    fn graph_record(&mut self, subject: &str, index: u32) -> Result<()> {
        let rec = self.graph.record(index);
        if let Some(index) = self
            .graph
            .provenance()
            .activities
            .iter()
            .position(|a| a.id.starts_with("activity:ingest-") && a.used.contains(&rec.entity))
        {
            self.generated(subject, "graph", index as u16)?;
        }
        let record = local("record", &format!("graph:{index}"));
        writeln!(self.out, "{subject} prov:wasDerivedFrom {record} .")?;
        self.derivation(subject, &record)?;
        if let Some(u) = rec.url.as_deref().and_then(safe_url) {
            writeln!(self.out, "{subject} dcterms:source {u} .")?;
        }
        if self.records.insert(record.clone()) {
            let source = local(
                "source",
                &format!("graph:{}", self.graph.provenance().entity(rec.entity).id),
            );
            writeln!(
                self.out,
                "{record} a prov:Entity ; dcterms:identifier {} ; ra:recordLocator {} ; ra:sha256 {} ; ra:hashScope {} ; dcterms:isPartOf {source} ; prov:wasDerivedFrom {source} .",
                lit(&rec.id),
                lit(&rec.locator.to_string()),
                lit(&hex(&rec.sha256)),
                lit(&serde_json::to_value(rec.hash)?.as_str().unwrap_or(""))
            )?;
            if let Some(t) = &rec.fetched_at {
                writeln!(self.out, "{record} ra:retrievedAt {} .", lit(t))?;
            }
            if let Some(version) = &self.graph.provenance().entity(rec.entity).version {
                writeln!(self.out, "{record} dcterms:hasVersion {} .", lit(version))?;
            }
            if let Some(u) = rec.url.as_deref().and_then(safe_url) {
                writeln!(self.out, "{record} dcterms:source {u} .")?;
            }
            self.sources.insert(("graph".into(), rec.entity.0));
            self.report.records += 1;
        }
        Ok(())
    }
    fn atlas_record(&mut self, subject: &str, rec: &RecordRef) -> Result<()> {
        let entity = self.atlas.provenance.entity(rec.entity);
        if let Some(index) = self
            .atlas
            .provenance
            .activities
            .iter()
            .position(|a| a.id.starts_with("activity:ingest-") && a.used.contains(&rec.entity))
        {
            self.generated(subject, "atlas", index as u16)?;
        }
        let record = local("record", &format!("atlas:{}:{}", rec.entity.0, rec.locator));
        let source = local("source", &format!("atlas:{}", entity.id));
        writeln!(self.out, "{subject} prov:wasDerivedFrom {record} .")?;
        self.derivation(subject, &record)?;
        if let Some(u) = safe_url(&entity.url) {
            writeln!(self.out, "{subject} dcterms:source {u} .")?;
        }
        if self.records.insert(record.clone()) {
            writeln!(
                self.out,
                "{record} a prov:Entity ; ra:recordLocator {} ; ra:hashScope \"input_file\" ; dcterms:isPartOf {source} ; prov:wasDerivedFrom {source} .",
                lit(&rec.locator.to_string())
            )?;
            if let Some(h) = &entity.sha256 {
                writeln!(self.out, "{record} ra:sha256 {} .", lit(h))?;
            }
            if let Some(v) = &entity.version {
                writeln!(self.out, "{record} dcterms:hasVersion {} .", lit(v))?;
            }
            if let Some(t) = &entity.retrieved_at {
                writeln!(self.out, "{record} ra:retrievedAt {} .", lit(t))?;
            }
            self.sources.insert(("atlas".into(), rec.entity.0));
            self.report.records += 1;
        }
        Ok(())
    }
    fn generated(&mut self, subject: &str, scope: &str, index: u16) -> Result<()> {
        let p = if scope == "atlas" {
            &self.atlas.provenance
        } else {
            self.graph.provenance()
        };
        let a = &p.activities[index as usize];
        let activity = local("activity", &format!("{scope}:{}", a.id));
        if subject.starts_with(&format!("<{BASE}edge/")) {
            if self.edge_activity.as_ref().is_none_or(|(s, _)| s != subject) {
                self.edge_activity = Some((subject.to_owned(), activity.clone()));
            }
        } else {
            self.node_activities
                .entry(subject.to_owned())
                .or_insert_with(|| activity.clone());
        }
        writeln!(self.out, "{subject} prov:wasGeneratedBy {} .", activity)?;
        self.activities.insert((scope.into(), index));
        Ok(())
    }
    fn derivation(&mut self, subject: &str, record: &str) -> Result<()> {
        let activity = self
            .edge_activity
            .as_ref()
            .filter(|(s, _)| s == subject)
            .map(|(_, a)| a)
            .or_else(|| self.node_activities.get(subject));
        if let Some(activity) = activity {
            let key = format!("{subject}\0{record}\0{activity}");
            let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
            let derivation = local("derivation", &hash);
            writeln!(
                self.out,
                "{subject} prov:qualifiedDerivation {derivation} .\n{derivation} a prov:Derivation ; prov:entity {record} ; prov:hadActivity {activity} ."
            )?;
        }
        Ok(())
    }
    fn provenance(&mut self) -> Result<()> {
        for (scope, index) in self.activities.clone() {
            let p: &Provenance = if scope == "atlas" {
                &self.atlas.provenance
            } else {
                self.graph.provenance()
            };
            let a = &p.activities[index as usize];
            let activity = local("activity", &format!("{scope}:{}", a.id));
            let agent = local(
                "software",
                &format!(
                    "{scope}:{}@{}@{}",
                    a.agent.name,
                    a.agent.version,
                    a.agent.commit.as_deref().unwrap_or("")
                ),
            );
            writeln!(
                self.out,
                "{activity} a prov:Activity ; dcterms:identifier {} ; prov:wasAssociatedWith {agent} .\n{agent} a prov:SoftwareAgent ; rdfs:label {} ; dcterms:hasVersion {} .",
                lit(&a.id),
                lit(&a.agent.name),
                lit(&a.agent.version)
            )?;
            if let Some(c) = &a.agent.commit {
                writeln!(self.out, "{agent} ra:commit {} .", lit(c))?;
            }
            if let Some(t) = &a.started_at {
                writeln!(self.out, "{activity} prov:startedAtTime {} .", lit(t))?;
            }
            if let Some(t) = &a.ended_at {
                writeln!(self.out, "{activity} prov:endedAtTime {} .", lit(t))?;
            }
            // Only actually retained sources: no suppressed input source metadata.
            for e in &a.used {
                if self.sources.contains(&(scope.clone(), e.0)) {
                    writeln!(
                        self.out,
                        "{activity} prov:used {} .",
                        local("source", &format!("{scope}:{}", p.entity(*e).id))
                    )?;
                }
            }
        }
        for (scope, index) in &self.sources {
            let p = if scope == "atlas" {
                &self.atlas.provenance
            } else {
                self.graph.provenance()
            };
            let e = &p.entities[*index as usize];
            let source = local("source", &format!("{scope}:{}", e.id));
            writeln!(self.out, "{source} a prov:Entity ; ra:bytes {} .", e.bytes)?;
            if let Some(u) = safe_url(&e.url) {
                writeln!(self.out, "{source} dcterms:source {u} .")?;
            }
            for (pred, value) in [
                ("ra:sha256", &e.sha256),
                ("ra:retrievedAt", &e.retrieved_at),
                ("dcterms:hasVersion", &e.version),
                ("dcterms:license", &e.licence),
            ] {
                if let Some(value) = value {
                    writeln!(self.out, "{source} {pred} {} .", lit(value))?;
                }
            }
        }
        Ok(())
    }
}

/// Create a new private projection; never overwrite an existing store input.
pub fn write(atlas: &Atlas, graph: &Graph, path: &Path) -> Result<Report> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut w = Writer {
        out: BufWriter::with_capacity(128 * 1024, file),
        atlas,
        graph,
        report: Report::default(),
        nodes: BTreeSet::new(),
        records: BTreeSet::new(),
        sources: BTreeSet::new(),
        activities: BTreeSet::new(),
        node_activities: BTreeMap::new(),
        edge_activity: None,
    };
    writeln!(
        w.out,
        "VERSION \"1.2\"\n@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix prov: <http://www.w3.org/ns/prov#> .\n@prefix dcterms: <http://purl.org/dc/terms/> .\n@prefix ra: <{BASE}vocab#> .\n# Private operational metadata: exact retained Study/Organisation/Grant country, Asset country fact; exact Study/Organisation/Asset kind.\nra:country a rdf:Property .\nra:kind a rdf:Property ."
    )?;
    let data = graph.data();
    for id in data
        .studies
        .iter()
        .map(|n| n.id.as_str())
        .chain(data.grants.iter().map(|n| n.id.as_str()))
        .chain(data.papers.iter().map(|n| n.id.as_str()))
        .chain(data.people.iter().map(|n| n.id.as_str()))
        .chain(data.orgs.iter().map(|n| n.id.as_str()))
        .chain(data.assets.iter().map(|n| n.id.as_str()))
    {
        let Some(key) = graph.node(id) else { continue };
        if graph.node_withheld(key).is_some() {
            w.report.excluded_nodes += 1;
            continue;
        }
        let node = graph.node_ref(key);
        w.node(id, &node.label, node.kind)?;
        for &r in graph.node_records(key) {
            w.graph_record(&local("id", id), r)?;
        }
    }
    for d in atlas.diseases() {
        if hidden(graph, &d.id) {
            w.report.excluded_nodes += 1;
            continue;
        }
        w.node(&d.id, &d.name, NodeKind::Disease)?;
        let subject = local("id", &d.id);
        w.generated(&subject, "atlas", d.generated_by.0)?;
        for r in &d.derived_from {
            w.atlas_record(&subject, r)?;
        }
        // Direct MONDO is_a assertions only. Keep the ontology record separate from
        // unrelated clinical annotations, and never propagate associations here.
        if let Some(mondo) = atlas.provenance.entity_by_file("mondo.obo") {
            let refs: Vec<_> = d.derived_from.iter().filter(|r| r.entity == mondo).collect();
            if !refs.is_empty() {
                for parent in &d.parents {
                    if hidden(graph, parent) {
                        w.report.excluded_edges += 1;
                        continue;
                    }
                    if let Some(node) = crate::nodes::any_ref(atlas, graph, parent) {
                        w.node(parent, &node.label, node.kind)?;
                    }
                    let edge = w.edge(&d.id, "subclass_of", parent, "observed")?;
                    w.generated(&edge, "atlas", d.generated_by.0)?;
                    for r in &refs {
                        w.atlas_record(&edge, r)?;
                    }
                    w.report.hierarchy_assertions += 1;
                }
            }
        }
        for e in atlas_journeys::gene_edges(atlas, &d.id, &d.genes) {
            if hidden(graph, &e.gene_id) {
                w.report.excluded_edges += 1;
                continue;
            }
            w.node(&e.gene_id, e.symbol, NodeKind::Gene)?;
            let edge = w.edge(&d.id, atlas_journeys::GENE_RELATION, &e.gene_id, "observed")?;
            for l in e.links {
                w.atlas_record(&edge, &l.record)?;
                w.atlas_record(&local("id", &e.gene_id), &l.record)?;
            }
        }
        for (absent, list) in [(false, &d.phenotypes), (true, &d.excluded)] {
            for e in list {
                let t = atlas.hpo.term(e.term);
                if hidden(graph, &t.id) {
                    w.report.excluded_edges += 1;
                    continue;
                }
                w.node(&t.id, &t.name, NodeKind::Phenotype)?;
                let edge = w.edge(&d.id, atlas_journeys::phenotype_relation(absent), &t.id, "observed")?;
                for a in &e.annotations {
                    w.atlas_record(&edge, &a.record)?;
                    w.atlas_record(&local("id", &t.id), &a.record)?;
                }
            }
        }
    }
    for g in atlas.genes() {
        if !hidden(graph, g.id()) {
            w.node(g.id(), &g.symbol, NodeKind::Gene)?;
        }
    }
    for t in atlas.hpo.terms() {
        if !hidden(graph, &t.id) {
            w.node(&t.id, &t.name, NodeKind::Phenotype)?;
            if let Some(hp) = atlas.provenance.entity_by_file("hp.obo") {
                w.atlas_record(&local("id", &t.id), &RecordRef::record(hp, &t.id))?;
                for parent in &t.parents {
                    if hidden(graph, parent) {
                        w.report.excluded_edges += 1;
                        continue;
                    }
                    let edge = w.edge(&t.id, "subclass_of", parent, "observed")?;
                    w.atlas_record(&edge, &RecordRef::record(hp, &t.id))?;
                    w.report.hierarchy_assertions += 1;
                }
            }
        }
    }
    for e in graph.edges() {
        if graph.records_withheld(&e.records).is_some() || hidden(graph, &e.from) || hidden(graph, &e.to) {
            w.report.excluded_edges += 1;
            continue;
        }
        for id in [&e.from, &e.to] {
            if let Some(n) = crate::nodes::any_ref(atlas, graph, id) {
                w.node(id, &n.label, n.kind)?;
            }
        }
        let edge = w.edge(&e.from, e.relation.as_str(), &e.to, e.kind.as_str())?;
        w.generated(&edge, "graph", e.activity.0)?;
        for &r in &e.records {
            w.graph_record(&edge, r)?;
        }
    }
    w.provenance()?;
    w.out.flush()?;
    w.out.get_ref().sync_all()?;
    w.report.bytes = w.out.get_ref().metadata()?.len();
    Ok(w.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_literals_and_release_identifiers() {
        assert_eq!(
            local("id", "HGNC:1>"),
            "<https://w3id.org/rare-disease-atlas/id/HGNC%3A1%3E>"
        );
        assert_eq!(lit("a\n\"b"), "\"a\\n\\\"b\"");
        assert!(safe_url("https://user:secret@example.com/").is_none());
        assert_eq!(
            safe_url("https://example.org/record?token=secret").as_deref(),
            Some("<https://example.org/record>")
        );
        assert_eq!(
            safe_url("https://example.org/record?id=record123&token=secret#evidence").as_deref(),
            Some("<https://example.org/record?id=record123#evidence>")
        );
        assert_eq!(
            safe_url("https://example.org/record?id=record123#access_token=secret").as_deref(),
            Some("<https://example.org/record?id=record123>")
        );
    }
    #[test]
    fn projection_preserves_kind_provenance_and_withholding() -> Result<()> {
        use atlas_core::graph::{
            GraphData, GraphEdge, LinkLevel, Paper, Quarantine, RecordHash, Relation, SourceRecord,
        };
        use atlas_core::node::EdgeKind;
        use atlas_core::provenance::{Activity, ActivityIdx, EntityIdx, Locator, SourceEntity};
        let prov = Provenance {
            entities: vec![SourceEntity {
                id: "source:fixture".into(),
                file: "mondo.obo".into(),
                url: "https://example.org/source?token=PRIVATE_TOKEN".into(),
                sha256: Some("actual-fixture-source-hash".into()),
                version: Some("fixture-v1".into()),
                ..Default::default()
            }],
            activities: vec![
                Activity {
                    id: "activity:ingest-fixture".into(),
                    used: vec![EntityIdx(0)],
                    parameters: std::collections::BTreeMap::from([("raw_letter".into(), "PRIVATE_LETTER".into())]),
                    ..Default::default()
                },
                Activity {
                    id: "activity:compute-fixture".into(),
                    ..Default::default()
                },
            ],
        };
        let mut disease = atlas_core::Disease::new("MONDO:1", ActivityIdx(0));
        disease.name = "Known condition".into();
        disease.parents = vec!["MONDO:2".into()];
        disease.derived_from = vec![RecordRef::record(EntityIdx(0), "MONDO:1")];
        let atlas = Atlas::new(vec![], Default::default(), prov.clone(), vec![disease]);
        let record = |id: &str| SourceRecord {
            entity: EntityIdx(0),
            locator: Locator::Line(1),
            id: id.into(),
            url: Some("https://example.org/record?token=PRIVATE_TOKEN".into()),
            fetched_at: Some("2026-10-04T00:00:00Z".into()),
            hash: RecordHash::JsonLine,
            sha256: [1; 32],
        };
        let paper = |id: &str, title: &str, r: u32| Paper {
            id: id.into(),
            title: title.into(),
            journal: String::new(),
            year: None,
            doi: None,
            review: false,
            records: vec![r],
        };
        let edge = |from: &str, kind: EdgeKind, r: u32| GraphEdge {
            from: from.into(),
            relation: Relation::AboutCondition,
            to: "MONDO:1".into(),
            kind,
            level: LinkLevel::Text,
            reason: "PRIVATE_REASON".into(),
            activity: ActivityIdx(if kind == EdgeKind::Inferred { 1 } else { 0 }),
            records: vec![r],
        };
        let graph = Graph::new(GraphData {
            provenance: prov,
            records: vec![record("PMID:1"), record("PMID:2")],
            studies: vec![atlas_core::graph::Study {
                id: "NCT00000001".into(),
                title: "Actual recruiting study".into(),
                status: "RECRUITING".into(),
                kind: atlas_core::graph::StudyKind::Trial,
                phases: vec![],
                sponsor: String::new(),
                sponsor_class: String::new(),
                start: String::new(),
                completion: String::new(),
                enrollment: None,
                countries: vec!["Germany".into(), "".into()],
                interventions: vec![],
                record: 0,
            }],
            papers: vec![
                paper("PMID:1", "Visible paper", 0),
                paper("PMID:2", "WITHHELD_NAME", 1),
                paper("PMID:3", "Computed paper", 0),
            ],
            edges: vec![
                edge("PMID:1", EdgeKind::Observed, 0),
                edge("PMID:3", EdgeKind::Inferred, 0),
                edge("PMID:2", EdgeKind::Observed, 1),
            ],
            quarantine: vec![Quarantine {
                record: 1,
                reason: "fixture quarantine".into(),
            }],
            ..Default::default()
        });
        let path = std::env::temp_dir().join(format!(
            "atlas-private-projection-{}-{}.ttl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let report = write(&atlas, &graph, &path)?;
        let rdf = std::fs::read_to_string(&path)?;
        std::fs::remove_file(&path)?;
        assert_eq!(report.excluded_nodes, 1);
        assert_eq!(report.excluded_edges, 1);
        assert_eq!(report.edges, 3);
        assert_eq!(report.hierarchy_assertions, 1);
        assert!(rdf.contains(
            "<http://www.w3.org/2000/01/rdf-schema#subClassOf> <https://w3id.org/rare-disease-atlas/id/MONDO%3A2>"
        ));
        assert!(rdf.contains("ra:recordLocator \"MONDO:1\""));
        assert!(rdf.contains("a ra:Node ;"));
        assert!(!rdf.contains("schema:"));
        for (prefix, iri) in [
            ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
            ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
            ("prov", "http://www.w3.org/ns/prov#"),
            ("dcterms", "http://purl.org/dc/terms/"),
            ("ra", "https://w3id.org/rare-disease-atlas/vocab#"),
        ] {
            assert!(rdf.contains(&format!("@prefix {prefix}: <{iri}> .")));
        }
        assert!(rdf.contains("ra:exportStatus \"projected\""));
        assert!(rdf.contains("ra:exportStatus \"asserted\""));
        assert!(rdf.contains("ra:edgeKind \"observed\""));
        assert!(rdf.contains("ra:edgeKind \"inferred\""));
        assert!(rdf.contains("ra:recordLocator \"L1\""));
        assert!(rdf.contains("prov:wasGeneratedBy"));
        assert!(rdf.contains("prov:qualifiedDerivation"));
        assert!(rdf.contains(
            "prov:hadActivity <https://w3id.org/rare-disease-atlas/activity/graph%3Aactivity%3Acompute-fixture>"
        ));
        assert!(rdf.contains("dcterms:hasVersion \"fixture-v1\""));
        assert!(rdf.contains("<https://w3id.org/rare-disease-atlas/id/NCT00000001> ra:status \"RECRUITING\" ."));
        assert!(rdf.contains("<https://w3id.org/rare-disease-atlas/id/NCT00000001> ra:country \"Germany\" ."));
        assert!(rdf.contains("<https://w3id.org/rare-disease-atlas/id/NCT00000001> ra:kind \"trial\" ."));
        assert!(!rdf.contains("ra:country \"\""));
        assert!(rdf.contains("ra:country a rdf:Property ."));
        assert!(rdf.contains("ra:kind a rdf:Property ."));
        assert!(rdf.contains("ra:sha256"));
        for private in [
            "WITHHELD_NAME",
            "PMID%3A2",
            "PRIVATE_TOKEN",
            "PRIVATE_LETTER",
            "PRIVATE_REASON",
        ] {
            assert!(!rdf.contains(private), "leaked {private}");
        }
        Ok(())
    }
}
