//! Optional research-navigation rules, never clinical entailments or identity rules.
//! The caller supplies a bounded, visibility-filtered ASSERTED slice. NRESE receives
//! only this module's eligibility projection in a separate versioned overlay.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "1.0.0";
pub const CONTRACT: &str = "zebratlas-connection-rules-v1";
pub const NS: &str = "https://w3id.org/rare-disease-atlas/connection/v1#";
const MAX_ASSERTIONS: usize = 2048;
const MAX_NODES: usize = 512;
const MAX_PATHS: usize = 16;
const MAX_JOIN_VISITS: usize = 100_000;
const MAX_CANDIDATES: usize = 128;
/// One ledger covers every copied assertion in positive paths AND negative
/// context. Count before cloning: top_k is applied after candidate construction.
const MAX_OUTPUT_ASSERTIONS: usize = 1024;

#[cfg(test)]
std::thread_local! {
    static COPIED_OUTPUT_ASSERTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn copy_output_assertion(assertion: &Assertion) -> Assertion {
    #[cfg(test)]
    COPIED_OUTPUT_ASSERTIONS.with(|copies| copies.set(copies.get() + 1));
    assertion.clone()
}
#[cfg(test)]
pub(crate) fn copied_output_assertions() -> usize {
    COPIED_OUTPUT_ASSERTIONS.with(std::cell::Cell::get)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Condition,
    Gene,
    Phenotype,
    Pathway,
    Drug,
    Community,
    Asset,
    /// Information objects are not silently relabelled as biomedical referents.
    GeneRecord,
    DiseaseRecord,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub obsolete: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub source_url: String,
    pub source_sha256: String,
    pub record_sha256: String,
    pub locator: String,
    pub retrieved_at: String,
    pub version: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Raw source strings: no conversion of a frequency range to a probability.
    pub frequency: Option<String>,
    pub onset: Option<String>,
    pub sex: Option<String>,
    pub modifiers: Vec<String>,
    pub variant_effect: Option<String>,
    pub target_action: Option<String>,
    pub species: Option<String>,
    pub evidence_code: Option<String>,
    pub aspect: Option<String>,
    pub scope: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Predicate {
    HasPhenotype,
    LacksPhenotype,
    HasCausalGene,
    InPathway,
    Targets,
    ServesGene,
    ModelOfGene,
    ResourceForGene,
}
impl Predicate {
    fn name(self) -> &'static str {
        match self {
            Self::HasPhenotype => "has_phenotype",
            Self::LacksPhenotype => "lacks_phenotype",
            Self::HasCausalGene => "has_causal_gene",
            Self::InPathway => "in_pathway",
            Self::Targets => "targets",
            Self::ServesGene => "serves_gene",
            Self::ModelOfGene => "model_of_gene",
            Self::ResourceForGene => "resource_for_gene",
        }
    }
    fn kinds(self) -> (Kind, Kind) {
        match self {
            Self::HasPhenotype | Self::LacksPhenotype => (Kind::Condition, Kind::Phenotype),
            Self::HasCausalGene => (Kind::Condition, Kind::Gene),
            Self::InPathway => (Kind::Gene, Kind::Pathway),
            Self::Targets => (Kind::Drug, Kind::Gene),
            Self::ServesGene => (Kind::Community, Kind::Gene),
            Self::ModelOfGene | Self::ResourceForGene => (Kind::Asset, Kind::Gene),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    /// Source-row identity, not a merged edge identity. Multiple source rows survive.
    pub id: String,
    pub subject: String,
    pub predicate: Predicate,
    pub object: String,
    pub origin: String,
    pub records: Vec<Record>,
    pub context: Context,
    /// Exact upstream eligibility, never inferred from absence of a NOT qualifier.
    /// Missing fields default false; HOOM 2.6's all-false values stay false.
    #[serde(default)]
    pub eligible_for_positive_inference: bool,
    #[serde(default)]
    pub eligibility: Option<Eligibility>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Eligibility {
    pub policy_id: String,
    pub policy_version: String,
    /// Both rights and assertion semantics need independent, source-bound review.
    pub permission_for_reasoning: bool,
    pub asserted_fact: bool,
    pub receipt: Record,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Distinctiveness {
    pub phenotype: String,
    pub annotated_conditions: u64,
    pub corpus_conditions: u64,
    /// Required corpus receipt; local neighbourhood prevalence is insufficient.
    pub corpus_sha256: String,
    pub record: Record,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub snapshot_sha256: String,
    pub scope: String,
    pub input_complete: bool,
    pub unavailable_sources: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slice {
    pub nodes: Vec<Node>,
    pub assertions: Vec<Assertion>,
    pub distinctiveness: Vec<Distinctiveness>,
    pub coverage: Coverage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    SharedDistinctivePhenotypes,
    SharedCausalGene,
    SharedPathway,
    DrugTargetOverlap,
    CommunityViaGene,
    AssetViaGene,
}
impl Rule {
    pub const ALL: [Self; 6] = [
        Self::SharedDistinctivePhenotypes,
        Self::SharedCausalGene,
        Self::SharedPathway,
        Self::DrugTargetOverlap,
        Self::CommunityViaGene,
        Self::AssetViaGene,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::SharedDistinctivePhenotypes => "CR-PHEN-01",
            Self::SharedCausalGene => "CR-GENE-01",
            Self::SharedPathway => "CR-PATH-01",
            Self::DrugTargetOverlap => "CR-DRUG-01",
            Self::CommunityViaGene => "CR-COMM-01",
            Self::AssetViaGene => "CR-ASSET-01",
        }
    }
    pub fn conclusion(self) -> &'static str {
        match self {
            Self::SharedDistinctivePhenotypes => "possible_shared_phenotypes",
            Self::SharedCausalGene => "possible_shared_gene",
            Self::SharedPathway => "possible_shared_pathway",
            Self::DrugTargetOverlap => "possible_target_research_lead",
            Self::CommunityViaGene => "possible_community_research_lead",
            Self::AssetViaGene => "possible_asset_research_lead",
        }
    }
    pub fn limitation(self) -> &'static str {
        match self {
            Self::SharedDistinctivePhenotypes => {
                "Two exact distinctive annotations in this corpus; no diagnosis, equivalence or patient-level compatibility. Qualifiers and differences require review."
            }
            Self::SharedCausalGene => {
                "Shared causal-gene annotation; variant effect, alleles and phenotype may differ. No disease equivalence."
            }
            Self::SharedPathway => {
                "Direct pathway membership only; does not establish a shared disease mechanism or transferable assay."
            }
            Self::DrugTargetOverlap => {
                "Target overlap only; action, variant effect, exposure and assay evidence require review. No treatment benefit or clinical recommendation."
            }
            Self::CommunityViaGene => {
                "Source asserts gene scope; exact condition coverage, contact route and willingness remain unestablished."
            }
            Self::AssetViaGene => {
                "Source asserts gene annotation; suitability, species, availability, access and permission remain unestablished."
            }
        }
    }
    fn body(self) -> &'static str {
        match self {
            Self::SharedDistinctivePhenotypes => {
                "?s c:eligible_phenotype ?p . ?o c:eligible_phenotype ?p . ?s c:eligible_phenotype ?q . ?o c:eligible_phenotype ?q . ?p log:notEqualTo ?q . ?s log:notEqualTo ?o ."
            }
            Self::SharedCausalGene => "?s c:has_causal_gene ?g . ?o c:has_causal_gene ?g . ?s log:notEqualTo ?o .",
            Self::SharedPathway => {
                "?s c:has_causal_gene ?g . ?g c:in_pathway ?p . ?o c:has_causal_gene ?h . ?h c:in_pathway ?p . ?s log:notEqualTo ?o ."
            }
            Self::DrugTargetOverlap => "?s c:has_causal_gene ?g . ?o c:targets ?g .",
            Self::CommunityViaGene => "?s c:has_causal_gene ?g . ?o c:serves_gene ?g .",
            Self::AssetViaGene => "?s c:has_causal_gene ?g . ?o c:eligible_asset_gene ?g .",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switches {
    /// All rules default off. A validated operator overlay is a separate gate.
    #[serde(default)]
    pub enabled: BTreeSet<Rule>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub id: String,
    pub subject: String,
    pub relation: &'static str,
    pub object: String,
    pub origin: &'static str,
    pub execution: &'static str,
    pub rule_id: &'static str,
    pub rule_version: &'static str,
    pub rules_sha256: String,
    /// Every path is ordered and contains entire asserted records/qualifiers.
    pub witness_paths: Vec<Vec<Assertion>>,
    pub negative_evidence: Vec<Assertion>,
    pub distinctiveness: Vec<Distinctiveness>,
    pub witnesses_complete: bool,
    pub limitation: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub contract: &'static str,
    pub enabled_rules: Vec<Rule>,
    pub rules_sha256: String,
    pub candidates: Vec<Candidate>,
    pub coverage: Coverage,
    pub eligible_assertions: usize,
    pub rejected_assertions: Vec<String>,
    pub result_truncated: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Projection {
    pub turtle: String,
    pub n3: String,
    pub rules_sha256: String,
    pub snapshot_sha256: String,
}

pub(crate) fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
pub(crate) fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(|c| c.is_control())
}
fn valid_record(r: &Record) -> bool {
    let url = reqwest::Url::parse(&r.source_url);
    valid_id(&r.id)
        && is_hash(&r.source_sha256)
        && is_hash(&r.record_sha256)
        && !r.locator.is_empty()
        && r.locator.len() <= 1024
        && !r.retrieved_at.is_empty()
        && r.retrieved_at.len() <= 64
        && r.source_url.len() <= 2048
        && r.version.as_ref().is_none_or(|v| v.len() <= 256)
        && url.is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
        })
}
fn valid_context(c: &Context) -> bool {
    [
        &c.frequency,
        &c.onset,
        &c.sex,
        &c.variant_effect,
        &c.target_action,
        &c.species,
        &c.evidence_code,
        &c.aspect,
        &c.scope,
    ]
    .into_iter()
    .all(|s| s.as_ref().is_none_or(|s| s.len() <= 512))
        && c.modifiers.len() <= 16
        && c.modifiers.iter().all(|s| s.len() <= 256)
}
fn eligible_lineage(a: &Assertion) -> bool {
    a.eligibility.as_ref().is_some_and(|e| {
        valid_id(&e.policy_id)
            && !e.policy_version.is_empty()
            && e.policy_version.len() <= 128
            && e.permission_for_reasoning
            && e.asserted_fact
            && valid_record(&e.receipt)
    })
}
/// None means unknown/unqualified; raw source text remains in the witness.
fn frequency_zero(raw: Option<&str>) -> Result<bool, String> {
    let Some(raw) = raw.map(str::trim) else {
        return Ok(false);
    };
    if raw == "HP:0040285" {
        return Ok(true);
    }
    if let Some(percent) = raw.strip_suffix('%') {
        if let Ok(value) = percent.parse::<f64>() {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err("Invalid phenotype percentage".into());
            }
            return Ok(value == 0.0);
        }
    }
    if let Some((n, d)) = raw.split_once('/') {
        if let (Ok(n), Ok(d)) = (n.parse::<u64>(), d.parse::<u64>()) {
            if d == 0 || n > d {
                return Err("Invalid phenotype cohort frequency".into());
            }
            return Ok(n == 0);
        }
    }
    Ok(false)
}
pub(crate) fn iri(id: &str) -> String {
    // Encode every non-unreserved byte, so identifiers cannot inject Turtle/N3.
    let encoded: String = id
        .as_bytes()
        .iter()
        .map(|&b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("<https://w3id.org/rare-disease-atlas/id/{encoded}>")
}
pub fn slice_sha256(slice: &Slice) -> Result<String, String> {
    prepare(slice)?;
    let mut canonical = slice.clone();
    canonical.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    canonical.assertions.sort_by(|a, b| a.id.cmp(&b.id));
    for a in &mut canonical.assertions {
        a.records.sort_by(|a, b| a.id.cmp(&b.id));
    }
    canonical.distinctiveness.sort_by(|a, b| a.phenotype.cmp(&b.phenotype));
    canonical.coverage.unavailable_sources.sort();
    serde_json::to_vec(&canonical).map(hash).map_err(|e| e.to_string())
}
pub(crate) fn projected_predicate(predicate: Predicate) -> Option<&'static str> {
    match predicate {
        Predicate::HasPhenotype => Some("eligible_phenotype"),
        Predicate::LacksPhenotype => None,
        Predicate::ModelOfGene | Predicate::ResourceForGene => Some("eligible_asset_gene"),
        other => Some(other.name()),
    }
}
pub fn n3(switches: &Switches) -> String {
    let mut result = format!(
        "@prefix c: <{NS}> .\n@prefix log: <http://www.w3.org/2000/10/swap/log#> .\n# {CONTRACT} version {VERSION}; candidate navigation only.\n"
    );
    for rule in &switches.enabled {
        result.push_str(&format!(
            "# {} version {}\n{{ {} }} => {{ ?s c:{} ?o . }} .\n",
            rule.id(),
            VERSION,
            rule.body(),
            rule.conclusion()
        ));
    }
    result
}
struct Prepared<'a> {
    eligible: Vec<&'a Assertion>,
    negative: Vec<&'a Assertion>,
    rejected: Vec<String>,
    distinctive: BTreeMap<&'a str, &'a Distinctiveness>,
    outgoing: BTreeMap<(Predicate, &'a str), Vec<&'a Assertion>>,
    incoming: BTreeMap<(Predicate, &'a str), Vec<&'a Assertion>>,
}
impl<'a> Prepared<'a> {
    fn out<'b>(&'b self, predicate: Predicate, subject: &'b str) -> &'b [&'a Assertion] {
        self.outgoing
            .get(&(predicate, subject))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn to<'b>(&'b self, predicate: Predicate, object: &'b str) -> &'b [&'a Assertion] {
        self.incoming
            .get(&(predicate, object))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}
fn visit(visits: &mut usize) -> Result<(), String> {
    *visits += 1;
    if *visits > MAX_JOIN_VISITS {
        Err("Connection witness join budget exceeded; narrow the slice".into())
    } else {
        Ok(())
    }
}
fn prepare(slice: &Slice) -> Result<Prepared<'_>, String> {
    if slice.nodes.len() > MAX_NODES
        || slice.assertions.len() > MAX_ASSERTIONS
        || slice.distinctiveness.len() > MAX_NODES
    {
        return Err("Connection slice budget exceeded".into());
    }
    if !is_hash(&slice.coverage.snapshot_sha256)
        || slice.coverage.scope.is_empty()
        || slice.coverage.scope.len() > 2048
        || slice.coverage.unavailable_sources.len() > 64
    {
        return Err("Snapshot hash and explicit coverage scope required".into());
    }
    let mut nodes = BTreeMap::new();
    let mut obsolete = BTreeSet::new();
    for n in &slice.nodes {
        if !valid_id(&n.id) || nodes.insert(n.id.as_str(), n.kind).is_some() {
            return Err("Invalid or duplicate verified node identifier".into());
        }
        if n.obsolete {
            obsolete.insert(n.id.as_str());
        }
    }
    let mut ids = BTreeSet::new();
    let mut eligible = Vec::new();
    let mut negative = Vec::new();
    let mut rejected = Vec::new();
    for a in &slice.assertions {
        if !valid_id(&a.id) || !ids.insert(&a.id) {
            return Err("Invalid or duplicate source assertion identifier".into());
        }
        let (from, to) = a.predicate.kinds();
        if a.origin != "asserted"
            || nodes.get(a.subject.as_str()) != Some(&from)
            || nodes.get(a.object.as_str()) != Some(&to)
            || a.records.is_empty()
            || a.records.len() > 16
            || !a.records.iter().all(valid_record)
            || !valid_context(&a.context)
            || !eligible_lineage(a)
        {
            if a.origin == "asserted"
                && (a.predicate == Predicate::LacksPhenotype
                    || (a.predicate == Predicate::HasPhenotype && frequency_zero(a.context.frequency.as_deref())?))
            {
                return Err(
                    "Negative phenotype evidence lacks valid asserted lineage/context; slice cannot be evaluated"
                        .into(),
                );
            }
            rejected.push(a.id.clone());
            continue;
        }
        // Zero-frequency statements cannot become positive premises. All other raw
        // forms survive without estimating a probability or multiplying confidence.
        let zero = if a.predicate == Predicate::HasPhenotype {
            frequency_zero(a.context.frequency.as_deref())?
        } else {
            false
        };
        if a.predicate == Predicate::LacksPhenotype || (a.predicate == Predicate::HasPhenotype && zero) {
            negative.push(a);
        } else if a.eligible_for_positive_inference
            && !obsolete.contains(a.subject.as_str())
            && !obsolete.contains(a.object.as_str())
        {
            eligible.push(a);
        } else {
            rejected.push(a.id.clone());
        }
    }
    let conflicts: BTreeSet<_> = negative.iter().map(|a| (&a.subject, &a.object)).collect();
    eligible.retain(|a| a.predicate != Predicate::HasPhenotype || !conflicts.contains(&(&a.subject, &a.object)));
    let mut distinctive = BTreeMap::new();
    for d in &slice.distinctiveness {
        if nodes.get(d.phenotype.as_str()) != Some(&Kind::Phenotype)
            || d.corpus_conditions == 0
            || d.annotated_conditions == 0
            || d.annotated_conditions > d.corpus_conditions
            || !is_hash(&d.corpus_sha256)
            || !valid_record(&d.record)
        {
            return Err("Invalid phenotype corpus receipt".into());
        }
        if distinctive.insert(d.phenotype.as_str(), d).is_some() {
            return Err("Duplicate phenotype corpus receipt".into());
        }
    }
    eligible.sort_by_key(|a| &a.id);
    negative.sort_by_key(|a| &a.id);
    rejected.sort();
    let mut outgoing: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut incoming: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for a in &eligible {
        outgoing.entry((a.predicate, a.subject.as_str())).or_default().push(*a);
        incoming.entry((a.predicate, a.object.as_str())).or_default().push(*a);
    }
    Ok(Prepared {
        eligible,
        negative,
        rejected,
        distinctive,
        outgoing,
        incoming,
    })
}
fn distinctive(p: &Prepared<'_>, id: &str) -> bool {
    p.distinctive
        .get(id)
        .is_some_and(|d| u128::from(d.annotated_conditions) * 10 <= u128::from(d.corpus_conditions))
}
/// No network/mutation. Export into a NEW isolated overlay, never the pinned store.
pub fn projection(slice: &Slice, switches: &Switches) -> Result<Projection, String> {
    let p = prepare(slice)?;
    let rules = n3(switches);
    let mut triples = BTreeSet::new();
    for a in &p.eligible {
        let predicate = match a.predicate {
            Predicate::HasPhenotype if distinctive(&p, &a.object) => "eligible_phenotype",
            Predicate::HasPhenotype | Predicate::LacksPhenotype => continue,
            Predicate::ModelOfGene | Predicate::ResourceForGene => "eligible_asset_gene",
            other => other.name(),
        };
        triples.insert(format!("{} <{NS}{predicate}> {} .", iri(&a.subject), iri(&a.object)));
    }
    Ok(Projection {
        turtle: triples.into_iter().collect::<Vec<_>>().join("\n") + "\n",
        rules_sha256: hash(&rules),
        n3: rules,
        snapshot_sha256: slice.coverage.snapshot_sha256.clone(),
    })
}
/// Witness retrieval over asserted rows. This output explicitly is NOT a NRESE
/// proof. Only an independently verified overlay may promote execution to NRESE.
pub fn evaluate(slice: &Slice, switches: &Switches, seeds: &[String], top_k: usize) -> Result<Output, String> {
    #[cfg(test)]
    COPIED_OUTPUT_ASSERTIONS.with(|copies| copies.set(0));
    if seeds.is_empty() || seeds.len() > 8 || top_k == 0 || top_k > 50 {
        return Err("Expected 1–8 condition seeds and top_k 1–50".into());
    }
    if seeds
        .iter()
        .any(|id| !slice.nodes.iter().any(|n| n.id == *id && n.kind == Kind::Condition))
    {
        return Err("Seeds must be verified condition identifiers in the slice".into());
    }
    let p = prepare(slice)?;
    let fingerprint = hash(n3(switches));
    let mut visits = 0;
    let mut candidates: BTreeMap<(Rule, String, String), Candidate> = BTreeMap::new();
    let mut stored_assertions = 0;
    let mut result_budget_exceeded = false;
    let mut emit = |rule: Rule, s: &str, o: &str, path: Vec<&Assertion>| {
        if s == o {
            return;
        }
        let key = (rule, s.to_owned(), o.to_owned());
        if !candidates.contains_key(&key) && candidates.len() >= MAX_CANDIDATES {
            result_budget_exceeded = true;
            return;
        }
        if let Some(existing) = candidates.get_mut(&key) {
            // Compare borrowed assertions before allocating a duplicate path.
            if existing.witness_paths.iter().any(|witnesses| {
                witnesses.len() == path.len()
                    && witnesses
                        .iter()
                        .zip(&path)
                        .all(|(witness, assertion)| witness == *assertion)
            }) {
                return;
            }
            if existing.witness_paths.len() >= MAX_PATHS {
                existing.witnesses_complete = false;
                return;
            }
        }
        let negative_count = if candidates.contains_key(&key) {
            0
        } else {
            p.negative.iter().filter(|a| a.subject == s || a.subject == o).count()
        };
        // Never discard contrary evidence to fit a page. Reserve complete
        // negative context and the positive path together, or fail closed.
        let needed = negative_count + path.len();
        if stored_assertions + needed > MAX_OUTPUT_ASSERTIONS {
            result_budget_exceeded = true;
            return;
        }
        stored_assertions += needed;
        let c = candidates.entry(key).or_insert_with(|| Candidate {
            id: hash(format!(
                "{CONTRACT}|{VERSION}|{}|{s}|{o}|{}|{fingerprint}",
                rule.id(),
                slice.coverage.snapshot_sha256
            )),
            subject: s.into(),
            relation: rule.conclusion(),
            object: o.into(),
            origin: "inferred",
            execution: "asserted-witness-query",
            rule_id: rule.id(),
            rule_version: VERSION,
            rules_sha256: fingerprint.clone(),
            witness_paths: vec![],
            negative_evidence: p
                .negative
                .iter()
                .filter(|a| a.subject == s || a.subject == o)
                .map(|a| copy_output_assertion(a))
                .collect(),
            distinctiveness: vec![],
            witnesses_complete: slice.coverage.input_complete
                && p.rejected.is_empty()
                && slice.coverage.unavailable_sources.is_empty(),
            limitation: rule.limitation(),
        });
        let witnesses: Vec<_> = path.iter().map(|a| copy_output_assertion(a)).collect();
        if rule == Rule::SharedDistinctivePhenotypes {
            for a in &path {
                if let Some(d) = p.distinctive.get(a.object.as_str()) {
                    if !c.distinctiveness.iter().any(|r| r.phenotype == d.phenotype) {
                        c.distinctiveness.push((*d).clone());
                    }
                }
            }
        }
        c.witness_paths.push(witnesses);
    };
    // All joins are bounded by MAX_ASSERTIONS. Seed restriction happens before
    // combinations; no global all-pairs materialisation on the serving graph.
    for seed in seeds.iter().collect::<BTreeSet<_>>() {
        if switches.enabled.contains(&Rule::SharedDistinctivePhenotypes) {
            let mut shared: BTreeMap<&str, Vec<(&Assertion, &Assertion)>> = BTreeMap::new();
            for a in p
                .out(Predicate::HasPhenotype, seed)
                .iter()
                .filter(|a| distinctive(&p, &a.object))
            {
                for b in p
                    .to(Predicate::HasPhenotype, &a.object)
                    .iter()
                    .filter(|b| b.subject != *seed)
                {
                    visit(&mut visits)?;
                    shared.entry(&b.subject).or_default().push((a, b));
                }
            }
            for (target, pairs) in shared {
                // At least two DISTINCT phenotype IDs, not repeated source rows.
                if pairs.iter().map(|(a, _)| &a.object).collect::<BTreeSet<_>>().len() < 2 {
                    continue;
                }
                for i in 0..pairs.len() {
                    for j in i + 1..pairs.len() {
                        visit(&mut visits)?;
                        let (a, b) = pairs[i];
                        let (c, d) = pairs[j];
                        if a.object != c.object {
                            emit(Rule::SharedDistinctivePhenotypes, seed, target, vec![a, b, c, d]);
                        }
                    }
                }
            }
        }
        for a in p.out(Predicate::HasCausalGene, seed) {
            for (predicate, rule) in [
                (Predicate::HasCausalGene, Rule::SharedCausalGene),
                (Predicate::Targets, Rule::DrugTargetOverlap),
                (Predicate::ServesGene, Rule::CommunityViaGene),
                (Predicate::ModelOfGene, Rule::AssetViaGene),
                (Predicate::ResourceForGene, Rule::AssetViaGene),
            ] {
                if !switches.enabled.contains(&rule) {
                    continue;
                }
                for b in p.to(predicate, &a.object) {
                    visit(&mut visits)?;
                    emit(rule, seed, &b.subject, vec![a, b]);
                }
            }
            if switches.enabled.contains(&Rule::SharedPathway) {
                for b in p.out(Predicate::InPathway, &a.object) {
                    visit(&mut visits)?;
                    for d in p.to(Predicate::InPathway, &b.object) {
                        visit(&mut visits)?;
                        for c in p.to(Predicate::HasCausalGene, &d.subject) {
                            visit(&mut visits)?;
                            emit(Rule::SharedPathway, seed, &c.subject, vec![a, b, c, d]);
                        }
                    }
                }
            }
        }
    }
    if result_budget_exceeded {
        return Err("Connection output witness budget exceeded; narrow the slice".into());
    }
    // Deterministic navigation order, deliberately no fabricated confidence score.
    let mut all: Vec<_> = candidates.into_values().collect();
    all.sort_by(|a, b| {
        a.rule_id
            .cmp(b.rule_id)
            .then(a.object.cmp(&b.object))
            .then(a.subject.cmp(&b.subject))
    });
    let result_truncated = all.len() > top_k;
    all.truncate(top_k);
    Ok(Output {
        contract: CONTRACT,
        enabled_rules: switches.enabled.iter().copied().collect(),
        rules_sha256: fingerprint,
        candidates: all,
        coverage: slice.coverage.clone(),
        eligible_assertions: p.eligible.len(),
        rejected_assertions: p.rejected,
        result_truncated,
    })
}
