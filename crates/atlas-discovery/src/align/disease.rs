//! Diseases: MONDO's own SSSOM (exact/broad), MONDO OBO annotations (obsolete equivalents, GARD) and
//! Orphanet's own mappings (E / NTBT / BTNT / ND), cross-checked against each other.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::json;

use super::*;

/// Prefixes in scope (brief: ORPHA, OMIM, ICD-10/ICD-11 ids only, GARD, MeSH, UMLS ids only, NCIT, DOID, MedGen).
const KEEP: [&str; 12] = [
    "ORPHA",
    "OMIM",
    "OMIMPS",
    "ICD10CM",
    "ICD10WHO",
    "icd11.foundation",
    "GARD",
    "MESH",
    "UMLS",
    "NCIT",
    "DOID",
    "MEDGEN",
];
/// Identifier only, no labels (licence terms of the terminology).
const IDS_ONLY: [&str; 6] = [
    "ICD10CM",
    "ICD10WHO",
    "icd11.foundation",
    "icd11.code",
    "UMLS",
    "MEDGEN",
];

/// When exact links put two ids of one prefix in one cluster, the links of the least specific
/// terminology are demoted first; MONDO and ORPHA links last.
const WEAKEST_FIRST: [&str; 14] = [
    "UMLS",
    "MEDGEN",
    "MESH",
    "icd11.code",
    "icd11.foundation",
    "ICD10WHO",
    "ICD10CM",
    "NCIT",
    "DOID",
    "GARD",
    "OMIMPS",
    "OMIM",
    "ORPHA",
    "MONDO",
];

fn norm_prefix(p: &str) -> &str {
    match p {
        "Orphanet" => "ORPHA",
        "mesh" => "MESH",
        _ => p,
    }
}

fn norm(curie: &str) -> String {
    match curie.split_once(':') {
        Some((p, l)) => format!("{}:{l}", norm_prefix(p)),
        None => curie.into(),
    }
}

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (k, v) in [
        ("MONDO", "http://purl.obolibrary.org/obo/MONDO_"),
        ("ORPHA", "http://www.orpha.net/ORDO/Orphanet_"),
        ("OMIM", "https://omim.org/entry/"),
        ("OMIMPS", "https://omim.org/phenotypicSeries/PS"),
        ("ICD10CM", "http://purl.bioontology.org/ontology/ICD10CM/"),
        ("ICD10WHO", "https://icd.who.int/browse10/2019/en#/"),
        ("icd11.foundation", "http://id.who.int/icd/entity/"),
        ("icd11.code", "https://icd.who.int/browse/2025-01/mms/en#"),
        ("GARD", "https://rarediseases.info.nih.gov/diseases/"),
        ("MESH", "http://id.nlm.nih.gov/mesh/"),
        ("UMLS", "http://linkedlifedata.com/resource/umls/id/"),
        ("NCIT", "http://purl.obolibrary.org/obo/NCIT_"),
        ("DOID", "http://purl.obolibrary.org/obo/DOID_"),
        ("MEDGEN", "http://identifiers.org/medgen/"),
    ] {
        m.insert(k.into(), v.into());
    }
    m
}

fn mondo_url(id: &str) -> String {
    format!("http://purl.obolibrary.org/obo/{}", id.replace(':', "_"))
}

/// MONDO OBO: per term (name, obsolete) and per xref the `source=` annotations.
struct Obo {
    obsolete: HashSet<String>,
    xref_sources: HashMap<(String, String), String>,
    gard: Vec<(String, String, usize)>,
    names: HashMap<String, String>,
}

fn read_obo(path: &Path) -> Result<Obo> {
    let mut o = Obo {
        obsolete: HashSet::new(),
        xref_sources: HashMap::new(),
        gard: vec![],
        names: HashMap::new(),
    };
    let mut id = String::new();
    let mut in_term = false;
    for (n, line) in lines(path)?.lines().enumerate() {
        let line = line?;
        if line.starts_with('[') {
            in_term = line == "[Term]";
            id.clear();
            continue;
        }
        if !in_term {
            continue;
        }
        if let Some(v) = line.strip_prefix("id: ") {
            id = v.trim().into();
        } else if let Some(v) = line.strip_prefix("name: ") {
            o.names.insert(id.clone(), v.trim().into());
        } else if line == "is_obsolete: true" {
            o.obsolete.insert(id.clone());
        } else if let Some(v) = line.strip_prefix("xref: ") {
            let (x, rest) = v.split_once(' ').unwrap_or((v, ""));
            let x = norm(x);
            if x.starts_with("GARD:") && id.starts_with("MONDO:") {
                o.gard.push((id.clone(), x.clone(), n + 1));
            }
            o.xref_sources.insert((id.clone(), x), rest.to_string());
        }
    }
    Ok(o)
}

/// Orphanet product 1: ORPHA code, name, and each external reference with its relation code.
struct OrphaRef {
    orpha: String,
    name: String,
    source: String,
    reference: String,
    relation: String,
}

fn read_orphanet(path: &Path) -> Result<Vec<OrphaRef>> {
    let mut r = Reader::from_reader(std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?));
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let (mut orpha, mut name) = (String::new(), String::new());
    let (mut source, mut reference, mut relation) = (String::new(), String::new(), String::new());
    let mut out = Vec::new();
    loop {
        match r.read_event_into(&mut buf)? {
            Event::Start(e) => stack.push(String::from_utf8_lossy(e.name().as_ref()).into_owned()),
            Event::End(e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if tag == "ExternalReference" && stack.iter().rev().nth(1).is_some_and(|t| t == "ExternalReferenceList")
                {
                    out.push(OrphaRef {
                        orpha: orpha.clone(),
                        name: name.clone(),
                        source: std::mem::take(&mut source),
                        reference: std::mem::take(&mut reference),
                        relation: std::mem::take(&mut relation),
                    });
                }
                stack.pop();
            }
            Event::Text(t) => {
                let text = t.decode().map(|c| c.into_owned()).unwrap_or_default();
                let n = stack.len();
                let parent = if n >= 2 { stack[n - 2].as_str() } else { "" };
                match (stack.last().map(String::as_str), parent) {
                    (Some("OrphaCode"), "Disorder") => orpha = text.trim().into(),
                    (Some("Name"), "Disorder") => name = text.trim().into(),
                    (Some("Source"), "ExternalReference") => source = text.trim().into(),
                    (Some("Reference"), "ExternalReference") => reference = text.trim().into(),
                    (Some("Name"), "DisorderMappingRelation") => {
                        relation = text.split_whitespace().next().unwrap_or("").into()
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn orpha_target(source: &str, reference: &str) -> Option<String> {
    Some(match source {
        "OMIM" => format!("OMIM:{reference}"),
        "ICD-10" => format!("ICD10WHO:{reference}"),
        "ICD-11" => format!("icd11.code:{reference}"),
        "MeSH" => format!("MESH:{reference}"),
        "UMLS" => format!("UMLS:{reference}"),
        // GARD ids are numbers; MONDO writes them zero-padded to 7 digits. Same number, same id.
        "GARD" => format!("GARD:{:0>7}", reference),
        // Orphanet drops MONDO's leading zeros; MONDO ids are 7 digits.
        "MONDO" => format!("MONDO:{:0>7}", reference),
        _ => return None,
    })
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let sssom_path = latest(data, "mondo-sssom", "mondo.sssom.tsv")?;
    let obo_path = data.join("raw").join("mondo.obo");
    let orpha_path = data.join("raw").join("en_product1.xml");

    let mut mondo = MappingSet {
        id: "disease-xrefs".into(),
        description: "MONDO disease cross-references (MONDO's own SSSOM), checked against MONDO OBO annotations and Orphanet's own mappings".into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let i_sssom = mondo.add_input(Input::from_file("MONDO SSSOM", &sssom_path, "", "", "CC-BY-4.0")?);
    mondo.inputs[i_sssom].license = "CC-BY-4.0 (MONDO; the SSSOM header says 'unspecified')".into();
    let i_obo = mondo.add_input(Input::from_file(
        "MONDO OBO (atlas release)",
        &obo_path,
        "http://purl.obolibrary.org/obo/mondo.obo",
        "releases/2026-09-01",
        "CC-BY-4.0",
    )?);
    let obo = read_obo(&obo_path).context("mondo.obo")?;

    let mut header: Vec<String> = Vec::new();
    for (n, line) in lines(&sssom_path)?.lines().enumerate() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if header.is_empty() {
            header = f.iter().map(|s| s.to_string()).collect();
            continue;
        }
        let col = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .and_then(|i| f.get(i))
                .copied()
                .unwrap_or("")
        };
        let (s, p, o) = (col("subject_id"), col("predicate_id"), norm(col("object_id")));
        let loc = format!("line {}", n + 1);
        let op = prefix(&o).to_string();
        if !s.starts_with("MONDO:") {
            mondo.exclude_from(Some(i_sssom), "subject is not a MONDO id", loc);
            continue;
        }
        if !KEEP.contains(&op.as_str()) {
            let reason = match op.as_str() {
                "SCTID" => "SNOMED CT: licence restricts redistribution; not ingested",
                "MedDRA" => "MedDRA: licensed terminology; not ingested",
                _ => "object prefix outside the identity brief",
            };
            mondo.exclude_from(Some(i_sssom), &format!("{reason} ({op})"), loc);
            continue;
        }
        let mut row = match p {
            EXACT => Row::xref(s, &o, i_sssom, loc),
            "skos:broadMatch" | "skos:narrowMatch" | "skos:closeMatch" | "skos:relatedMatch" => {
                Row::link(s, p, &o, i_sssom, loc)
            }
            _ => {
                mondo.exclude_from(Some(i_sssom), &format!("predicate {p}"), loc);
                continue;
            }
        };
        row.subject_label = col("subject_label").into();
        row.upstream_justification = col("mapping_justification").into();
        if !IDS_ONLY.contains(&op.as_str()) {
            row.object_label = col("object_label").into();
        }
        row.subject_source = "infores:mondo".into();
        row.object_source = format!("obo:{}", op.to_lowercase());
        row.evidence_url = mondo_url(s);
        row.comment = format!(
            "MONDO-asserted {p} (upstream justification {})",
            col("mapping_justification")
        );
        if op == "GARD" && row.is_exact() {
            row.demote("current_id_unverified");
            row.comment
                .push_str("; round 2 requires current GARD assertion/independent corroboration: see gard-xrefs");
        }
        if obo.obsolete.contains(s) {
            row.conflict = "retired".into();
        }
        match obo.xref_sources.get(&(s.to_string(), o.clone())) {
            Some(src) if src.contains("bsolete") => {
                row.conflict = "retired".into();
                row.comment.push_str(&format!("; OBO annotation {src}"));
            }
            None if row.asserted == EXACT => {
                row.demote("release_drift");
                row.comment
                    .push_str("; mapping absent from pinned OBO: requires release reconciliation");
            }
            _ => {}
        }
        if row.conflict == "retired" && row.is_exact() {
            row.predicate = CLOSE.into();
        }
        mondo.rows.push(row);
    }
    // GARD: in the OBO only, annotated MONDO:GARD (imported from GARD), no equivalence annotation.
    for (m, g, line) in &obo.gard {
        let mut row = Row::link(m, CLOSE, g, i_obo, format!("line {line}"));
        row.justification = XREF.into();
        row.confidence = 0.8;
        row.subject_label = obo.names.get(m).cloned().unwrap_or_default();
        row.subject_source = "infores:mondo".into();
        row.object_source = "infores:gard".into();
        row.evidence_url = mondo_url(m);
        row.comment =
            "MONDO OBO xref with source MONDO:GARD (GARD-provided, not annotated as equivalence): candidate".into();
        if obo.obsolete.contains(m) {
            row.conflict = "retired".into();
        }
        mondo.rows.push(row);
    }

    // Orphanet's own mappings.
    let mut orpha = MappingSet {
        id: "disease-orphanet-xrefs".into(),
        description: "Orphanet's own disorder mappings (product 1): E exact, NTBT/BTNT granularity, ND undecided"
            .into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let i_orpha = orpha.add_input(Input::from_file(
        "Orphanet product 1 (cross-references)",
        &orpha_path,
        "https://www.orphadata.com/data/xml/en_product1.xml",
        "JDBOR 2026-06-23 07:53:50",
        "CC-BY-4.0",
    )?);
    let refs = read_orphanet(&orpha_path).context("en_product1.xml")?;
    let mut contradictions: Vec<(String, String, String)> = Vec::new();
    for (k, x) in refs.iter().enumerate() {
        let loc = format!(
            "Disorder OrphaCode {} / ExternalReference #{k} ({} {})",
            x.orpha, x.source, x.reference
        );
        let Some(target) = orpha_target(&x.source, &x.reference) else {
            orpha.exclude_from(
                Some(i_orpha),
                &format!("source {} outside the identity brief or licensed", x.source),
                loc,
            );
            continue;
        };
        let s = format!("ORPHA:{}", x.orpha);
        let (pred, note) = match x.relation.as_str() {
            "E" => (EXACT, "Orphanet E: exact"),
            "NTBT" => ("skos:broadMatch", "Orphanet NTBT: ORPHA code narrower than target"),
            "BTNT" => ("skos:narrowMatch", "Orphanet BTNT: ORPHA code broader than target"),
            "NTBT/E" | "BTNT/E" => (CLOSE, "Orphanet granularity/E mixed"),
            "ND" => ("skos:relatedMatch", "Orphanet ND: not decided"),
            other => {
                orpha.exclude_from(Some(i_orpha), &format!("relation {other}"), loc);
                continue;
            }
        };
        let mut row = if pred == EXACT {
            Row::xref(&s, &target, i_orpha, loc)
        } else {
            Row::link(&s, pred, &target, i_orpha, loc)
        };
        if pred != EXACT {
            contradictions.push((
                s.clone(),
                target.clone(),
                format!("Orphanet {} for this pair", x.relation),
            ));
        }
        row.subject_label = x.name.clone();
        if !IDS_ONLY.contains(&prefix(&target)) && prefix(&target) == "MONDO" {
            row.object_label = obo.names.get(&target).cloned().unwrap_or_default();
        }
        row.subject_source = "infores:orphanet".into();
        row.object_source = format!("obo:{}", prefix(&target).to_lowercase());
        row.evidence_url = format!("https://www.orpha.net/en/disease/detail/{}", x.orpha);
        row.comment = note.into();
        if prefix(&target) == "GARD" && row.is_exact() {
            row.demote("current_id_unverified");
            row.comment.push_str(
                "; round 2: legacy Orphanet GARD assertion requires current GARD corroboration; see gard-xrefs",
            );
        }
        if prefix(&target) == "MONDO" && obo.obsolete.contains(&target) {
            row.conflict = "retired".into();
            row.predicate = CLOSE.into();
        }
        orpha.rows.push(row);
    }

    check_cardinality(&mut mondo);
    check_cardinality(&mut orpha);
    // MONDO and Orphanet both use OMIM/MESH/UMLS/ICD; a joint cluster may hold several ICD/UMLS/MESH
    // codes only if each source keeps them 1:1, so no prefix is exempt.
    let contradicted = check_contradictions(&mut [&mut mondo, &mut orpha], &contradictions);
    let cluster_demoted = check_clusters_ordered(&mut [&mut mondo, &mut orpha], &[], &WEAKEST_FIRST);

    // Version drift between the SSSOM release and the OBO the atlas uses (report, not a conflict).
    let in_obo = mondo
        .rows
        .iter()
        .filter(|r| r.input == i_sssom && !r.comment.contains("not in the 2026-09-01 OBO"))
        .count();
    mondo.parameters = json!({
        "prefixes_kept": KEEP, "ids_only": IDS_ONLY,
        "rule": "exact = MONDO SSSOM skos:exactMatch, not obsolete, 1:1 per prefix pair, cluster has one id per prefix, no Orphanet non-E assertion inside the cluster",
        "gard": "candidate (closeMatch 0.8): OBO xref source MONDO:GARD has no equivalence annotation",
    });
    orpha.parameters = json!({"relations": {"E": EXACT, "NTBT": "skos:broadMatch", "BTNT": "skos:narrowMatch", "NTBT/E|BTNT/E": CLOSE, "ND": "skos:relatedMatch"},
        "gard_normalisation": "Orphanet GARD numbers zero-padded to 7 digits as MONDO writes them",
        "gard_exact": "legacy Orphanet E is a candidate pending current GARD corroboration (gard-xrefs)"});
    mondo.extra.insert("sssom_rows_also_in_atlas_obo".into(), json!(in_obo));
    mondo
        .extra
        .insert("cluster_conflict_rows_demoted_both_sets".into(), json!(cluster_demoted));
    mondo
        .extra
        .insert("cross_source_rows_demoted_both_sets".into(), json!(contradicted));
    Ok(vec![mondo, orpha])
}

/// A non-exact assertion by one source (a, b) contradicts exact rows that put a and b in one cluster.
pub fn check_contradictions(sets: &mut [&mut MappingSet], pairs: &[(String, String, String)]) -> usize {
    let mut comp: HashMap<String, usize> = HashMap::new();
    let mut all_rows: Vec<Row> = Vec::new();
    for s in sets.iter() {
        all_rows.extend(s.rows.iter().filter(|r| r.is_exact()).cloned());
    }
    for (i, c) in clusters(&all_rows).into_iter().enumerate() {
        for id in c {
            comp.insert(id, i);
        }
    }
    // The contradicted target id b: its exact links inside the joint cluster are the ones that put it there.
    let mut bad: HashMap<String, String> = HashMap::new();
    for (a, b, why) in pairs {
        if matches!((comp.get(a), comp.get(b)), (Some(x), Some(y)) if x == y) {
            bad.insert(b.clone(), format!("{why} ({a})"));
        }
    }
    let mut n = 0;
    for s in sets.iter_mut() {
        for r in s.rows.iter_mut().filter(|r| r.is_exact()) {
            if let Some(why) = bad.get(&r.subject_id).or_else(|| bad.get(&r.object_id)) {
                r.demote("cross_source");
                r.comment.push_str(&format!(
                    "; exact links join ids another source relates non-exactly: {why}"
                ));
                n += 1;
            }
        }
    }
    n
}
