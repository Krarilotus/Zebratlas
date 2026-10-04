//! Genes: HGNC ↔ NCBIGene ↔ ENSEMBL as identities, HGNC → UniProtKB as "has gene product" (never a merge).
//! HGNC's cross-references are checked against NCBI Gene's own `dbXrefs`; disagreement is a conflict.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use super::*;

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (k, v) in [
        ("HGNC", "http://identifiers.org/hgnc/"),
        ("NCBIGene", "http://identifiers.org/ncbigene/"),
        ("ENSEMBL", "http://identifiers.org/ensembl/"),
        ("UniProtKB", "http://identifiers.org/uniprot/"),
    ] {
        m.insert(k.into(), v.into());
    }
    m
}

fn clean(s: &str) -> &str {
    s.trim().trim_matches('"')
}

/// NCBI GeneID → (symbol, HGNC ids, Ensembl ids, line).
type NcbiIndex = HashMap<String, (String, Vec<String>, Vec<String>, usize)>;

fn read_ncbi(path: &Path) -> Result<NcbiIndex> {
    let mut out = HashMap::new();
    for (n, line) in lines(path)?.lines().enumerate() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 || f[0] != "9606" {
            continue;
        }
        let mut hgnc = vec![];
        let mut ens = vec![];
        for x in f[5].split('|') {
            if let Some(h) = x.strip_prefix("HGNC:") {
                hgnc.push(h.to_string()); // "HGNC:5"
            } else if let Some(e) = x.strip_prefix("Ensembl:") {
                ens.push(format!("ENSEMBL:{e}"));
            }
        }
        out.insert(f[1].to_string(), (f[2].to_string(), hgnc, ens, n + 1));
    }
    Ok(out)
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let hgnc_path = data.join("raw").join("hgnc_complete_set.txt");
    let ncbi_path = latest(data, "ncbi-gene-info", "Homo_sapiens.gene_info.gz")?;
    let mut set = MappingSet {
        id: "gene-xrefs".into(),
        description: "Human genes: HGNC = NCBIGene = ENSEMBL (identity, cross-checked HGNC vs NCBI Gene), HGNC has gene product UniProtKB (typed link)".into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let i_h = set.add_input(Input::from_file(
        "HGNC complete set",
        &hgnc_path,
        "https://storage.googleapis.com/public-download-files/hgnc/tsv/tsv/hgnc_complete_set.txt",
        "file modified 2026-10-03 (atlas raw copy)",
        "CC0-1.0",
    )?);
    let i_n = set.add_input(Input::from_file(
        "NCBI Gene gene_info (human)",
        &ncbi_path,
        "",
        "",
        "public-domain",
    )?);
    let ncbi = read_ncbi(&ncbi_path)?;
    // NCBI's reverse view: HGNC id → GeneIDs and HGNC id → Ensembl ids asserted by NCBI.
    let mut ncbi_by_hgnc: HashMap<String, Vec<String>> = HashMap::new();
    for (gid, (_, hs, _, _)) in &ncbi {
        for h in hs {
            ncbi_by_hgnc.entry(h.clone()).or_default().push(gid.clone());
        }
    }
    let mut seen_hgnc = std::collections::HashSet::new();
    let mut header: Vec<String> = vec![];
    let mut stats: BTreeMap<&str, u64> = BTreeMap::new();
    for (n, line) in lines(&hgnc_path)?.lines().enumerate() {
        let line = line?;
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
                .map(|s| clean(s))
                .unwrap_or("")
        };
        let (h, sym, status) = (col("hgnc_id"), col("symbol"), col("status"));
        let loc = format!("line {} ({h})", n + 1);
        if status != "Approved" {
            set.exclude_from(Some(i_h), &format!("HGNC status {status}"), loc);
            continue;
        }
        seen_hgnc.insert(h.to_string());
        let url = format!("https://www.genenames.org/data/gene-symbol-report/#!/hgnc_id/{h}");
        let mk = |mut r: Row, src: &str| {
            r.subject_label = sym.into();
            r.subject_source = "infores:hgnc".into();
            r.object_source = src.into();
            r.evidence_url = url.clone();
            r
        };
        // NCBI Gene
        let entrez = col("entrez_id");
        if !entrez.is_empty() {
            let mut r = mk(
                Row::xref(h, &format!("NCBIGene:{entrez}"), i_h, loc.clone()),
                "infores:ncbi-gene",
            );
            match ncbi.get(entrez) {
                Some((s, hs, _, _)) if hs.iter().any(|x| x == h) => {
                    r.object_label = s.clone();
                    r.comment = "HGNC entrez_id; NCBI Gene dbXrefs confirm".into();
                    *stats.entry("ncbigene_reciprocal").or_default() += 1;
                }
                Some((s, hs, _, ln)) => {
                    r.object_label = s.clone();
                    r.demote("cross_source");
                    r.comment = format!("HGNC entrez_id, but NCBI Gene line {ln} cross-references {hs:?}");
                }
                None => {
                    r.demote("status_unknown");
                    r.comment =
                        "HGNC entrez_id absent from human gene_info; retirement unverified without gene_history".into();
                }
            }
            set.rows.push(r);
        }
        // Ensembl
        let ens = col("ensembl_gene_id");
        if !ens.is_empty() {
            let e = format!("ENSEMBL:{ens}");
            let mut r = mk(Row::xref(h, &e, i_h, loc.clone()), "infores:ensembl");
            let ncbi_ens: Vec<&String> = ncbi_by_hgnc
                .get(h)
                .into_iter()
                .flatten()
                .filter_map(|g| ncbi.get(g))
                .flat_map(|(_, _, es, _)| es)
                .collect();
            if ncbi_ens.iter().any(|x| **x == e) {
                r.comment = "HGNC ensembl_gene_id; NCBI Gene dbXrefs confirm".into();
                *stats.entry("ensembl_confirmed_by_ncbi").or_default() += 1;
            } else if ncbi_ens.is_empty() {
                r.comment = "HGNC ensembl_gene_id; NCBI Gene gives no Ensembl id (no contradiction)".into();
            } else {
                r.demote("cross_source");
                r.comment = format!("HGNC ensembl_gene_id, but NCBI Gene gives {ncbi_ens:?}");
            }
            set.rows.push(r);
        }
        // UniProtKB: gene product, never identity.
        for acc in col("uniprot_ids").split('|').map(str::trim).filter(|s| !s.is_empty()) {
            let mut r = mk(
                Row::link(h, "RO:0002205", &format!("UniProtKB:{acc}"), i_h, loc.clone()),
                "infores:uniprot",
            );
            r.comment =
                "HGNC uniprot_ids: the protein is a product of the gene (RO:0002205), not the same entity".into();
            set.rows.push(r);
        }
    }
    // NCBI-only assertions: NCBI cross-references an approved HGNC id that HGNC does not map back.
    let hgnc_entrez: std::collections::HashSet<(String, String)> = set
        .rows
        .iter()
        .filter(|r| r.object_id.starts_with("NCBIGene:"))
        .map(|r| (r.subject_id.clone(), r.object_id.clone()))
        .collect();
    let hgnc_has_entrez: std::collections::HashSet<String> = hgnc_entrez.iter().map(|(h, _)| h.clone()).collect();
    let mut ncbi_only = 0;
    let mut gids: Vec<_> = ncbi.keys().cloned().collect();
    gids.sort();
    for gid in gids {
        let (sym, hs, _, ln) = &ncbi[&gid];
        for h in hs {
            let g = format!("NCBIGene:{gid}");
            if hgnc_entrez.contains(&(h.clone(), g.clone())) {
                continue;
            }
            let mut r = Row::xref(h, &g, i_n, format!("line {ln} (GeneID {gid})"));
            r.object_label = sym.clone();
            r.subject_source = "infores:hgnc".into();
            r.object_source = "infores:ncbi-gene".into();
            r.evidence_url = format!("https://www.ncbi.nlm.nih.gov/gene/{gid}");
            if !seen_hgnc.contains(h) {
                r.demote("retired");
                r.comment = "NCBI Gene cross-references an HGNC id that is not approved in the HGNC set".into();
            } else if hgnc_has_entrez.contains(h) {
                r.demote("cross_source");
                r.comment = "asserted by NCBI Gene only; HGNC maps this HGNC id to another GeneID".into();
            } else {
                r.confidence = 0.95;
                r.comment = "asserted by NCBI Gene dbXrefs; HGNC gives no entrez_id (no contradiction)".into();
            }
            ncbi_only += 1;
            set.rows.push(r);
        }
    }
    check_cardinality(&mut set);
    let demoted = check_clusters(&mut [&mut set], &[]);
    set.parameters = json!({"rule": "exact = HGNC-asserted NCBIGene/ENSEMBL id, not contradicted by NCBI Gene, 1:1 per prefix pair, one id per prefix per cluster; UniProtKB via RO:0002205 only"});
    set.extra.insert("stats".into(), json!(stats));
    set.extra.insert("ncbi_only_rows".into(), json!(ncbi_only));
    set.extra.insert("cluster_conflict_rows".into(), json!(demoted));
    Ok(vec![set])
}
