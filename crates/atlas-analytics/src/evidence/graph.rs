use super::*;
use atlas_core::node::EdgeKind;
use atlas_core::provenance::RecordRef;
use atlas_core::{Atlas, DiseaseIdx};

fn citation(atlas: &Atlas, record: &RecordRef, references: Vec<String>) -> Citation {
    Citation {
        entity: atlas.provenance.entity(record.entity).clone(),
        locator: record.locator.to_string(),
        references,
    }
}

/// Read-only adapter for the base Atlas: phenotype/NOT, cohort fractions, gene links, effects.
/// Unknown or retired requested disease IDs error explicitly. None selects all active diseases.
pub fn statements_from_atlas(atlas: &Atlas, diseases: Option<&[String]>) -> Result<Vec<Statement>, String> {
    let indices: Vec<DiseaseIdx> = match diseases {
        Some(ids) => ids
            .iter()
            .map(|id| atlas.disease_idx(id).ok_or_else(|| format!("Unknown disease: {id}")))
            .collect::<Result<_, _>>()?,
        None => atlas.active().map(|(idx, _)| idx).collect(),
    };
    let mut statements = Vec::new();
    for idx in indices {
        let d = atlas.disease_at(idx);
        if !d.is_active() {
            return Err(format!("Retired disease requested: {}", d.id));
        }
        for edge in d.phenotypes.iter().chain(&d.excluded) {
            for a in &edge.annotations {
                let frequency = a.frequency.as_ref().and_then(|f| f.cohort);
                let value = if a.negated {
                    Value::Absent
                } else if let Some((affected, examined)) = frequency {
                    Value::Frequency { affected, examined }
                } else {
                    Value::Present
                };
                statements.push(Statement {
                    id: format!(
                        "{}|phenotype|{}",
                        atlas.provenance.cite(&a.record),
                        atlas.hpo.term(edge.term).id
                    ),
                    subject: d.id.clone(),
                    relation: Relation::Phenotype,
                    object: atlas.hpo.term(edge.term).id.clone(),
                    raw_value: format!(
                        "qualifier={};frequency={};evidence={}",
                        if a.negated { "NOT" } else { "" },
                        a.frequency.as_ref().map_or("", |f| f.raw.as_str()),
                        a.evidence
                    ),
                    value,
                    kind: EdgeKind::Observed,
                    tier: match a.evidence.as_str() {
                        "PCS" => Tier::Cohort,
                        "TAS" => Tier::AuthorStatement,
                        "IEA" => Tier::Computational,
                        _ => Tier::Unrated,
                    },
                    curation: Curation::Unknown,
                    source_classification: Some(a.evidence.clone()),
                    context: Context {
                        source_disease: a.disease_id.clone(),
                        onset: a.onset.clone(),
                        sex: a.sex.clone(),
                        modifiers: a.modifiers.clone(),
                        ..Context::default()
                    },
                    citations: vec![citation(atlas, &a.record, a.references.clone())],
                    reviewed_year: a.date().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()),
                    quote: None,
                });
            }
        }
        for g in &d.genes {
            let object = atlas
                .gene(&g.symbol)
                .map(|i| atlas.gene_at(i).id().to_owned())
                .unwrap_or_else(|| g.symbol.clone());
            let mut source = citation(atlas, &g.record, g.pmids.clone());
            // Base ingest's Orphanet locator ends at gene, but the XML can contain separate
            // LoF and GoF association records for that same gene. Retain and identify both.
            if g.source == "Orphanet" {
                source
                    .locator
                    .push_str(&format!("[DisorderGeneAssociationType/Name={:?}]", g.association));
            }
            let mut s = Statement {
                id: format!("{}#{}|gene|{}", source.entity.file, source.locator, object),
                subject: d.id.clone(),
                relation: Relation::GeneAssociation,
                object,
                value: Value::Present,
                raw_value: g.association.clone(),
                kind: EdgeKind::Observed,
                tier: Tier::Curated,
                curation: match g.assessed {
                    Some(true) => Curation::Reviewed,
                    Some(false) => Curation::Unreviewed,
                    None => Curation::Unknown,
                },
                source_classification: g.assessed.map(|x| if x { "Assessed" } else { "Not assessed" }.into()),
                context: Context {
                    source_disease: g.source_disease.clone(),
                    ..Context::default()
                },
                citations: vec![source],
                reviewed_year: None,
                quote: None,
            };
            statements.push(s.clone());
            s.id = s.id.replace("|gene|", "|mechanism|");
            s.relation = Relation::Mechanism;
            s.value = match g.variant_effect() {
                "LoF" => Value::LossOfFunction,
                "GoF" => Value::GainOfFunction,
                "DN" => Value::DominantNegative,
                _ => Value::Unknown,
            };
            statements.push(s);
        }
    }
    Ok(statements)
}

/// Additional assertions are already normalized by their ingest owner (e.g. G2P/pathways/LLM).
/// With a disease filter, additional assertions outside the selected canonical IDs are retained
/// in the caller and counted as excluded in the returned activity.
pub fn analyze_atlas(
    atlas: &Atlas,
    diseases: Option<&[String]>,
    additional: &[Statement],
    as_of_year: u16,
) -> Result<Review, String> {
    let mut statements = statements_from_atlas(atlas, diseases)?;
    let selected: Option<std::collections::BTreeSet<_>> = diseases.map(|ids| {
        ids.iter()
            .filter_map(|id| atlas.disease_idx(id))
            .map(|idx| atlas.disease_at(idx).id.clone())
            .collect()
    });
    let mut excluded = 0;
    for s in additional {
        if selected.as_ref().is_none_or(|ids| ids.contains(&s.subject)) {
            statements.push(s.clone());
        } else {
            excluded += 1;
        }
    }
    let mut review = analyze(&statements, as_of_year)?;
    review
        .activity
        .count("excluded:additional-outside-requested-disease-scope", excluded);
    let used: std::collections::BTreeSet<_> = review
        .statements
        .iter()
        .flat_map(|s| &s.citations)
        .filter_map(|c| atlas.provenance.entities.iter().position(|e| e == &c.entity))
        .collect();
    review.activity.used = used
        .into_iter()
        .map(|i| atlas_core::provenance::EntityIdx(i as u16))
        .collect();
    Ok(review)
}
