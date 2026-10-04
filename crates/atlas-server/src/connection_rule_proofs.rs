//! Fail-closed attachment of actual NRESE proofs to asserted-source witnesses.
//! No IO, process launching, store mutation, or inference performed here.
use crate::connection_rules::{self as rules, Assertion, Candidate, Projection, Rule, Slice, Switches};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PROFILE_FORMAT: &str = "zebratlas-connection-overlay-profile-v1";
pub const RECEIPT_FORMAT: &str = "zebratlas-connection-overlay-verification-v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBinding {
    pub rule: Rule,
    pub version: String,
    pub engine_rule: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub format: String,
    pub engine: String,
    pub semantics_version: u32,
    pub mode: String,
    pub snapshot_sha256: String,
    pub slice_sha256: String,
    pub projection_sha256: String,
    pub rules_sha256: String,
    pub engine_binary_sha256: String,
    pub query_endpoint: String,
    pub proof_endpoint: String,
    pub rule_bindings: Vec<RuleBinding>,
    pub verification_receipt_sha256: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleCheck {
    pub rule: Rule,
    pub asserted_only_absent: bool,
    pub inferred_present: bool,
    pub proof_verified: bool,
    pub source_paths_verified: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub format: String,
    pub scope: String,
    pub status: String,
    pub synthetic: bool,
    pub snapshot_sha256: String,
    pub slice_sha256: String,
    pub projection_sha256: String,
    pub rules_sha256: String,
    pub engine_binary_sha256: String,
    pub checks: Vec<RuleCheck>,
}
/// These anchors MUST come from operator-reviewed sealed source/engine/receipt
/// artifacts, never from request fields or from the untrusted Profile itself.
pub struct ExpectedArtifacts {
    pub snapshot_sha256: String,
    pub slice_sha256: String,
    pub engine_binary_sha256: String,
    pub verification_receipt_sha256: BTreeSet<String>,
}
/// Private fields prevent callers constructing a verified capability directly.
pub struct ValidatedOverlay {
    profile: Profile,
    slice: Slice,
    projection: Projection,
}
impl ValidatedOverlay {
    pub fn query_endpoint(&self) -> &str {
        &self.profile.query_endpoint
    }
    pub fn proof_endpoint(&self) -> &str {
        &self.profile.proof_endpoint
    }
    pub fn supported_rules(&self) -> impl Iterator<Item = Rule> + '_ {
        self.profile.rule_bindings.iter().map(|b| b.rule)
    }
    pub fn rules_sha256(&self) -> &str {
        &self.profile.rules_sha256
    }
    pub fn snapshot_sha256(&self) -> &str {
        &self.profile.snapshot_sha256
    }
    /// Build witnesses with the exact validated rule fingerprint, then apply the
    /// request's switches. No rematerialisation or temporary store per request.
    pub fn witnesses(&self, selected: &Switches, seeds: &[String], top_k: usize) -> Result<Vec<Candidate>, String> {
        let supported: BTreeSet<_> = self.supported_rules().collect();
        if !selected.enabled.is_subset(&supported) {
            return Err("Requested rule is absent from the validated overlay".into());
        }
        if top_k == 0 || top_k > 50 {
            return Err("top_k must be 1–50".into());
        }
        if selected.enabled.is_empty() {
            return Ok(vec![]);
        }
        let mut output = rules::evaluate(&self.slice, &Switches { enabled: supported }, seeds, 50)?;
        output
            .candidates
            .retain(|c| selected.enabled.iter().any(|r| r.id() == c.rule_id));
        // Dropping other rules after a top-50 cutoff could silently hide selected
        // results. Fail closed; the adapter must supply a narrower bounded scope.
        if output.result_truncated {
            return Err("Validated overlay candidate scope exceeds the witness page; narrow seed scope".into());
        }
        output.candidates.truncate(top_k);
        Ok(output.candidates)
    }
}
fn endpoint(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|e| e.to_string())?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Connection overlay endpoint must be operator-configured loopback HTTP".into());
    }
    Ok(())
}
pub fn receipt_sha256(receipt: &Receipt) -> Result<String, String> {
    serde_json::to_vec(receipt).map(rules::hash).map_err(|e| e.to_string())
}
pub fn validate(
    profile: &Profile,
    slice: &Slice,
    receipts: &[Receipt],
    expected: &ExpectedArtifacts,
) -> Result<ValidatedOverlay, String> {
    if profile.format != PROFILE_FORMAT
        || profile.engine != "nrese"
        || profile.mode != "custom"
        || profile.semantics_version != 2
    {
        return Err("Unsupported connection overlay profile".into());
    }
    for h in [
        &profile.snapshot_sha256,
        &profile.slice_sha256,
        &profile.projection_sha256,
        &profile.rules_sha256,
        &profile.engine_binary_sha256,
    ] {
        if !rules::is_hash(h) {
            return Err("Invalid overlay artifact hash".into());
        }
    }
    if profile.snapshot_sha256 != expected.snapshot_sha256
        || profile.slice_sha256 != expected.slice_sha256
        || profile.engine_binary_sha256 != expected.engine_binary_sha256
        || profile.snapshot_sha256 != slice.coverage.snapshot_sha256
        || profile.slice_sha256 != rules::slice_sha256(slice)?
    {
        return Err("Connection overlay is stale or does not match the sealed artifacts".into());
    }
    endpoint(&profile.query_endpoint)?;
    endpoint(&profile.proof_endpoint)?;
    let query = reqwest::Url::parse(&profile.query_endpoint).map_err(|e| e.to_string())?;
    let proof = reqwest::Url::parse(&profile.proof_endpoint).map_err(|e| e.to_string())?;
    if query.origin() != proof.origin()
        || query.path().strip_suffix("/sparql").is_none()
        || query.path().strip_suffix("/sparql") != proof.path().strip_suffix("/explain")
    {
        return Err("Query and proof must address the same loopback overlay repository".into());
    }
    let mut bindings = BTreeMap::new();
    let mut names = BTreeSet::new();
    for binding in &profile.rule_bindings {
        if binding.version != rules::VERSION
            || binding.engine_rule.is_empty()
            || binding.engine_rule.len() > 512
            || bindings.insert(binding.rule, binding).is_some()
            || !names.insert(&binding.engine_rule)
        {
            return Err("Invalid or duplicate overlay rule binding".into());
        }
    }
    if bindings.is_empty() || bindings.len() > Rule::ALL.len() {
        return Err("Overlay must name its implemented rules".into());
    }
    for (index, binding) in bindings.values().enumerate() {
        if binding.engine_rule != format!("connection-rules.n3#{}", index + 1) {
            return Err("Overlay engine rule ordinal does not match the exact generated N3 registry".into());
        }
    }
    let selected = Switches {
        enabled: bindings.keys().copied().collect(),
    };
    let projection = rules::projection(slice, &selected)?;
    if projection.turtle.trim().is_empty() {
        return Err("Source overlay has no rights-cleared, positively eligible asserted premises".into());
    }
    if projection.rules_sha256 != profile.rules_sha256 || rules::hash(&projection.turtle) != profile.projection_sha256 {
        return Err("Exact overlay rules/projection fingerprint mismatch".into());
    }
    let wanted: BTreeSet<_> = profile.verification_receipt_sha256.iter().cloned().collect();
    if wanted.is_empty()
        || wanted.len() != profile.verification_receipt_sha256.len()
        || wanted != expected.verification_receipt_sha256
        || receipts.len() != wanted.len()
        || receipts.len() > 8
    {
        return Err("Operator-anchored source-overlay receipts required".into());
    }
    let mut checked = BTreeSet::new();
    let mut seen_receipts = BTreeSet::new();
    for receipt in receipts {
        let hash = receipt_sha256(receipt)?;
        if !wanted.contains(&hash)
            || !seen_receipts.insert(hash)
            || receipt.format != RECEIPT_FORMAT
            || receipt.scope != "source-overlay"
            || receipt.status != "passed"
            || receipt.synthetic
            || receipt.snapshot_sha256 != profile.snapshot_sha256
            || receipt.slice_sha256 != profile.slice_sha256
            || receipt.projection_sha256 != profile.projection_sha256
            || receipt.rules_sha256 != profile.rules_sha256
            || receipt.engine_binary_sha256 != profile.engine_binary_sha256
            || receipt.checks.len() > 6
        {
            return Err("Unverified, synthetic or mismatched source-overlay receipt".into());
        }
        for c in &receipt.checks {
            if !bindings.contains_key(&c.rule)
                || !c.asserted_only_absent
                || !c.inferred_present
                || !c.proof_verified
                || !c.source_paths_verified
            {
                return Err("Incomplete overlay rule verification".into());
            }
            checked.insert(c.rule);
        }
    }
    if checked != selected.enabled {
        return Err("Every enabled overlay rule needs actual inference/source-proof verification".into());
    }
    Ok(ValidatedOverlay {
        profile: profile.clone(),
        slice: slice.clone(),
        projection,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub origin: String,
    #[serde(default)]
    pub rule: Option<String>,
    #[serde(default)]
    pub premises: Vec<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proof {
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Justification {
    pub verified: bool,
    pub complete: bool,
}
#[derive(Debug, Serialize)]
pub struct VerifiedCandidate {
    pub candidate: Candidate,
    pub engine: &'static str,
    pub engine_rule: String,
    pub engine_binary_sha256: String,
    pub snapshot_sha256: String,
    pub rules_sha256: String,
    pub proof: Proof,
    pub justification: Justification,
    pub asserted_source_paths: Vec<Vec<Assertion>>,
}
fn triple(step: &Step) -> String {
    format!("<{}> <{}> <{}> .", step.subject, step.predicate, step.object)
}
fn assertion_triple(a: &Assertion) -> Option<String> {
    rules::projected_predicate(a.predicate).map(|p| {
        format!(
            "{} <{}{p}> {} .",
            rules::iri(&a.subject),
            rules::NS,
            rules::iri(&a.object)
        )
    })
}
/// The candidate is recomputed from the validated source slice to prevent caller
/// substitution. All reachable leaves must match complete source witness paths.
pub fn attach(
    overlay: &ValidatedOverlay,
    candidate: Candidate,
    proof: Proof,
    justification: Justification,
) -> Result<VerifiedCandidate, String> {
    let binding = overlay
        .profile
        .rule_bindings
        .iter()
        .find(|b| b.rule.id() == candidate.rule_id)
        .ok_or("Candidate rule absent from overlay")?;
    let selected = Switches {
        enabled: [binding.rule].into_iter().collect(),
    };
    let originals = overlay.witnesses(&selected, &[candidate.subject.clone()], 50)?;
    let original = originals
        .into_iter()
        .find(|c| c.id == candidate.id)
        .ok_or("Candidate is absent from the validated source slice")?;
    if serde_json::to_value(&original).map_err(|e| e.to_string())?
        != serde_json::to_value(&candidate).map_err(|e| e.to_string())?
    {
        return Err("Candidate source evidence was changed after overlay validation".into());
    }
    if !justification.verified || !justification.complete || proof.steps.is_empty() || proof.steps.len() > 64 {
        return Err("Unverified or incomplete NRESE candidate proof".into());
    }
    let root = &proof.steps[0];
    if root.origin != "inferred"
        || root.subject != rules::iri(&candidate.subject).trim_matches(['<', '>'])
        || root.predicate != format!("{}{}", rules::NS, candidate.relation)
        || root.object != rules::iri(&candidate.object).trim_matches(['<', '>'])
        || root.rule.as_deref() != Some(binding.engine_rule.as_str())
        || root.premises.is_empty()
    {
        return Err("NRESE proof does not establish this candidate with its named rule".into());
    }
    // These six rules derive only navigation heads, none of which occurs in any
    // body. There can be no inferred intermediate premise or cyclic proof.
    let mut leaves = BTreeSet::new();
    let mut visited = BTreeSet::from([0]);
    for &index in &root.premises {
        if index == 0 || index >= proof.steps.len() {
            return Err("Invalid NRESE proof premise index".into());
        }
        let step = &proof.steps[index];
        visited.insert(index);
        if step.origin != "asserted"
            || !step.premises.is_empty()
            || step.subject.len() > 2048
            || step.predicate.len() > 2048
            || step.object.len() > 2048
        {
            return Err("Unsupported inferred or malformed navigation premise".into());
        }
        let t = triple(step);
        if !overlay.projection.turtle.lines().any(|line| line == t) {
            return Err("NRESE premise is absent from the validated projection".into());
        }
        leaves.insert(t);
    }
    if visited.len() != proof.steps.len() {
        return Err("NRESE proof contains unreachable premises".into());
    }
    let matching: Vec<_> = candidate
        .witness_paths
        .iter()
        .filter(|path| {
            let triples: BTreeSet<_> = path.iter().filter_map(assertion_triple).collect();
            triples == leaves && path.iter().all(|a| a.origin == "asserted" && !a.records.is_empty())
        })
        .cloned()
        .collect();
    if matching.is_empty() {
        return Err("Actual engine proof has no complete source-row witness path".into());
    }
    let mut verified = candidate;
    verified.execution = "nrese-verified-overlay";
    Ok(VerifiedCandidate {
        candidate: verified,
        engine: "nrese",
        engine_rule: binding.engine_rule.clone(),
        engine_binary_sha256: overlay.profile.engine_binary_sha256.clone(),
        snapshot_sha256: overlay.profile.snapshot_sha256.clone(),
        rules_sha256: overlay.profile.rules_sha256.clone(),
        proof,
        justification,
        asserted_source_paths: matching,
    })
}
