//! Offline real-data audit. Writes only $RARE_ATLAS_DATA/cache/evidence/.
use atlas_analytics::evidence::*;
use atlas_core::{Atlas, node::EdgeKind, provenance::SourceEntity};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const GENES: [&str; 10] = [
    "STXBP1", "STX1B", "SNAP25", "SYT1", "VAMP2", "UNC13A", "SCN1A", "SCN2A", "KCNQ2", "SYNGAP1",
];

fn entity(path: &Path, url: &str, version: Option<&str>) -> SourceEntity {
    SourceEntity {
        id: format!("source:{}", path.file_name().unwrap().to_string_lossy()),
        file: path.file_name().unwrap().to_string_lossy().into_owned(),
        url: url.into(),
        version: version.map(str::to_owned),
        retrieved_at: Some(atlas_ingest::sources::rfc3339(
            fs::metadata(path).unwrap().modified().unwrap(),
        )),
        sha256: Some(atlas_ingest::sources::sha256(path).unwrap()),
        bytes: fs::metadata(path).unwrap().len(),
        licence: None,
    }
}

fn g2p(atlas: &Atlas, raw: &Path) -> (Vec<Statement>, Vec<String>) {
    let path = raw.join("allG2P_2026-09-28.csv.gz");
    let entity = entity(
        &path,
        "https://www.ebi.ac.uk/gene2phenotype",
        Some("2026-09-28 (filename)"),
    );
    let mut csv = csv::Reader::from_reader(flate2::read::GzDecoder::new(fs::File::open(path).unwrap()));
    let headers = csv.headers().unwrap().clone();
    let mut out = Vec::new();
    let mut excluded = Vec::new();
    for record in csv.records() {
        let row = record.unwrap();
        let get = |key| row.get(headers.iter().position(|h| h == key).unwrap()).unwrap();
        if !GENES.contains(&get("gene symbol")) {
            continue;
        }
        let id = get("g2p id");
        let candidates = [get("disease MONDO").to_owned(), format!("OMIM:{}", get("disease mim"))];
        let targets: BTreeSet<_> = candidates.iter().filter_map(|id| atlas.disease_idx(id)).collect();
        if targets.len() != 1 {
            excluded.push(format!(
                "{id}: {} existing canonical disease targets; no fuzzy/label fallback",
                targets.len()
            ));
            continue;
        }
        let target = atlas.disease_at(*targets.first().unwrap());
        let gene = atlas.gene(get("gene symbol")).expect("slice gene exists");
        out.push(Statement {
            id: id.into(),
            subject: target.id.clone(),
            relation: Relation::Mechanism,
            object: atlas.gene_at(gene).id().into(),
            value: match get("molecular mechanism") {
                "loss of function" => Value::LossOfFunction,
                "gain of function" => Value::GainOfFunction,
                "dominant negative" => Value::DominantNegative,
                _ => Value::Unknown,
            },
            raw_value: format!(
                "{}; support={}; disease={}",
                get("molecular mechanism"),
                get("molecular mechanism support"),
                get("disease name")
            ),
            kind: if get("molecular mechanism support") == "inferred" {
                EdgeKind::Inferred
            } else {
                EdgeKind::Observed
            },
            tier: Tier::Curated,
            curation: match get("confidence") {
                "disputed" => Curation::Disputed,
                "refuted" => Curation::Refuted,
                _ => Curation::Reviewed,
            },
            source_classification: Some(get("confidence").into()),
            context: Context {
                source_disease: candidates
                    .iter()
                    .find(|s| !s.is_empty() && s.as_str() != "OMIM:")
                    .unwrap()
                    .clone(),
                inheritance: Some(get("allelic requirement").into()),
                ..Context::default()
            },
            citations: vec![Citation {
                entity: entity.clone(),
                locator: format!("g2p id={id}"),
                references: get("publications")
                    .split(';')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!("PMID:{}", s.trim()))
                    .collect(),
            }],
            reviewed_year: get("date of last review").get(..4).and_then(|s| s.parse().ok()),
            quote: None,
        });
    }
    (out, excluded)
}

fn pathways(atlas: &Atlas, raw: &Path, diseases: &[String]) -> Vec<Statement> {
    let path = raw.join("NCBI2Reactome.txt");
    // Upstream release was not captured at download; do not invent a release number.
    let entity = entity(&path, "https://reactome.org/download-data", None);
    let rows = fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    for disease in diseases {
        let d = atlas.disease_at(atlas.disease_idx(disease).unwrap());
        for gene in &d.genes {
            if !GENES.contains(&gene.symbol.as_str())
                || !(gene.association.to_ascii_lowercase().starts_with("disease-causing")
                    || gene.association == "MENDELIAN")
            {
                continue;
            }
            let Some(ncbi) = atlas
                .gene(&gene.symbol)
                .and_then(|i| atlas.gene_at(i).ncbi_gene.as_ref())
            else {
                continue;
            };
            let ncbi = ncbi.strip_prefix("NCBIGene:").unwrap_or(ncbi);
            for (line, row) in rows.lines().enumerate() {
                let fields: Vec<_> = row.split('\t').collect();
                if fields.len() < 6 || fields[0] != ncbi || fields[5] != "Homo sapiens" {
                    continue;
                }
                out.push(Statement {
                    id: format!(
                        "{}|{}|{}|Reactome:L{}",
                        d.id,
                        atlas.provenance.cite(&gene.record),
                        gene.association,
                        line + 1
                    ),
                    subject: d.id.clone(),
                    relation: Relation::Pathway,
                    object: fields[1].into(),
                    value: Value::Present,
                    raw_value: format!("{} via {}; {}", fields[3], gene.symbol, fields[4]),
                    kind: EdgeKind::Inferred,
                    tier: Tier::Curated,
                    curation: Curation::Unknown,
                    source_classification: None,
                    context: Context {
                        source_disease: gene.source_disease.clone(),
                        ..Context::default()
                    },
                    citations: vec![
                        Citation {
                            entity: entity.clone(),
                            locator: format!("L{}", line + 1),
                            references: vec![fields[2].into()],
                        },
                        Citation {
                            entity: atlas.provenance.entity(gene.record.entity).clone(),
                            locator: gene.record.locator.to_string(),
                            references: gene.pmids.clone(),
                        },
                    ],
                    reviewed_year: None,
                    quote: None,
                });
            }
        }
    }
    out
}

fn cohort_examples(atlas: &Atlas, data: &Path) -> Vec<Statement> {
    let path = data.join("cache/pubmed/SCN1A.json");
    let cache: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut entity = entity(&path, "https://pubmed.ncbi.nlm.nih.gov/", Some("pubmed.articles v1"));
    entity.retrieved_at = cache["header"]["retrieved_at"].as_str().map(str::to_owned);
    let mut out = Vec::new();
    for (id, k, n, pop, quote) in [
        (
            "PMID:41617776",
            1061,
            1215,
            "Chinese DS/DS-like cohort, 2005-2023",
            "SCN1A variants were identified in 1061 patients (87.3%)",
        ),
        (
            "PMID:39299018",
            30,
            52,
            "North Indian children with DS phenotype, 2015-2019",
            "pathogenic variants in the SCN1A gene were identified in 30 children",
        ),
    ] {
        let row = cache["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        assert!(
            row["abstract"].as_str().unwrap().contains(quote),
            "Extraction anchor changed"
        );
        assert!(
            row["abstract"].as_str().unwrap().contains(&n.to_string()),
            "Denominator absent"
        );
        out.push(Statement {
            id: format!("{id}|SCN1A-fraction"),
            subject: atlas.disease_at(atlas.disease_idx("MONDO:0100135").unwrap()).id.clone(),
            relation: Relation::GeneAssociation,
            object: atlas.gene_at(atlas.gene("SCN1A").unwrap()).id().into(),
            value: Value::Frequency {
                affected: k,
                examined: n,
            },
            raw_value: format!("{k}/{n}; manual extraction checked against cached abstract anchors"),
            kind: EdgeKind::Extracted,
            tier: Tier::Cohort,
            curation: Curation::Unreviewed,
            source_classification: None,
            context: Context {
                source_disease: "MONDO:0100135".into(),
                population: Some(pop.into()),
                ..Context::default()
            },
            citations: vec![Citation {
                entity: entity.clone(),
                locator: format!("records[id={id}].abstract"),
                references: vec![row["url"].as_str().unwrap().into(), id.into()],
            }],
            reviewed_year: None,
            quote: Some(quote.into()),
        });
    }
    out
}

#[test]
#[ignore = "real-data audit (rebuilds the graph, writes data/cache/evidence/); run with --ignored"]
fn audit_real_slice() {
    let data = atlas_ingest::data_dir();
    if !data.join("raw").join("allG2P_2026-09-28.csv.gz").exists() || !data.join("cache/pubmed/SCN1A.json").exists() {
        eprintln!("skipped: real data missing under {}", data.display());
        return;
    }
    let atlas = atlas_ingest::build(&data.join("raw")).expect("real graph build (never overwrites shared snapshot)");
    let mut diseases: Vec<_> = atlas
        .active()
        .filter(|(_, d)| d.genes.iter().any(|g| GENES.contains(&g.symbol.as_str())))
        .map(|(_, d)| d.id.clone())
        .collect();
    diseases.sort();
    let (mut additional, excluded) = g2p(&atlas, &data.join("raw"));
    // Slice membership includes either base-graph gene links or mapped G2P slice-gene records.
    diseases.extend(additional.iter().map(|s| s.subject.clone()));
    diseases.sort();
    diseases.dedup();
    additional.extend(pathways(&atlas, &data.join("raw"), &diseases));
    additional.extend(cohort_examples(&atlas, &data));
    let mut review = analyze_atlas(&atlas, Some(&diseases), &additional, 2026).unwrap();
    review.limitations.extend(excluded.clone());
    review
        .activity
        .count("excluded:g2p-unresolved-or-ambiguous", excluded.len() as u64);
    let reordered: Vec<_> = review.statements.iter().cloned().rev().collect();
    let again = analyze(&reordered, 2026).unwrap();
    assert_eq!(review.edges, again.edges);
    assert_eq!(review.findings, again.findings);
    assert!(review.findings.iter().any(|f| f.kind == FindingKind::ScopeReview));
    if ["G2P03905", "G2P03909"]
        .iter()
        .all(|id| review.statements.iter().any(|s| s.id == *id))
    {
        assert!(review.findings.iter().any(|f| f.kind == FindingKind::ContextDifference
            && f.statements.contains(&"G2P03905".into())
            && f.statements.contains(&"G2P03909".into())));
    } else {
        assert!(excluded.iter().any(|s| s.starts_with("G2P03905")));
        assert!(excluded.iter().any(|s| s.starts_with("G2P03909")));
    }
    let proposal = propose_shared_mechanism(&review, "MONDO:0012812", "MONDO:0014590").unwrap();
    assert!(!proposal.statement_ids.is_empty(), "real STXBP1/SNAP25 shared pathway");
    let mut counts = BTreeMap::<String, usize>::new();
    for f in &review.findings {
        *counts.entry(format!("{:?}", f.kind)).or_default() += 1;
    }
    let output = data.join("cache/evidence");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("review.json"), serde_json::to_vec_pretty(&review).unwrap()).unwrap();
    let mut manifest: BTreeMap<String, SourceEntity> = atlas
        .provenance
        .entities
        .iter()
        .map(|e| (e.file.clone(), e.clone()))
        .collect();
    for c in review.statements.iter().flat_map(|s| &s.citations) {
        manifest.insert(c.entity.file.clone(), c.entity.clone());
    }
    fs::write(
        output.join("sources.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("proposal.json"),
        serde_json::to_vec_pretty(&proposal).unwrap(),
    )
    .unwrap();
    let mut report = format!(
        "Diseases: {}\nStatements: {}\nEdges: {}\nCounts: {:?}\nExcluded: {:?}\n",
        diseases.len(),
        review.statements.len(),
        review.edges.len(),
        counts,
        excluded
    );
    for f in &review.findings {
        if matches!(f.kind, FindingKind::UnknownMechanism | FindingKind::IndirectLink) {
            continue;
        }
        report.push_str(&format!("\n{:?}: {}\n", f.kind, f.explanation));
        for id in &f.statements {
            let s = review.statements.iter().find(|s| &s.id == id).unwrap();
            report.push_str(&format!(
                "  {} {} {} {:?} raw={}\n",
                s.id, s.subject, s.object, s.value, s.raw_value
            ));
        }
    }
    fs::write(output.join("audit.txt"), &report).unwrap();
    println!("{}", report.lines().take(5).collect::<Vec<_>>().join("\n"));
    println!("Proposal: {}", proposal.question);
    println!(
        "Review SHA-256: {}",
        atlas_ingest::sources::sha256(&output.join("review.json")).unwrap()
    );
}
