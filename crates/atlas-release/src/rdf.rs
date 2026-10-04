//! RDF 1.2 assertions and explicit, independently citable provenance entities.
use crate::{Citation, ReleaseEdge, ReleaseNode, digest, personal_reference, policy, public_url};
use anyhow::Result;
use atlas_core::provenance::Provenance;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufWriter, Write};

const BASE: &str = "https://w3id.org/rare-disease-atlas/";
pub struct Writer {
    out: BufWriter<File>,
    records: BTreeSet<String>,
}
impl Writer {
    pub fn new(file: File) -> Self {
        Self {
            out: BufWriter::new(file),
            records: BTreeSet::new(),
        }
    }
    pub fn flush(&mut self) -> Result<()> {
        self.out.flush()?;
        Ok(())
    }
}

pub(crate) fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn local(area: &str, id: &str) -> String {
    format!("<{BASE}{area}/{}>", enc(id))
}
fn lit(s: &str) -> String {
    serde_json::to_string(s).expect("string serialises")
}
fn iri(s: &str) -> String {
    let mut out = String::from("<");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b":/?#[]@!$&'()*+,;=%-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.push('>');
    out
}
fn licence(s: &str) -> String {
    if s.starts_with("https://creativecommons.org/") {
        iri(s)
    } else {
        lit(s)
    }
}
fn record_id(r: &Citation) -> String {
    local(
        "record",
        &digest(format!("{}\0{}\0{}", r.source_entity, r.record_locator, r.sha256).as_bytes()),
    )
}

pub fn header(w: &mut Writer) -> Result<()> {
    writeln!(
        w.out,
        "VERSION \"1.2\"\n@prefix prov: <http://www.w3.org/ns/prov#> .\n@prefix dcterms: <http://purl.org/dc/terms/> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix ra: <{BASE}vocab#> .\n"
    )?;
    Ok(())
}

fn references(w: &mut Writer, subject: &str, refs: &[Citation], activities: &[String]) -> Result<()> {
    for r in refs {
        let record = record_id(r);
        writeln!(
            w.out,
            "{subject} dcterms:source {} ; dcterms:license {} ; prov:wasDerivedFrom {record} .",
            iri(r.record_url.as_deref().unwrap_or(&r.source_url)),
            licence(&r.license)
        )?;
        writeln!(
            w.out,
            "{subject} prov:qualifiedDerivation [ a prov:Derivation ; prov:entity {record}{} ] .",
            activities
                .first()
                .map(|a| format!(" ; prov:hadActivity {}", local("activity", a)))
                .unwrap_or_default()
        )?;
        if w.records.insert(record.clone()) {
            writeln!(
                w.out,
                "{record} a prov:Entity ; dcterms:source {} ; dcterms:license {} ; dcterms:isPartOf {} ;\n  ra:recordLocator {} ; ra:sha256 {} ; ra:hashScope {} ; dcterms:hasVersion {} ; ra:retrievalBasis {} .",
                iri(r.record_url.as_deref().unwrap_or(&r.source_url)),
                licence(&r.license),
                local("source", &r.source_entity),
                lit(&r.record_locator),
                lit(&r.sha256),
                lit(&r.hash_scope),
                lit(&r.version),
                lit(&r.retrieval_basis)
            )?;
            if let Some(t) = &r.retrieved_at {
                writeln!(w.out, "{record} ra:retrievedAt {} .", lit(t))?;
            }
            writeln!(
                w.out,
                "{record} ra:sourceUrlBasis {} ; ra:sourceHashScope {} .",
                lit(&r.source_url_basis),
                lit(&r.source_hash_scope)
            )?;
        }
    }
    for a in activities {
        writeln!(w.out, "{subject} prov:wasGeneratedBy {} .", local("activity", a))?;
    }
    Ok(())
}

pub fn node(w: &mut Writer, n: &ReleaseNode) -> Result<()> {
    let subject = local("id", &n.id);
    writeln!(
        w.out,
        "{subject} a ra:Node ; dcterms:identifier {} ; ra:nodeKind {} ; ra:exportStatus {} .",
        lit(&n.id),
        lit(&n.kind),
        lit(&n.export_status)
    )?;
    if let Some(label) = &n.label {
        writeln!(w.out, "{subject} rdfs:label {} .", lit(label))?;
    }
    if let Some(status) = &n.status {
        writeln!(w.out, "{subject} ra:status {} .", lit(status))?;
    }
    writeln!(
        w.out,
        "{subject} prov:wasGeneratedBy {} .",
        local("activity", "release:projection")
    )?;
    references(w, &subject, &n.provenance, &[])
}

pub fn edge(w: &mut Writer, e: &ReleaseEdge) -> Result<()> {
    let subject = local("edge", &e.id);
    if let (Some(from), Some(relation), Some(to)) = (&e.from, &e.relation, &e.to) {
        // The '~' identifier denotes the RDF 1.2 reifier for this exact asserted triple.
        writeln!(
            w.out,
            "{} {} {} ~ {subject} .",
            local("id", from),
            if relation == "subclass_of" {
                iri("http://www.w3.org/2000/01/rdf-schema#subClassOf")
            } else {
                iri(&format!("{BASE}vocab#{relation}"))
            },
            local("id", to)
        )?;
        writeln!(w.out, "{subject} ra:edgeKind {} .", lit(e.kind.as_deref().unwrap()))?;
    }
    writeln!(
        w.out,
        "{subject} a ra:Edge ; dcterms:identifier {} ; ra:exportStatus {} .",
        lit(&e.id),
        lit(&e.export_status)
    )?;
    writeln!(
        w.out,
        "{subject} prov:wasGeneratedBy {} .",
        local("activity", "release:projection")
    )?;
    references(w, &subject, &e.provenance, &e.activities)
}

/// The same licensed exact rows as the SSSOM file, with independently citable reifiers.
/// Alias IRIs remain identifiers, not new named biological nodes or label-based merges.
pub fn mapping(w: &mut Writer, from: &str, to: &str, refs: &[Citation]) -> Result<()> {
    let subject = local("mapping", &digest(format!("{from}|skos:exactMatch|{to}").as_bytes()));
    writeln!(
        w.out,
        "{} <http://www.w3.org/2004/02/skos/core#exactMatch> {} ~ {subject} .",
        local("id", from),
        local("id", to)
    )?;
    writeln!(w.out, "{subject} a ra:Mapping ; ra:exportStatus \"asserted\" .")?;
    references(w, &subject, refs, &["release:projection".into()])
}

pub fn provenance(
    w: &mut Writer,
    prov: &Provenance,
    namespace: &str,
    sources: &BTreeSet<String>,
    activities: &BTreeSet<String>,
) -> Result<()> {
    for e in &prov.entities {
        if !sources.contains(&format!("{namespace}:{}", e.id)) {
            continue;
        }
        let subject = local("source", &format!("{namespace}:{}", e.id));
        let p = policy::for_entity(e);
        writeln!(
            w.out,
            "{subject} a prov:Entity ; dcterms:identifier {} ; ra:sha256 {} ; ra:bytes {} ; dcterms:license {} ; ra:copyFields {} ; ra:policyCheckedOn {} ; ra:termsPage {} .",
            lit(&e.file),
            lit(e.sha256.as_deref().unwrap_or("")),
            e.bytes,
            licence(p.license),
            p.copy_fields,
            lit(p.checked_on),
            iri(p.terms_url)
        )?;
        if let Some(url) = public_url(&e.url) {
            writeln!(w.out, "{subject} dcterms:source {} .", iri(&url))?;
        }
        writeln!(
            w.out,
            "{subject} ra:originalUrlSha256 {} .",
            lit(&digest(e.url.as_bytes()))
        )?;
        if let Some(t) = &e.retrieved_at {
            writeln!(w.out, "{subject} ra:retrievedAt {} .", lit(t))?;
        }
        writeln!(
            w.out,
            "{subject} dcterms:hasVersion {} .",
            lit(&e
                .version
                .clone()
                .unwrap_or_else(|| format!("snapshot-sha256:{}", e.sha256.as_deref().unwrap_or(""))))
        )?;
    }
    for a in &prov.activities {
        let id = format!("{namespace}:{}", a.id);
        if !activities.contains(&id)
            && !a
                .used
                .iter()
                .any(|idx| sources.contains(&format!("{namespace}:{}", prov.entity(*idx).id)))
        {
            continue;
        }
        // Activities involving the person cache are outside the release lineage.
        if a.used.iter().any(|idx| personal_reference(&prov.entity(*idx).file)) {
            continue;
        }
        let subject = local("activity", &format!("{namespace}:{}", a.id));
        let agent_id = format!(
            "{}@{}@{}",
            a.agent.name,
            a.agent.version,
            a.agent.commit.as_deref().unwrap_or("unrecorded")
        );
        let agent = local("software", &agent_id);
        writeln!(
            w.out,
            "{subject} a prov:Activity ; dcterms:identifier {} ; prov:wasAssociatedWith {agent} ; ra:parametersSha256 {} .",
            lit(&a.id),
            lit(&digest(&serde_json::to_vec(&a.parameters)?))
        )?;
        writeln!(
            w.out,
            "{agent} a prov:SoftwareAgent ; dcterms:identifier {} ; dcterms:hasVersion {} ; ra:commit {} .",
            lit(&a.agent.name),
            lit(&a.agent.version),
            lit(a.agent.commit.as_deref().unwrap_or("unrecorded"))
        )?;
        if let Some(t) = &a.started_at {
            writeln!(w.out, "{subject} prov:startedAtTime {} .", lit(t))?;
        }
        if let Some(t) = &a.ended_at {
            writeln!(w.out, "{subject} prov:endedAtTime {} .", lit(t))?;
        }
        for idx in &a.used {
            if !sources.contains(&format!("{namespace}:{}", prov.entity(*idx).id)) {
                continue;
            }
            let source = local("source", &format!("{namespace}:{}", prov.entity(*idx).id));
            writeln!(
                w.out,
                "{subject} prov:used {source} ; prov:qualifiedUsage [ a prov:Usage ; prov:entity {source} ; prov:hadRole ra:sourceInput ] ."
            )?;
        }
        // Count keys may contain record IDs/names. Preserve their digest rather than arbitrary text.
        writeln!(
            w.out,
            "{subject} ra:countsSha256 {} .",
            lit(&digest(&serde_json::to_vec(&a.counts)?))
        )?;
    }
    Ok(())
}

/// Each original identifier remains a provenance entity after an identity merge.
pub fn identity_merge(
    w: &mut Writer,
    merge: &atlas_core::graph::IdentityMerge,
    graph: &atlas_core::Graph,
) -> Result<()> {
    let a = &merge.activity;
    let activity = local("activity", &format!("graph:{}", a.id));
    let agent = local("software", &format!("{}@{}", a.agent.name, a.agent.version));
    writeln!(
        w.out,
        "{activity} a prov:Activity ; rdfs:label \"identity merge\" ; prov:wasAssociatedWith {agent} ."
    )?;
    writeln!(
        w.out,
        "{agent} a prov:SoftwareAgent ; dcterms:identifier {} ; dcterms:hasVersion {} .",
        lit(&a.agent.name),
        lit(&a.agent.version)
    )?;
    if let Some(t) = &a.started_at {
        writeln!(w.out, "{activity} prov:startedAtTime {} .", lit(t))?;
    }
    if let Some(t) = &a.ended_at {
        writeln!(w.out, "{activity} prov:endedAtTime {} .", lit(t))?;
    }
    writeln!(
        w.out,
        "{} prov:wasGeneratedBy {activity} .",
        local("id", &merge.canonical)
    )?;
    for mapping in &merge.mappings {
        let rec = graph.record(mapping.record);
        let refs = crate::graph_refs(graph, &[mapping.record]);
        // Exact SSSOM rows are inspectable even when restricted, but their relationships
        // are asserted only when the same source policy grants release permission.
        if crate::may_copy(&refs) {
            self::mapping(w, &mapping.subject, &mapping.object, &refs)?;
        }
        let row = local("mapping-row", &atlas_core::graph::hex(&rec.sha256));
        let upstream = local(
            "evidence",
            &digest(format!("{}\0{}", mapping.evidence_sha256, mapping.evidence_locator).as_bytes()),
        );
        let physical = record_id(&crate::graph_refs(graph, &[mapping.record])[0]);
        writeln!(
            w.out,
            "{row} prov:specializationOf {physical} ; prov:wasDerivedFrom {upstream} .\n{physical} prov:wasDerivedFrom {upstream} .\n{upstream} a prov:Entity ; ra:sha256 {} ; ra:recordLocator {} ; ra:hashScope \"input_file\" .",
            lit(&mapping.evidence_sha256),
            lit(&mapping.evidence_locator)
        )?;
        if !mapping.decision_id.is_empty() {
            let decision = iri(&mapping.decision_id);
            let assertion = iri(&mapping.assertion_id);
            writeln!(
                w.out,
                "{row} prov:wasDerivedFrom {decision} .\n{activity} prov:used {decision} .\n{decision} a prov:Entity ; prov:wasDerivedFrom {assertion} ; ra:gateManifestSha256 {} .\n{assertion} a prov:Entity ; prov:wasDerivedFrom {upstream} .",
                lit(&mapping.gate_manifest_sha256)
            )?;
        }
        if let Some(url) = public_url(&mapping.evidence_url) {
            writeln!(w.out, "{upstream} dcterms:source {} .", iri(&url))?;
        }
        writeln!(w.out, "{activity} prov:used {row} .")?;
        writeln!(
            w.out,
            "{row} a prov:Entity ; ra:ruleId {} ; ra:ruleVersion {} ; ra:mappingSetId {} ; ra:mappingSetVersion {} ; ra:sha256 {} ; ra:recordLocator {} ; ra:evidenceSha256 {} ; ra:evidenceLocator {} ; ra:mappingTool {} .",
            lit(&mapping.rule_id),
            lit(&mapping.rule_version),
            lit(&mapping.mapping_set_id),
            lit(&mapping.mapping_set_version),
            lit(&atlas_core::graph::hex(&rec.sha256)),
            lit(&rec.locator.to_string()),
            lit(&mapping.evidence_sha256),
            lit(&mapping.evidence_locator),
            lit(&mapping.mapping_tool)
        )?;
        if let Some(url) = public_url(&mapping.evidence_url) {
            writeln!(w.out, "{row} dcterms:source {} .", iri(&url))?;
        }
    }
    for member in &merge.members {
        let subject = local("id", &member.id);
        writeln!(w.out, "{subject} a prov:Entity .")?;
        for &idx in &member.derived_from {
            let refs = crate::graph_refs(graph, &[idx]);
            references(w, &subject, &refs, &[format!("graph:{}", a.id)])?;
        }
    }
    Ok(())
}

pub fn release_activity(
    w: &mut Writer,
    atlas: &Provenance,
    graph: &Provenance,
    version: &str,
    sources: &BTreeSet<String>,
) -> Result<()> {
    let activity = local("activity", "release:projection");
    let agent = local("software", &format!("atlas-release@{}", env!("CARGO_PKG_VERSION")));
    let policy = local("source", "release:licence-policy");
    writeln!(
        w.out,
        "{activity} a prov:Activity ; prov:wasAssociatedWith {agent} ; dcterms:hasVersion {} ; prov:used {policy} ; ra:policyRule \"strictest-source; field-whitelist; restricted-identifiers-only\" .",
        lit(version)
    )?;
    writeln!(
        w.out,
        "{agent} a prov:SoftwareAgent ; dcterms:identifier \"atlas-release\" ; dcterms:hasVersion {} .",
        lit(env!("CARGO_PKG_VERSION"))
    )?;
    writeln!(
        w.out,
        "{policy} a prov:Entity ; dcterms:identifier \"docs/release/LICENCES.md\" ; ra:sha256 {} ; ra:retrievedAt {} ; dcterms:hasVersion {} .",
        lit(&digest(include_bytes!("../../../docs/release/LICENCES.md"))),
        lit(policy::CHECKED),
        lit(env!("CARGO_PKG_VERSION"))
    )?;
    for (namespace, prov) in [("atlas", atlas), ("graph", graph)] {
        for e in &prov.entities {
            if !sources.contains(&format!("{namespace}:{}", e.id)) {
                continue;
            }
            let source = local("source", &format!("{namespace}:{}", e.id));
            writeln!(
                w.out,
                "{activity} prov:used {source} ; prov:qualifiedUsage [ a prov:Usage ; prov:entity {source} ; prov:hadRole ra:sourceInput ] ."
            )?;
        }
    }
    Ok(())
}

/// A statement using a canonical endpoint depends on that endpoint's accepted cluster.
/// This conservative dependency permits complete retraction after any cluster split.
pub fn identity_statement(w: &mut Writer, edge: &str, merge: &atlas_core::graph::IdentityMerge) -> Result<()> {
    let statement = local("edge", edge);
    let activity = local("activity", &format!("graph:{}", merge.activity.id));
    for mapping in &merge.mappings {
        if mapping.decision_id.is_empty() {
            continue;
        }
        let decision = iri(&mapping.decision_id);
        writeln!(
            w.out,
            "{statement} prov:wasDerivedFrom {decision} ; prov:qualifiedDerivation [ a prov:Derivation ; prov:entity {decision} ; prov:hadActivity {activity} ] ."
        )?;
    }
    Ok(())
}
