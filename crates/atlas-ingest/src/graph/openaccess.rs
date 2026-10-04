//! Source-reported free-to-read locations, joined to existing papers by exact identifiers only.
//! The inventory includes closed/unknown records; keep them in provenance without making OA claims.
use std::collections::HashMap;
use std::path::Path;

use atlas_core::graph::{LicenceClass, OpenAccess};

use super::builder::Builder;
use super::research::{self, Spec, opt, s, strs};
use crate::error::IngestError;

pub const FILE: &str = "cache/openaccess/live-papers.json";

fn doi(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("DOI:")
        .to_ascii_lowercase()
}

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        "activity:ingest-openaccess",
        "Exact paper identifier joins to free-to-read metadata",
        &[],
    );
    let spec = Spec {
        source: "openaccess",
        label: "Unpaywall / Europe PMC",
        file: FILE,
        schema: "openaccess.papers",
        versions: &[1],
        url: "https://europepmc.org/RestfulWebService ; https://unpaywall.org/products/api",
        licence: "OA metadata; full-text reuse rights are separate",
        class: LicenceClass::Unknown,
        scope: "exact PMID/DOI matches to cached papers; source-reported reading links, no full-text redistribution",
    };
    let Some(mut src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    src.coverage.genes = src
        .env
        .records
        .iter()
        .flat_map(|r| strs(r, "genes").map(str::to_owned))
        .collect();
    src.coverage.genes.sort();
    src.coverage.genes.dedup();
    let mut identifiers: HashMap<String, Vec<String>> = HashMap::new();
    for paper in &b.data.papers {
        identifiers.entry(paper.id.clone()).or_default().push(paper.id.clone());
        if let Some(d) = &paper.doi {
            identifiers
                .entry(format!("DOI:{}", doi(d)))
                .or_default()
                .push(paper.id.clone());
        }
    }
    let (mut linked, mut excluded) = (0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        let url = opt(r, "best_full_text_url");
        let reason = if !src.env.header_verified {
            Some("open-access envelope checksum failed")
        } else if r["free_to_read"] != true {
            Some("no source-reported free full text; closed or unknown access")
        } else if !url
            .as_deref()
            .is_some_and(|u| u.starts_with("https://") || u.starts_with("http://"))
        {
            Some("free full-text location has no HTTP(S) URL")
        } else if r["best_location"]["url"].as_str() != url.as_deref()
            || r["best_location"]["evidence"]["record_locator"]
                .as_str()
                .is_none_or(str::is_empty)
            || r["best_location"]["evidence"]["lookup"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            Some("free full-text location lacks consistent provider evidence")
        } else {
            None
        };
        if let Some(reason) = reason {
            super::quarantine::mark(b, rec, reason);
            excluded += 1;
            continue;
        }
        let mut papers = Vec::new();
        for id in strs(r, "identifiers").chain(std::iter::once(s(r, "id"))) {
            let key = if id.starts_with("DOI:") {
                format!("DOI:{}", doi(id))
            } else {
                id.to_owned()
            };
            papers.extend(identifiers.get(&key).into_iter().flatten().cloned());
        }
        papers.sort();
        papers.dedup();
        if papers.is_empty() {
            super::quarantine::mark(b, rec, "no exact PMID or DOI match to an ingested paper");
            excluded += 1;
        }
        for paper in papers {
            // The openaccess record is independently verifiable; include it in the paper's PROV chain.
            if let Some((_, idx)) = b.node(&paper) {
                b.data.papers[idx as usize].records.push(rec);
            }
            b.data.open_access.push(OpenAccess {
                paper,
                url: url.clone().expect("validated URL"),
                status: s(r, "oa_status").into(),
                licence: opt(&r["best_location"], "license"),
                record: rec,
            });
            linked += 1;
        }
    }
    b.param(
        act,
        "excluded_reason",
        "closed/unknown, unverifiable metadata, or no exact paper identifier; source records retained",
    );
    b.finish(act, &[("reading_locations", linked), ("excluded", excluded)]);
    src.finish(b, linked, 0, excluded);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use atlas_core::{Graph, graph::Paper, node::NodeKind};
    use serde_json::{Value, json};

    #[test]
    fn exact_doi_join_closed_records_and_tampering() {
        let atlas = fixtures::atlas();
        let d = fixtures::TempData::new();
        let mut b = fixtures::builder(&atlas);
        b.register("PMID:123", NodeKind::Paper, 0);
        b.data.papers.push(Paper {
            id: "PMID:123".into(),
            title: "fixture".into(),
            journal: "".into(),
            year: None,
            doi: Some("10.1234/ABC".into()),
            review: false,
            records: vec![],
        });
        let row = json!({"id":"DOI:10.1234/abc", "free_to_read":true, "oa_status":"open",
            "best_full_text_url":"https://repository.example/paper", "best_location":{"license":"cc-by",
                "url":"https://repository.example/paper",
                "evidence":{"lookup":"epmc:fixture", "record_locator":"/resultList/result/0"}}});
        d.envelope(
            FILE,
            "openaccess.papers",
            json!([row, {"id":"PMID:123", "free_to_read":false}]),
        );
        ingest(&mut b, d.path()).unwrap();
        let graph = Graph::new(b.data);
        assert_eq!(graph.open_access("PMID:123").count(), 1);
        assert_eq!(graph.data().records.len(), 2);
        assert_eq!(graph.data().quarantine.len(), 1);
        assert!(graph.data().quarantine[0].reason.contains("closed or unknown"));
        assert!(super::super::verify::record(d.path(), &graph, 0).matches);
        let mut envelope: Value = serde_json::from_slice(&std::fs::read(d.path().join(FILE)).unwrap()).unwrap();
        envelope["header"]["sha256"] = json!("tampered");
        d.write(FILE, &serde_json::to_vec(&envelope).unwrap());
        let mut b = fixtures::builder(&atlas);
        ingest(&mut b, d.path()).unwrap();
        assert!(b.data.open_access.is_empty());
        assert_eq!(b.data.records.len(), 2);
    }
}
