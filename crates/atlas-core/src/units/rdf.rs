//! RDF 1.1 Turtle, deliberately compatible with serial SPARQL loaders.
//! Atlas vocabulary classes implement the application profile; no invented SEMUNIT ontology IRIs.
use super::{SemanticUnit, UnitCollection, UnitType};
use std::collections::BTreeSet;
use std::fmt::Write;

const BASE: &str = "https://w3id.org/rare-disease-atlas/";
pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
pub fn iri(id: &str) -> String {
    let url = if id.starts_with("unit:") {
        format!("{BASE}unit/{}", enc(id))
    } else if id.starts_with("MONDO:") || id.starts_with("HP:") || id.starts_with("GO:") {
        format!("http://purl.obolibrary.org/obo/{}", enc(&id.replace(':', "_")))
    } else if let Some(local) = id.strip_prefix("HGNC:") {
        format!("https://identifiers.org/hgnc:{}", enc(local))
    } else if let Some(local) = id.strip_prefix("NCBIGene:") {
        format!("https://identifiers.org/ncbigene:{}", enc(local))
    } else if let Some(local) = id.strip_prefix("PMID:") {
        format!("https://pubmed.ncbi.nlm.nih.gov/{}/", enc(local))
    } else if let Some(local) = id.strip_prefix("OMIM:") {
        format!("https://omim.org/entry/{}", enc(local))
    } else if let Some(local) = id.strip_prefix("ORPHA:") {
        format!("https://www.orpha.net/en/disease/detail/{}", enc(local))
    } else if let Some(local) = id.strip_prefix("ORCID:") {
        format!("https://orcid.org/{}", enc(local))
    } else if id.starts_with("NCT") {
        format!("https://clinicaltrials.gov/study/{}", enc(id))
    } else {
        format!("{BASE}id/{}", enc(id))
    };
    format!("<{url}>")
}
pub fn lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn property(out: &mut String, subject: &str, predicate: &str, object: &str) {
    // Relative atlas IRIs resolve through @base. Avoid repeating the same long namespace
    // hundreds of thousands of times (also keeps exports below the privacy body's cap).
    let _ = writeln!(out, "{} {} {} .", compact(subject), compact(predicate), compact(object));
}
fn compact(term: &str) -> std::borrow::Cow<'_, str> {
    match term.strip_prefix("<https://w3id.org/rare-disease-atlas/") {
        Some(local) if local.ends_with('>') => format!("<{local}").into(),
        _ => term.into(),
    }
}
fn time_property(out: &mut String, subject: &str, predicate: &str, value: &str) {
    if humantime::parse_rfc3339(value).is_ok() {
        property(
            out,
            subject,
            predicate,
            &format!("{}^^<http://www.w3.org/2001/XMLSchema#dateTime>", lit(value)),
        );
    } else {
        // Preserve malformed or partial upstream time strings without asserting an invalid PROV datatype.
        property(out, subject, "ra:sourceTimeText", &lit(value));
    }
}
fn unit(out: &mut String, u: &SemanticUnit, emitted: &mut BTreeSet<String>, standalone: bool) {
    let id = iri(&u.id);
    let class = match u.unit_type {
        UnitType::Statement => "ra:StatementUnit",
        UnitType::Item => "ra:ItemUnit",
        UnitType::Community => "ra:CommunityUnit",
        UnitType::MechanismGroup => "ra:MechanismItemGroupUnit",
    };
    property(out, &id, "a", &format!("prov:Entity, ra:SemanticUnit, {class}"));
    property(out, &id, "rdfs:label", &lit(&u.label));
    property(out, &id, "ra:hasSemanticUnitSubject", &iri(&u.subject.id));
    property(out, &id, "ra:status", &serde_json::to_string(&u.status).unwrap());
    property(out, &id, "ra:sha256", &lit(&u.sha256));
    property(out, &id, "dcterms:hasVersion", &lit(super::SCHEMA));
    property(out, &id, "prov:wasGeneratedBy", &iri(&u.generated_by));
    for member in &u.members {
        property(out, &id, "ra:hasMember", &iri(&member.id));
        if emitted.insert(format!("node:{}:{}", member.id, member.label)) {
            property(out, &iri(&member.id), "rdfs:label", &lit(&member.label));
            property(out, &iri(&member.id), "ra:nodeKind", &lit(member.kind.as_str()));
        }
    }
    for child in &u.children {
        property(out, &id, "ra:hasAssociatedSemanticUnit", &iri(child));
        property(out, &id, "prov:wasDerivedFrom", &iri(child));
    }
    for support in &u.support {
        let target = if support.starts_with("unit:") {
            iri(support)
        } else {
            format!("<{BASE}edge/{}>", enc(support))
        };
        property(out, &id, "prov:wasDerivedFrom", &target);
    }
    if let Some(relation) = &u.relation {
        property(out, &id, "ra:relation", &lit(relation));
        let predicate = format!("<{BASE}vocab#{}>", enc(relation));
        // Statement units own their proposition. Supporting membership propositions have their own units.
        if let Some(object) = u.members.iter().find(|n| {
            n.id != u.subject.id && (relation != "shares_pathway_with" || n.kind == crate::node::NodeKind::Gene)
        }) {
            property(out, &id, "a", "rdf:Statement");
            property(out, &id, "rdf:subject", &iri(&u.subject.id));
            property(out, &id, "rdf:predicate", &predicate);
            property(out, &id, "rdf:object", &iri(&object.id));
            // A hypothesis remains a statement about a proposition, never an asserted fact.
            if relation != "shares_pathway_with" && u.status != super::AssertionStatus::Hypothesis {
                property(out, &iri(&u.subject.id), &predicate, &iri(&object.id));
            }
            if relation == "shares_pathway_with" {
                for p in u.members.iter().filter(|n| n.kind == crate::node::NodeKind::Pathway) {
                    property(out, &id, "ra:viaPathway", &iri(&p.id));
                }
            }
        }
    }
    for evidence in u
        .evidence
        .iter()
        .filter(|_| standalone || u.unit_type == UnitType::Statement || u.children.is_empty())
        .chain(u.contacts.iter().flat_map(|c| c.evidence.iter()))
    {
        let key = super::digest(&serde_json::to_vec(evidence).unwrap());
        let record = format!("<{BASE}unit-record/{key}>");
        property(out, &id, "prov:wasDerivedFrom", &record);
        if !emitted.insert(key) {
            continue;
        }
        let source = format!("<{BASE}source/{}>", enc(&evidence.source.id));
        property(out, &record, "a", "prov:Entity");
        property(out, &record, "prov:wasDerivedFrom", &source);
        property(out, &record, "ra:locator", &lit(&evidence.locator));
        if let Some(hash) = &evidence.record_sha256 {
            property(out, &record, "ra:sha256", &lit(hash));
        }
        if let Some(url) = &evidence.record_url {
            property(out, &record, "ra:recordUrl", &lit(url));
        }
        if let Some(time) = &evidence.retrieved_at {
            time_property(out, &record, "prov:generatedAtTime", time);
        }
        if let Some(code) = &evidence.evidence_code {
            property(out, &record, "ra:evidenceCode", &lit(code));
        }
        if let Some(status) = &evidence.status {
            property(out, &record, "ra:status", &serde_json::to_string(status).unwrap());
        }
        if let Some(activity) = &evidence.upstream_activity {
            property(out, &record, "prov:wasGeneratedBy", &iri(activity));
            if emitted.insert(format!("activity-source:{activity}:{source}")) {
                property(out, &iri(activity), "a", "prov:Activity");
                property(out, &iri(activity), "prov:used", &source);
            }
        }
        if !emitted.insert(format!(
            "source:{}",
            super::digest(&serde_json::to_vec(&evidence.source).unwrap())
        )) {
            continue;
        }
        property(out, &source, "a", "prov:Entity");
        property(out, &source, "ra:sourceUrl", &lit(&evidence.source.url));
        property(out, &source, "ra:file", &lit(&evidence.source.file));
        if let Some(hash) = &evidence.source.sha256 {
            property(out, &source, "ra:sha256", &lit(hash));
        }
        if let Some(version) = &evidence.source.version {
            property(out, &source, "dcterms:hasVersion", &lit(version));
        }
        if let Some(licence) = &evidence.source.licence {
            property(out, &source, "dcterms:license", &lit(licence));
        }
        if let Some(time) = &evidence.source.retrieved_at {
            time_property(out, &source, "prov:generatedAtTime", time);
        }
        property(out, &iri(&u.generated_by), "prov:used", &source);
    }
    for contact in &u.contacts {
        // Contact URLs are literals: source strings cannot inject IRIs or Turtle syntax.
        property(out, &iri(&contact.node), "ra:contactRoute", &lit(&contact.url));
    }
    for resource in &u.resources {
        property(out, &iri(&resource.node.id), "ra:resourceRole", &lit(&resource.role));
    }
}

pub fn turtle(collection: &UnitCollection) -> String {
    let mut out = String::from(
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix prov: <http://www.w3.org/ns/prov#> .\n@prefix dcterms: <http://purl.org/dc/terms/> .\n@prefix ra: <https://w3id.org/rare-disease-atlas/vocab#> .\n\n",
    );
    let _ = writeln!(out, "@base <{BASE}> .\n");
    for activity in [&collection.activity, &collection.pathway_activity] {
        if activity.id.is_empty() {
            continue;
        }
        let id = iri(&activity.id);
        property(&mut out, &id, "a", "prov:Activity");
        property(&mut out, &id, "rdfs:label", &lit(&activity.label));
        if let Some(time) = &activity.started_at {
            time_property(&mut out, &id, "prov:startedAtTime", time);
        }
        if let Some(time) = &activity.ended_at {
            time_property(&mut out, &id, "prov:endedAtTime", time);
        }
        let agent = format!(
            "<{BASE}software/{}>",
            enc(&format!("{}:{}", activity.agent.name, activity.agent.version))
        );
        property(&mut out, &id, "prov:wasAssociatedWith", &agent);
        property(&mut out, &agent, "a", "prov:SoftwareAgent");
        property(&mut out, &agent, "rdfs:label", &lit(&activity.agent.name));
        property(&mut out, &agent, "dcterms:hasVersion", &lit(&activity.agent.version));
        for (name, value) in &activity.parameters {
            property(&mut out, &id, "ra:parameter", &lit(&format!("{name}={value}")));
        }
        for (name, value) in &activity.counts {
            property(&mut out, &id, "ra:count", &lit(&format!("{name}={value}")));
        }
    }
    let mut emitted = BTreeSet::new();
    for u in &collection.units {
        unit(&mut out, u, &mut emitted, collection.units.len() == 1);
    }
    for link in &collection.links {
        let id = iri(&link.id);
        property(&mut out, &id, "a", "prov:Entity, ra:UnitLink");
        property(&mut out, &id, "ra:fromUnit", &iri(&link.from));
        property(&mut out, &id, "ra:toUnit", &iri(&link.to));
        property(&mut out, &id, "ra:status", "\"inferred\"");
        property(&mut out, &id, "ra:sha256", &lit(&link.sha256));
        property(&mut out, &id, "prov:wasGeneratedBy", &iri(&link.generated_by));
        property(&mut out, &iri(&link.from), "ra:sharesPathwayGroup", &iri(&link.to));
        for support in &link.support {
            property(&mut out, &id, "prov:wasDerivedFrom", &iri(support));
        }
    }
    out
}
