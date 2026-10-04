//! Drugs: CHEMBL.COMPOUND = DRUGBANK (id only) and = UNII through UniChem (identical standard InChI,
//! CC0). The separate round-two `rxnorm-xrefs` set uses public RxNav REST UNII lookups.
//! Coverage of the ChEMBL ids that our caches mention is reported.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use regex::Regex;
use serde_json::json;

use super::*;

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (k, v) in [
        ("CHEMBL.COMPOUND", "https://www.ebi.ac.uk/chembl/compound_report_card/"),
        ("DRUGBANK", "https://go.drugbank.com/drugs/"),
        ("UNII", "https://precision.fda.gov/uniisearch/srs/unii/"),
    ] {
        m.insert(k.into(), v.into());
    }
    m
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let mut set = MappingSet {
        id: "drug-xrefs".into(),
        description: "Compounds: ChEMBL = DrugBank (id only) and ChEMBL = FDA UNII via UniChem (same standard InChI)"
            .into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    for (source, file, prefix_o, src, url) in [
        (
            "unichem-chembl-drugbank",
            "src1src2.txt.gz",
            "DRUGBANK",
            "infores:drugbank",
            "https://go.drugbank.com/drugs/",
        ),
        (
            "unichem-chembl-unii",
            "src1src14.txt.gz",
            "UNII",
            "infores:fda-srs",
            "https://precision.fda.gov/uniisearch/srs/unii/",
        ),
    ] {
        let p = latest(data, source, file)?;
        let i = set.add_input(Input::from_file(&format!("UniChem {source}"), &p, "", "", "CC0-1.0")?);
        for (n, line) in lines(&p)?.lines().enumerate() {
            let line = line?;
            let mut f = line.split('\t');
            let (Some(a), Some(b)) = (f.next(), f.next()) else {
                continue;
            };
            if !a.starts_with("CHEMBL") {
                continue; // header
            }
            let mut r = Row::xref(
                &format!("CHEMBL.COMPOUND:{a}"),
                &format!("{prefix_o}:{}", b.trim()),
                i,
                format!("line {}", n + 1),
            );
            r.subject_source = "infores:chembl".into();
            r.object_source = src.into();
            r.evidence_url =
                format!("https://www.ebi.ac.uk/unichem/compoundsources?type=sourceID&compound={a}&sourceID=1");
            r.comment = format!("UniChem: identical standard InChI ({url}{})", b.trim());
            set.rows.push(r);
        }
    }
    check_cardinality(&mut set);
    let cluster = check_clusters(&mut [&mut set], &[]);

    // Which ChEMBL ids in our caches are covered (for the graph owner; no mapping is created from this).
    let re = Regex::new(r"CHEMBL\d+").unwrap();
    let mut ours: BTreeSet<String> = BTreeSet::new();
    for rel in [
        "repurposing/opentargets.json",
        "repurposing/everycure.json",
        "pipelines/evidence.json",
        "regulatory/signals.json",
    ] {
        let p = data.join("cache").join(rel);
        if let Ok(text) = std::fs::read_to_string(&p) {
            ours.extend(re.find_iter(&text).map(|m| format!("CHEMBL.COMPOUND:{}", m.as_str())));
        }
    }
    let exact_subjects: BTreeSet<&str> = set
        .rows
        .iter()
        .filter(|r| r.is_exact())
        .map(|r| r.subject_id.as_str())
        .collect();
    let covered = ours.iter().filter(|c| exact_subjects.contains(c.as_str())).count();
    set.parameters =
        json!({"rule": "exact = UniChem cross-reference, 1:1 per prefix pair, one id per prefix per cluster"});
    set.notes.push("RxNorm mappings are in the separate rxnorm-xrefs set (public RxNav REST UNII_CODE lookups, no UMLS-licensed download). Exact ChEMBL–UNII links may be chained with its exact UNII–RxCUI ingredient links.".into());
    set.notes
        .push("DrugBank appears as identifiers only (DrugBank data is not openly licensed).".into());
    set.extra.insert("chembl_ids_in_our_caches".into(), json!(ours.len()));
    set.extra
        .insert("of_which_with_an_exact_drugbank_or_unii".into(), json!(covered));
    set.extra.insert("cluster_conflict_rows".into(), json!(cluster));
    Ok(vec![set])
}
