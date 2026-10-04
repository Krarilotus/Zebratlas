//! Extracted claims (`claims.extracted` v1, D4): LLM-extracted statements from PubMed abstracts
//! whose quote was verified verbatim. Only **accepted** claims become edges (`claims_about`,
//! kind `extracted`, paper → gene or phenotype); excluded claims are counted with their reasons in
//! the coverage row. The claim type and the verified quote travel in the edge reason.

use std::path::Path;

use atlas_core::graph::{LicenceClass, LinkLevel, Relation, activity};
use atlas_core::node::{EdgeKind, NodeKind};

use super::builder::Builder;
use super::research::{self, Spec, Via, s};
use crate::error::IngestError;

pub const FILE: &str = "cache/claims/claims.json";

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_CLAIMS,
        "Extracted claims with verified quotes (accepted only)",
        &[],
    );
    let spec = Spec {
        source: "claims",
        label: "Claims extracted from abstracts (LLM, quote verified)",
        file: FILE,
        schema: "claims.extracted",
        versions: &[1],
        url: "derived: atlas-extract over data/cache/pubmed (abstract text stays with PubMed)",
        licence: "derived statements; quotes from PubMed abstracts (publisher copyright, shown as short quotes only)",
        class: LicenceClass::Unknown,
        scope: "accepted claims about slice genes from a sample of PubMed abstracts",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        b.finish(act, &[]);
        return Ok(());
    };
    let (mut edges, mut excluded, mut no_paper) = (0, 0, 0);
    for i in 0..src.env.records.len() {
        let r = &src.env.records[i];
        if s(r, "status") != "accepted" || r["quote_verified"].as_bool() != Some(true) {
            excluded += 1;
            continue;
        }
        let paper = s(r, "pmid");
        if !matches!(b.node(paper), Some((NodeKind::Paper, _))) {
            no_paper += 1;
            continue;
        }
        let rec = src.record(b, i);
        let c = &r["candidate"];
        let quote: String = s(c, "quote").chars().take(300).collect();
        let reason = format!("{} claim: \"{quote}\"", s(c, "claim_type"));
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Extracted,
            level: LinkLevel::Text,
        };
        let paper = paper.to_owned();
        for node in [&r["gene_node"], &r["phenotype_node"]] {
            let id = s(node, "id");
            let target = match s(node, "kind") {
                "gene" => research::gene(b, id),
                "phenotype" => b.atlas.hpo.canonical(id).map(|_| id.to_owned()),
                _ => None,
            };
            if let Some(t) = target {
                edges += via.link(b, &paper, Relation::ClaimsAbout, &t, reason.clone());
            }
        }
    }
    b.count(act, "skipped:paper-not-in-graph", no_paper);
    src.finish(b, 0, edges, excluded + no_paper);
    b.finish(act, &[("edges", edges)]);
    Ok(())
}
