//! Identity alignment across sources as SSSOM mapping sets (docs/design/IDENTITY.md).
//!
//! One rule for readers: in our files `skos:exactMatch` means "merge". A row gets it only when a source
//! asserts the cross-reference, both ids are valid and live, the link is 1:1 within its prefix pair and its
//! exact-merge cluster holds at most one id per prefix. Every other asserted-exact row is demoted to
//! `skos:closeMatch` with `asserted_predicate_id` and `conflict` filled, and listed in `<set>.conflicts.tsv`.

pub mod affiliation;
pub mod annotate;
pub mod disease;
pub mod drug;
pub mod gard;
pub mod gene;
pub mod org;
pub mod researchers;
pub mod rxnorm;
pub mod safety;
pub mod trial;
pub mod work;
mod write;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use write::{SetSummary, write_set};

pub const EXACT: &str = "skos:exactMatch";
pub const CLOSE: &str = "skos:closeMatch";
/// Extraction from an external identifier assertion, not a claim about its upstream methodology.
pub const XREF: &str = "semapv:BackgroundKnowledgeBasedMatching";
pub const LEXICAL: &str = "semapv:LexicalMatching";
pub const TOOL: &str = "atlas-discovery align";

/// One input file, as a PROV entity.
#[derive(Clone, Debug, Serialize)]
pub struct Input {
    pub role: String,
    pub path: String,
    pub url: String,
    pub version: String,
    pub sha256: String,
    pub bytes: u64,
    pub license: String,
    pub retrieved_at: Option<String>,
}

impl Input {
    /// Hash the file (streamed) and take URL/version/licence from the download manifest when present.
    pub fn from_file(role: &str, path: &Path, url: &str, version: &str, license: &str) -> Result<Self> {
        let (sha256, bytes) = sha256_file(path)?;
        let mut input = Input {
            role: role.into(),
            path: path.display().to_string().replace('\\', "/"),
            url: url.into(),
            version: version.into(),
            sha256,
            bytes,
            license: license.into(),
            retrieved_at: None,
        };
        if let Some(m) = path.parent().map(|p| p.join("manifest.json")).filter(|m| m.exists()) {
            let v: Value = serde_json::from_slice(&std::fs::read(&m)?)?;
            if let Some(u) = v["url"].as_str() {
                input.url = u.into();
            }
            if let Some(s) = v["version"].as_str() {
                input.version = s.into();
            }
            if let Some(l) = v["license"].as_str() {
                input.license = l.into();
            }
            let meta = v["entities"]
                .as_array()
                .and_then(|es| {
                    es.iter()
                        .find(|e| e["filename"].as_str() == path.file_name().and_then(|s| s.to_str()))
                })
                .unwrap_or(&v);
            input.retrieved_at = meta["retrieved_at"].as_str().map(String::from);
            if let Some(u) = meta["source_url"].as_str() {
                input.url = u.into();
            }
            if let Some(v) = meta["version"].as_str() {
                input.version = v.into();
            }
            if let Some(l) = meta["license"].as_str() {
                input.license = l.into();
            }
        }
        Ok(input)
    }
}

/// One mapping row. `predicate` is the final predicate; `asserted` what the source asserted.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub subject_id: String,
    pub subject_label: String,
    pub predicate: String,
    pub object_id: String,
    pub object_label: String,
    pub justification: String,
    pub confidence: f32,
    pub cardinality: String,
    pub subject_source: String,
    pub object_source: String,
    pub comment: String,
    pub asserted: String,
    pub conflict: String,
    pub evidence_url: String,
    pub input: usize,
    pub locator: String,
    /// Original method is retained separately from the extraction activity.
    pub upstream_justification: String,
}

impl Row {
    /// A source-asserted identifier cross-reference; exact until the checks say otherwise.
    pub fn xref(subject: &str, object: &str, input: usize, locator: impl Into<String>) -> Self {
        Row {
            subject_id: subject.into(),
            predicate: EXACT.into(),
            asserted: EXACT.into(),
            object_id: object.into(),
            justification: XREF.into(),
            confidence: 1.0,
            input,
            locator: locator.into(),
            ..Default::default()
        }
    }
    /// A typed, non-merging link (candidate, granularity, domain relation).
    pub fn link(subject: &str, predicate: &str, object: &str, input: usize, locator: impl Into<String>) -> Self {
        Row {
            predicate: predicate.into(),
            asserted: predicate.into(),
            ..Row::xref(subject, object, input, locator)
        }
    }
    pub fn is_exact(&self) -> bool {
        self.predicate == EXACT
    }
    fn demote(&mut self, kind: &str) {
        if self.is_exact() {
            self.predicate = CLOSE.into();
        }
        if self.conflict.is_empty() {
            self.conflict = kind.into();
        } else if !self.conflict.split(';').any(|k| k == kind) {
            self.conflict = format!("{};{kind}", self.conflict);
        }
    }
}

pub fn prefix(curie: &str) -> &str {
    curie.split_once(':').map_or(curie, |(p, _)| p)
}

/// A mapping set before writing.
#[derive(Debug, Default, Clone)]
pub struct MappingSet {
    pub id: String,
    pub description: String,
    pub license: String,
    pub inputs: Vec<Input>,
    pub rows: Vec<Row>,
    pub curie_map: BTreeMap<String, String>,
    /// reason → (count, up to 5 example locators)
    pub excluded: BTreeMap<String, (u64, Vec<String>)>,
    pub exclusion_ledger: Vec<Exclusion>,
    pub parameters: Value,
    pub notes: Vec<String>,
    pub extra: BTreeMap<String, Value>,
}

impl MappingSet {
    pub fn exclude(&mut self, reason: &str, locator: impl Into<String>) {
        self.exclude_from(None, reason, locator);
    }
    pub fn exclude_from(&mut self, input: Option<usize>, reason: &str, locator: impl Into<String>) {
        let locator = locator.into();
        self.exclusion_ledger.push(Exclusion {
            input,
            reason: reason.into(),
            locator: locator.clone(),
        });
        let e = self.excluded.entry(reason.into()).or_default();
        e.0 += 1;
        if e.1.len() < 5 {
            e.1.push(locator);
        }
    }
    pub fn add_input(&mut self, input: Input) -> usize {
        self.inputs.push(input);
        self.inputs.len() - 1
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Exclusion {
    pub input: Option<usize>,
    pub reason: String,
    pub locator: String,
}

/// Cardinality per prefix pair over asserted-exact rows; demote everything that is not 1:1.
pub fn check_cardinality(set: &mut MappingSet) {
    let mut fwd: HashMap<(String, String), u32> = HashMap::new();
    let mut back: HashMap<(String, String), u32> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for r in set.rows.iter().filter(|r| r.asserted == EXACT) {
        if !seen.insert((r.subject_id.clone(), r.object_id.clone())) {
            continue;
        }
        *fwd.entry((r.subject_id.clone(), prefix(&r.object_id).into()))
            .or_default() += 1;
        *back
            .entry((r.object_id.clone(), prefix(&r.subject_id).into()))
            .or_default() += 1;
    }
    for r in set.rows.iter_mut().filter(|r| r.asserted == EXACT) {
        let f = fwd[&(r.subject_id.clone(), prefix(&r.object_id).into())];
        let b = back[&(r.object_id.clone(), prefix(&r.subject_id).into())];
        r.cardinality = match (b > 1, f > 1) {
            (false, false) => "1:1",
            (false, true) => "1:n",
            (true, false) => "n:1",
            (true, true) => "n:n",
        }
        .into();
        match r.cardinality.as_str() {
            "1:n" => r.demote("one_to_many"),
            "n:1" => r.demote("many_to_one"),
            "n:n" => r.demote("many_to_many"),
            _ => {}
        }
    }
}

/// Declared alias spaces: rows from subjects of `subject_prefix` whose only conflict is `kind` (e.g. several
/// ICTRP mirror records of one registration, several FundRef ids of one ROR record) are exact again.
pub fn accept_cardinality(set: &mut MappingSet, subject_prefix: &str, kind: &str, note: &str) -> usize {
    let mut n = 0;
    for r in set.rows.iter_mut() {
        if prefix(&r.subject_id) == subject_prefix && r.conflict == kind {
            r.conflict.clear();
            r.predicate = EXACT.into();
            r.comment.push_str(&format!("; {note}"));
            n += 1;
        }
    }
    n
}

/// Union-find over the remaining exact rows of several sets. A cluster with two ids of one prefix (other
/// than the allowed `multi` prefixes) demotes every exact row touching an id of that prefix in the cluster
/// (`cluster_conflict`); repeated until every cluster holds at most one id per prefix.
pub fn check_clusters(sets: &mut [&mut MappingSet], multi: &[&str]) -> usize {
    check_clusters_ordered(sets, multi, &[])
}

/// As `check_clusters`, but per round only the links of the weakest prefix present in a bad cluster are
/// demoted (order `weakest_first`; prefixes not listed count as weakest). Hub links (e.g. MONDO–ORPHA)
/// then survive a disagreement about a peripheral id (e.g. which UMLS CUI), which stays flagged.
pub fn check_clusters_ordered(sets: &mut [&mut MappingSet], multi: &[&str], weakest_first: &[&str]) -> usize {
    let mut total = 0;
    loop {
        let n = check_clusters_once(sets, multi, weakest_first);
        total += n;
        if n == 0 {
            return total;
        }
    }
}

fn check_clusters_once(sets: &mut [&mut MappingSet], multi: &[&str], weakest_first: &[&str]) -> usize {
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut id_of = |s: &str, parent: &mut Vec<usize>| -> usize {
        *ids.entry(s.to_string()).or_insert_with(|| {
            parent.push(parent.len());
            parent.len() - 1
        })
    };
    let mut edges = Vec::new();
    for (si, set) in sets.iter().enumerate() {
        for (ri, r) in set.rows.iter().enumerate().filter(|(_, r)| r.is_exact()) {
            let a = id_of(&r.subject_id, &mut parent);
            let b = id_of(&r.object_id, &mut parent);
            edges.push((si, ri, a));
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra] = rb;
            }
        }
    }
    let mut members: HashMap<usize, BTreeMap<String, u32>> = HashMap::new();
    for (id, &ix) in &ids {
        let root = find(&mut parent, ix);
        *members
            .entry(root)
            .or_default()
            .entry(prefix(id).to_string())
            .or_default() += 1;
    }
    // root → prefixes that occur more than once in that cluster
    let bad: HashMap<usize, Vec<String>> = members
        .into_iter()
        .filter_map(|(r, m)| {
            let dup: Vec<String> = m
                .iter()
                .filter(|(p, c)| **c > 1 && !multi.contains(&p.as_str()))
                .map(|(p, _)| p.clone())
                .collect();
            if dup.is_empty() || weakest_first.is_empty() {
                return (!dup.is_empty()).then_some((r, dup));
            }
            // Ordered mode: cut the weakest terminology present in the cluster, duplicated or not
            // (it may be the bridge that joins two hub ids).
            let weakest = m
                .keys()
                .filter(|p| !multi.contains(&p.as_str()))
                .min_by_key(|p| weakest_first.iter().position(|w| w == *p).map_or(0, |i| i + 1))
                .cloned();
            weakest.map(|w| (r, vec![w]))
        })
        .collect();
    let mut demoted = 0;
    for (si, ri, a) in edges {
        let root = find(&mut parent, a);
        if let Some(dup) = bad.get(&root) {
            let r = &mut sets[si].rows[ri];
            let touches = dup
                .iter()
                .any(|p| prefix(&r.subject_id) == p || prefix(&r.object_id) == p);
            if touches {
                r.demote("cluster_conflict");
                r.comment
                    .push_str(&format!("; exact links would join several {} ids", dup.join("/")));
                demoted += 1;
            }
        }
    }
    demoted
}

/// Connected components over the exact rows (for dedupe reports): id → cluster members.
pub fn clusters(rows: &[Row]) -> Vec<Vec<String>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in rows.iter().filter(|r| r.is_exact()) {
        adj.entry(&r.subject_id).or_default().push(&r.object_id);
        adj.entry(&r.object_id).or_default().push(&r.subject_id);
    }
    let mut seen = std::collections::HashSet::new();
    let mut keys: Vec<&str> = adj.keys().copied().collect();
    keys.sort();
    let mut out = Vec::new();
    for k in keys {
        if !seen.insert(k) {
            continue;
        }
        let mut comp = vec![k.to_string()];
        let mut stack = vec![k];
        while let Some(x) = stack.pop() {
            for &y in &adj[x] {
                if seen.insert(y) {
                    comp.push(y.to_string());
                    stack.push(y);
                }
            }
        }
        comp.sort();
        out.push(comp);
    }
    out
}

/// Validate new exact links against the already published source sets without rewriting them.
/// `replaced_prefix` is a terminology whose old links are superseded by this round (e.g. GARD).
pub fn check_background(set: &mut MappingSet, data: &Path, names: &[&str], replaced_prefix: &str) -> Result<()> {
    let mut background = MappingSet::default();
    let mut granular = Vec::new();
    for name in names {
        let p = data.join("cache/mappings").join(format!("{name}.sssom.tsv"));
        if !p.exists() {
            continue;
        }
        set.add_input(Input::from_file(
            "existing identity constraints (not new evidence)",
            &p,
            &format!("https://w3id.org/rare-atlas/mappings/{name}"),
            "pinned-existing-set",
            "CC-BY-4.0",
        )?);
        let text: String = lines(&p)?
            .lines()
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let mut rdr = csv::ReaderBuilder::new().delimiter(b'\t').from_reader(text.as_bytes());
        let h = rdr.headers()?.clone();
        let col = |n: &str| h.iter().position(|s| s == n).context("SSSOM constraint column absent");
        let (s, o, pred) = (col("subject_id")?, col("object_id")?, col("predicate_id")?);
        for row in rdr.records() {
            let row = row?;
            let (a, b, p) = (&row[s], &row[o], &row[pred]);
            if replaced_prefix == "GARD" && (prefix(a) == replaced_prefix || prefix(b) == replaced_prefix) {
                continue;
            }
            if p == EXACT {
                background.rows.push(Row::xref(a, b, 0, "existing constraint"));
            }
            if matches!(p, "skos:broadMatch" | "skos:narrowMatch" | "skos:relatedMatch") {
                granular.push((a.to_string(), b.to_string()));
            }
        }
    }
    // New terminology is the weakest bridge; preserve the existing source-supported graph.
    let mut order: Vec<&str> = vec![replaced_prefix];
    let other: std::collections::BTreeSet<String> = background
        .rows
        .iter()
        .flat_map(|r| [prefix(&r.subject_id).to_string(), prefix(&r.object_id).to_string()])
        .collect();
    order.extend(other.iter().map(String::as_str));
    check_clusters_ordered(&mut [&mut *set, &mut background], &[], &order);
    loop {
        let mut rows = background.rows.clone();
        rows.extend(set.rows.iter().cloned());
        let components = clusters(&rows);
        let mut by_id = HashMap::new();
        for (i, c) in components.iter().enumerate() {
            for id in c {
                by_id.insert(id.as_str(), i);
            }
        }
        let bad: std::collections::HashSet<usize> = granular
            .iter()
            .filter_map(|(a, b)| {
                let x = by_id.get(a.as_str())?;
                (Some(x) == by_id.get(b.as_str())).then_some(*x)
            })
            .collect();
        let mut demoted = 0;
        for row in set.rows.iter_mut().filter(|r| r.is_exact()) {
            if by_id.get(row.subject_id.as_str()).is_some_and(|i| bad.contains(i)) {
                row.demote("cross_source");
                row.comment
                    .push_str("; would collapse an existing source granularity/related relation");
                demoted += 1;
            }
        }
        if demoted == 0 {
            break;
        }
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((format!("{:x}", h.finalize()), n))
}

/// Line reader that transparently gunzips `.gz` files (streamed, never whole).
pub fn lines(path: &Path) -> Result<Box<dyn BufRead>> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(if path.extension().is_some_and(|e| e == "gz") {
        Box::new(BufReader::with_capacity(1 << 20, flate2::read::MultiGzDecoder::new(f)))
    } else {
        Box::new(BufReader::with_capacity(1 << 20, f))
    })
}

/// The newest `data/raw/<source>/<version>/<file>` (versions sort lexically).
pub fn latest(data: &Path, source: &str, file: &str) -> Result<PathBuf> {
    let dir = data.join("raw").join(source);
    let mut versions: Vec<_> = std::fs::read_dir(&dir)
        .with_context(|| format!("no download in {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join(file).exists())
        .collect();
    versions.sort();
    versions
        .pop()
        .map(|p| p.join(file))
        .with_context(|| format!("{file} not in {}", dir.display()))
}

pub fn today() -> String {
    // UTC date from the system clock without extra dependencies.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Common curie_map entries; each set adds its own.
pub fn base_curies() -> BTreeMap<String, String> {
    [
        ("skos", "http://www.w3.org/2004/02/skos/core#"),
        ("semapv", "https://w3id.org/semapv/vocab/"),
        ("RO", "http://purl.obolibrary.org/obo/RO_"),
        ("rda", "https://w3id.org/rare-atlas/vocab/"),
        ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

/// Run named sets (or all) against a data root; returns per-set summaries.
pub fn run(data: &Path, names: &[String]) -> Result<Vec<SetSummary>> {
    let out = std::env::var("RARE_ATLAS_ALIGN_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            data.join("cache/align")
                .join(format!("staged-{}", now_utc().replace(':', "-")))
        });
    anyhow::ensure!(
        out != data.join("cache/mappings"),
        "round3 writes must be staged; atomic reader migration and review precede installation"
    );
    std::fs::create_dir_all(&out)?;
    if let Ok(active) = data.join("cache/mappings").canonicalize() {
        anyhow::ensure!(
            out.canonicalize()? != active,
            "staging must not resolve to the active mappings directory"
        );
    }
    let all = [
        "disease",
        "gene",
        "trial",
        "org",
        "work",
        "drug",
        "researchers",
        "gard",
        "rxnorm",
        "affiliation",
    ];
    let wanted: Vec<&str> = if names.is_empty() || names.iter().any(|n| n == "all") {
        all.to_vec()
    } else {
        names.iter().map(String::as_str).collect()
    };
    let mut summaries = Vec::new();
    for name in wanted {
        let started = now_utc();
        let t = std::time::Instant::now();
        let sets = match name {
            "disease" => disease::build(data)?,
            "gene" => gene::build(data)?,
            "trial" => trial::build(data, &out)?,
            "org" => org::build(data)?,
            "work" => work::build(data)?,
            "drug" => drug::build(data)?,
            "researchers" => researchers::build(data)?,
            "gard" => gard::build(data)?,
            "rxnorm" => rxnorm::build(data)?,
            "affiliation" => affiliation::build(data)?,
            other => anyhow::bail!("unknown set {other}; use {all:?}"),
        };
        for set in &sets {
            summaries.push(write_set(set, &out, &started, t.elapsed().as_millis())?);
        }
    }
    Ok(summaries)
}
