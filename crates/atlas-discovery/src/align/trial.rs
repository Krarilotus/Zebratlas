//! Trials: the same study across registries. Inputs: every CT.gov record's declared secondary ids (all
//! 605k studies), the ICTRP/CTIS/EUCTR slice caches. A registry id is recognised by its syntax; a link is
//! exact when a registry record declares it and it stays 1:1 and cluster-consistent. Shared sponsor
//! protocol numbers are candidates only. Writes `trials-dedupe.tsv` + `trials-dedupe.json` (clusters).

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::{Value, json};

use super::*;

/// (CURIE prefix, pattern with one capture = local id, URL template).
static REGISTRIES: LazyLock<Vec<(&'static str, Regex, &'static str)>> = LazyLock::new(|| {
    [
        ("NCT", r"\b(NCT\d{8})\b", "https://clinicaltrials.gov/study/{}"),
        ("EUCT", r"\b(20\d{2}-5\d{5}-\d{2}-\d{2})\b", "https://euclinicaltrials.eu/search-for-clinical-trials/?lang=en&EUCT={}"),
        ("EUDRACT", r"\b((?:19|20)\d{2}-\d{6}-\d{2})\b", "https://www.clinicaltrialsregister.eu/ctr-search/search?query={}"),
        ("ISRCTN", r"\bISRCTN ?(\d{8})\b", "https://www.isrctn.com/ISRCTN{}"),
        ("DRKS", r"\b(DRKS\d{8})\b", "https://drks.de/search/en/trial/{}"),
        ("CHICTR", r"\b(ChiCTR-?(?:[A-Z]{2,4}-)?\d{6,10})\b", "https://www.chictr.org.cn/searchprojEN.html?regno={}"),
        ("ANZCTR", r"\b(ACTRN\d{14})\b", "https://www.anzctr.org.au/ACTRN={}"),
        ("CTRI", r"\b(CTRI/\d{4}/\d{2,3}/\d{6})\b", "https://ctri.nic.in/Clinicaltrials/advsearch.php?q={}"),
        ("JRCT", r"\b(jRCT[a-z]?\d{9,10})\b", "https://jrct.mhlw.go.jp/en-latest-detail/{}"),
        ("JAPICCTI", r"\bJapicCTI-? ?(\d{6})\b", "https://www.clinicaltrials.jp/cti-user/trial/ShowDirect.jsp?japicId=JapicCTI-{}"),
        ("UMIN", r"\bUMIN ?(\d{9})\b", "https://center6.umin.ac.jp/cgi-open-bin/ctr_e/ctr_view.cgi?recptno=R{}"),
        ("NTR", r"\b(NTR\d{3,5})\b", "https://onderzoekmetmensen.nl/en/trial/{}"),
        ("OMON", r"\b(NL-OMON\d+)\b", "https://onderzoekmetmensen.nl/en/trial/{}"),
        ("IRCT", r"\b(IRCT\d{8,14}N\d{1,3})\b", "https://irct.behdasht.gov.ir/search/result?query={}"),
        ("KCT", r"\b(KCT\d{7})\b", "https://cris.nih.go.kr/cris/search/detailSearch.do?search_lang=E&search_page=M&pageSize=10&page=undefined&seq=&status=5&seq_group=&search_word={}"),
        ("PACTR", r"\b(PACTR\d{15})\b", "https://pactr.samrc.ac.za/TrialDisplay.aspx?TrialID={}"),
        ("TCTR", r"\b(TCTR\d{11})\b", "https://www.thaiclinicaltrials.org/show/{}"),
        ("SLCTR", r"\b(SLCTR/\d{4}/\d{3})\b", "https://slctr.lk/trials/{}"),
        ("RBR", r"\b(RBR-[0-9a-z]{6,8})\b", "https://ensaiosclinicos.gov.br/rg/{}"),
        ("RPCEC", r"\b(RPCEC\d{8})\b", "https://rpcec.sld.cu/en/trials/{}"),
        ("LBCTR", r"\b(LBCTR\d{10})\b", "https://lbctr.moph.gov.lb/Trials/Details/{}"),
    ]
    .into_iter()
    .map(|(p, re, url)| (p, Regex::new(re).unwrap(), url))
    .collect()
});

/// All registry ids recognised in a free-text id field. EU CT numbers are not also read as EudraCT.
pub fn recognise(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (p, re, _) in REGISTRIES.iter() {
        for c in re.captures_iter(value) {
            let m = c.get(0).unwrap();
            if spans.iter().any(|&(a, b)| m.start() < b && a < m.end()) {
                continue;
            }
            spans.push((m.start(), m.end()));
            let local = c.get(1).unwrap().as_str().replace(' ', "");
            let id = format!("{p}:{local}");
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

pub fn url_of(curie: &str) -> String {
    let (p, l) = curie.split_once(':').unwrap_or(("", curie));
    REGISTRIES
        .iter()
        .find(|r| r.0 == p)
        .map(|r| r.2.replace("{}", l))
        .unwrap_or_default()
}

/// ICTRP main ids carry the registry's own id (EUCTR adds the member state, CTIS a prefix).
pub fn ictrp_main(id: &str) -> Option<String> {
    if let Some(rest) = id.strip_prefix("EUCTR") {
        let eudract = rest.get(..14)?;
        return Some(format!("EUDRACT:{eudract}")).filter(|_| recognise(eudract).len() == 1);
    }
    if let Some(rest) = id.strip_prefix("CTIS") {
        return recognise(rest).into_iter().find(|c| c.starts_with("EUCT:"));
    }
    let r = recognise(id);
    (r.len() == 1).then(|| r[0].clone())
}

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (p, _, url) in REGISTRIES.iter() {
        m.insert(p.to_string(), url.replace("{}", ""));
    }
    m.insert(
        "ICTRP".into(),
        "https://trialsearch.who.int/Trial2.aspx?TrialID=".into(),
    );
    m
}

fn protocol_key(v: &str) -> Option<String> {
    let k: String = v
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    let generic = ["NONE", "N/A", "NA", "PROTOCOL", "PILOT", "STUDY", "IRB", "1.0"];
    if k.len() < 6 || k.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.') || generic.contains(&k.as_str()) {
        return None;
    }
    if !recognise(&k).is_empty() {
        return None; // a registry id, handled as a cross-reference
    }
    Some(k)
}

pub fn build(data: &Path, out: &Path) -> Result<Vec<MappingSet>> {
    let pages = latest(data, "ctgov-ids", "pages.jsonl.gz")?;
    let reg = data.join("cache").join("registries");
    let mut set = MappingSet {
        id: "trial-xrefs".into(),
        description: "The same clinical study across registries (CT.gov declared secondary ids for all studies; WHO ICTRP, EU CTIS, EU CTR slice caches); shared sponsor protocol numbers as candidates".into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let i_ct = set.add_input(Input::from_file(
        "CT.gov ids (API v2, all studies)",
        &pages,
        "",
        "",
        "public-domain",
    )?);
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut org_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut studies = 0u64;
    for (pn, line) in lines(&pages)?.lines().enumerate() {
        let page: Value = serde_json::from_str(&line?)?;
        for (sn, s) in page["studies"].as_array().into_iter().flatten().enumerate() {
            studies += 1;
            let m = &s["protocolSection"]["identificationModule"];
            let Some(nct) = m["nctId"].as_str() else { continue };
            let subject = format!("NCT:{nct}");
            let title = m["briefTitle"].as_str().unwrap_or("").to_string();
            if let Some(k) = m["orgStudyIdInfo"]["id"].as_str().and_then(protocol_key) {
                org_ids.entry(k).or_default().push(nct.to_string());
            }
            for (xn, x) in m["secondaryIdInfos"].as_array().into_iter().flatten().enumerate() {
                let value = x["id"].as_str().unwrap_or("");
                let ty = x["type"].as_str().unwrap_or("");
                let domain = x["domain"].as_str().unwrap_or("");
                let found: Vec<String> = recognise(value).into_iter().filter(|c| *c != subject).collect();
                let loc = format!("page {} study {sn} ({nct}) secondaryIdInfos[{xn}]", pn + 1);
                if found.is_empty() {
                    set.exclude_from(
                        Some(i_ct),
                        &format!(
                            "secondary id type {} not a recognised registry id",
                            if ty.is_empty() { "(none)" } else { ty }
                        ),
                        loc,
                    );
                    continue;
                }
                if found.len() > 1 {
                    set.exclude_from(
                        Some(i_ct),
                        "secondary id field names several registry ids (ambiguous)",
                        loc,
                    );
                    continue;
                }
                let o = &found[0];
                let typed = matches!((ty, prefix(o)), ("EUDRACT_NUMBER", "EUDRACT") | ("CTIS", "EUCT"))
                    || (ty == "REGISTRY" && !domain.is_empty());
                *by_type.entry(format!("{ty}->{}", prefix(o))).or_default() += 1;
                let mut r = Row::xref(&subject, o, i_ct, loc);
                r.confidence = if typed { 1.0 } else { 0.95 };
                r.subject_label = title.clone();
                r.subject_source = "infores:clinicaltrials".into();
                r.object_source = format!("registry:{}", prefix(o).to_lowercase());
                r.evidence_url = format!("https://clinicaltrials.gov/study/{nct}");
                r.comment = format!(
                    "CT.gov secondary id type={ty}{} value \"{value}\"",
                    if domain.is_empty() {
                        String::new()
                    } else {
                        format!(" domain=\"{domain}\"")
                    }
                );
                set.rows.push(r);
            }
            titles.insert(subject, title);
        }
    }

    // ICTRP records: the main id is the registry's own record; secondary ids are cross-references.
    let ictrp_path = reg.join("ictrp.json");
    if ictrp_path.exists() {
        let i_ic = set.add_input(Input::from_file(
            "WHO ICTRP slice cache",
            &ictrp_path,
            "https://trialsearch.who.int/",
            "2026-10-03",
            "WHO ICTRP terms: non-commercial reuse with attribution",
        )?);
        let v: Value = serde_json::from_slice(&std::fs::read(&ictrp_path)?)?;
        for (k, rec) in v["records"].as_array().into_iter().flatten().enumerate() {
            let id = rec["id"].as_str().unwrap_or("");
            let title = rec["public_title"].as_str().unwrap_or("").to_string();
            let Some(main) = ictrp_main(id) else {
                set.exclude_from(Some(i_ic), "ICTRP main id not recognised", format!("records[{k}] {id}"));
                continue;
            };
            let mut r = Row::xref(&format!("ICTRP:{id}"), &main, i_ic, format!("records[{k}]"));
            r.subject_label = title.clone();
            r.subject_source = "infores:who-ictrp".into();
            r.object_source = format!("registry:{}", prefix(&main).to_lowercase());
            r.evidence_url = rec["url"].as_str().unwrap_or("").into();
            r.comment = "ICTRP record of the registry's own registration (registry naming convention)".into();
            set.rows.push(r);
            for (j, sec) in rec["secondary_ids"].as_array().into_iter().flatten().enumerate() {
                let found: Vec<String> = recognise(sec.as_str().unwrap_or(""))
                    .into_iter()
                    .filter(|c| *c != main)
                    .collect();
                if found.len() != 1 {
                    set.exclude_from(
                        Some(i_ic),
                        "ICTRP secondary id not exactly one recognised registry id",
                        format!("records[{k}].secondary_ids[{j}]"),
                    );
                    continue;
                }
                let mut r = Row::xref(&main, &found[0], i_ic, format!("records[{k}].secondary_ids[{j}]"));
                r.confidence = 0.95;
                r.subject_label = title.clone();
                r.subject_source = "infores:who-ictrp".into();
                r.object_source = format!("registry:{}", prefix(&found[0]).to_lowercase());
                r.evidence_url = rec["url"].as_str().unwrap_or("").into();
                r.comment = format!("ICTRP secondary id \"{}\"", sec.as_str().unwrap_or(""));
                set.rows.push(r);
            }
        }
    }

    // Candidates: an EU CTR sponsor protocol number equal to a CT.gov org study id (never merged).
    let euctr_path = reg.join("euctr.json");
    if euctr_path.exists() {
        let i_eu = set.add_input(Input::from_file(
            "EU CTR slice cache",
            &euctr_path,
            "https://www.clinicaltrialsregister.eu/",
            "2026-10-03",
            "EMA reuse terms",
        )?);
        let v: Value = serde_json::from_slice(&std::fs::read(&euctr_path)?)?;
        for (k, rec) in v["records"].as_array().into_iter().flatten().enumerate() {
            let eud = format!("EUDRACT:{}", rec["id"].as_str().unwrap_or(""));
            let Some(key) = rec["sponsor_protocol"].as_str().and_then(protocol_key) else {
                continue;
            };
            for nct in org_ids.get(&key).into_iter().flatten() {
                let mut r = Row::link(
                    &eud,
                    CLOSE,
                    &format!("NCT:{nct}"),
                    i_eu,
                    format!("records[{k}].sponsor_protocol"),
                );
                r.justification = LEXICAL.into();
                r.confidence = 0.6;
                r.subject_label = rec["title"].as_str().unwrap_or("").into();
                r.object_label = titles.get(&format!("NCT:{nct}")).cloned().unwrap_or_default();
                r.evidence_url = rec["url"].as_str().unwrap_or("").into();
                r.comment =
                    format!("same sponsor protocol number \"{key}\" in EU CTR and CT.gov org study id: candidate");
                set.rows.push(r);
            }
        }
    }

    // Candidates inside CT.gov: two NCT records with the same organisation study id (>=6 chars, not
    // generic), the same lead sponsor and overlapping titles (precision sample showed that an org study id
    // alone mostly pairs unrelated studies).
    let sponsors = read_sponsors(data)?;
    let mut dup_groups = 0u64;
    let mut dropped = 0u64;
    let mut keys: Vec<_> = org_ids.iter().filter(|(_, v)| v.len() == 2).collect();
    keys.sort();
    for (key, ncts) in keys {
        let (a, b) = (format!("NCT:{}", ncts[0]), format!("NCT:{}", ncts[1]));
        let (sa, sb) = (sponsors.get(&a), sponsors.get(&b));
        let (ta, tb) = (
            titles.get(&a).cloned().unwrap_or_default(),
            titles.get(&b).cloned().unwrap_or_default(),
        );
        let overlap = jaccard(&ta, &tb);
        if sa.is_none() || sa != sb || overlap < 0.3 {
            dropped += 1;
            continue;
        }
        dup_groups += 1;
        let mut r = Row::link(&a, CLOSE, &b, i_ct, format!("orgStudyIdInfo.id = \"{key}\""));
        r.justification = LEXICAL.into();
        r.confidence = 0.6;
        r.subject_label = ta;
        r.object_label = tb;
        r.evidence_url = format!("https://clinicaltrials.gov/study/{}", ncts[0]);
        r.comment = format!(
            "same organisation study id \"{key}\", same lead sponsor, title word overlap {overlap:.2} (possible duplicate registration or sub-study): candidate"
        );
        set.rows.push(r);
    }
    set.extra
        .insert("org_study_id_pairs_rejected_sponsor_or_title".into(), json!(dropped));
    let crowded = org_ids.values().filter(|v| v.len() > 2).count();

    for r in set.rows.iter_mut() {
        if r.object_label.is_empty() {
            r.object_label = titles.get(&r.object_id).cloned().unwrap_or_default();
        }
    }
    check_cardinality(&mut set);
    accept_cardinality(
        &mut set,
        "ICTRP",
        "many_to_one",
        "several ICTRP mirror records of one registration (expected)",
    );
    let demoted = check_clusters(&mut [&mut set], &["ICTRP"]);

    // Dedupe report: clusters of exact links = one study, several registrations.
    let slice: HashSet<String> = read_slice(data);
    let comps = clusters(&set.rows);
    let mut w = std::io::BufWriter::new(std::fs::File::create(out.join("trials-dedupe.tsv"))?);
    writeln!(w, "cluster\tsize\tregistries\tmembers\tin_atlas_slice\ttitle")?;
    let mut sizes: BTreeMap<usize, u64> = BTreeMap::new();
    let mut slice_clusters = Vec::new();
    let mut multi_registry = 0u64;
    for (i, c) in comps.iter().enumerate() {
        let regs: std::collections::BTreeSet<&str> = c.iter().map(|x| prefix(x)).filter(|p| *p != "ICTRP").collect();
        if regs.len() > 1 {
            multi_registry += 1;
        }
        *sizes.entry(c.len()).or_default() += 1;
        let title = c.iter().find_map(|x| titles.get(x)).cloned().unwrap_or_default();
        let in_slice = c.iter().any(|x| slice.contains(x));
        if in_slice {
            slice_clusters.push(json!({"members": c, "title": title}));
        }
        writeln!(
            w,
            "trial-cluster-{i}\t{}\t{}\t{}\t{}\t{}",
            c.len(),
            regs.into_iter().collect::<Vec<_>>().join(","),
            c.join(" "),
            in_slice,
            title.replace('\t', " ")
        )?;
    }
    let report = json!({
        "what": "clusters of registrations joined by exact cross-references (one study, several registry records)",
        "studies_read": studies,
        "clusters": comps.len(),
        "clusters_spanning_registries": multi_registry,
        "cluster_size_histogram": sizes,
        "ctgov_org_study_id_candidate_pairs": dup_groups,
        "org_study_ids_shared_by_more_than_two_records_not_linked": crowded,
        "atlas_slice_clusters": slice_clusters,
    });
    std::fs::write(out.join("trials-dedupe.json"), serde_json::to_string_pretty(&report)?)?;
    set.parameters = json!({"recognised_registries": REGISTRIES.iter().map(|r| r.0).collect::<Vec<_>>(),
        "rule": "exact = registry record declares the other registry id (recognised by syntax), 1:1 per prefix pair (ICTRP mirror records may be n:1), one id per registry per cluster",
        "confidence": "1.0 typed field (EUDRACT_NUMBER, CTIS, REGISTRY with domain); 0.95 recognised by syntax in an untyped field; 0.6 shared protocol number candidate",
        "protocol_key": "whitespace removed, upper case, >=6 chars, not only digits/punctuation, not a registry id; CT.gov pairs only when exactly two records share it"});
    set.extra.insert("secondary_id_type_to_registry".into(), json!(by_type));
    set.extra.insert("cluster_conflict_rows".into(), json!(demoted));
    set.notes
        .push("Dedupe clusters: trials-dedupe.tsv / trials-dedupe.json in the same folder.".into());
    Ok(vec![set])
}

/// NCT ids the atlas links to the demo slice (contacts cache), for the dedupe report only.
fn read_slice(data: &Path) -> HashSet<String> {
    let p = data.join("cache").join("contacts").join("ctgov.json");
    let Ok(bytes) = std::fs::read(p) else {
        return HashSet::new();
    };
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    v["records"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["id"].as_str())
        .map(|s| format!("NCT:{s}"))
        .collect()
}

/// Lead sponsor per NCT from the CT.gov snapshot (streamed; normalised lower case).
fn read_sponsors(data: &Path) -> Result<HashMap<String, String>> {
    let p = data.join("cache").join("trials").join("studies.jsonl.gz");
    let mut out = HashMap::new();
    if !p.exists() {
        return Ok(out);
    }
    for line in lines(&p)?.lines() {
        let v: Value = serde_json::from_str(&line?)?;
        if let (Some(n), Some(s)) = (v["nct_id"].as_str(), v["sponsor"].as_str()) {
            out.insert(format!("NCT:{n}"), s.trim().to_lowercase());
        }
    }
    Ok(out)
}

/// Word-set Jaccard similarity of two titles (words of 4+ letters, lower case).
pub fn jaccard(a: &str, b: &str) -> f32 {
    let w = |s: &str| -> HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 4)
            .map(String::from)
            .collect()
    };
    let (x, y) = (w(a), w(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    x.intersection(&y).count() as f32 / x.union(&y).count() as f32
}
