//! Every English fallback text of this crate's catalog messages (D26), in one place.
//!
//! Handlers never write user-facing English: they call [`sentence`] / [`msg`] with a message key
//! and params; the fallback is rendered here from the same params. The web renders the key from
//! its 12 catalogs (`web/messages/<locale>.json`, top-level `questions`), so every key below must
//! exist there with the same params. Params may be strings, numbers, string lists (joined in the
//! reader's language) or a nested message `{key, params, fallback}` (rendered first).
//!
//! Copy rules (docs/design/DESIGN.md §5): plain words, reading age ~12, short sentences; a
//! hypothesis is always a question, never a claim. Gene symbols, ids and names stay as they are.

use serde_json::{Value, json};

/// Catalog sentence: key + params + English fallback, with the records it cites.
pub fn sentence(key: &str, params: Value, cites: Vec<String>) -> Value {
    let fallback = text(key, &params);
    json!({ "text": fallback, "msg": { "key": key, "params": params, "fallback": fallback }, "cites": cites })
}

/// A bare catalog message `{key, params, fallback}` (for nesting into another message's params).
pub fn msg(key: &str, params: Value) -> Value {
    let fallback = text(key, &params);
    json!({ "key": key, "params": params, "fallback": fallback })
}

/// What one source covers ("Where we looked"), as a catalog message `coverage.scope.<source>`.
/// The ingest crates record the scope in English for the provenance record; the reader gets this
/// plain rewrite. An unknown source keeps the recorded scope as its fallback.
pub fn scope(source: &str, recorded: &str) -> Value {
    let kind = match source {
        "ctgov" | "reporter" | "pubmed" | "people" | "hgnc" | "contacts" | "wikidata" | "orgs" => Some(source),
        _ if recorded.starts_with("grants ") => Some("reporter"),
        _ if recorded.starts_with("articles ") => Some("pubmed"),
        _ => None,
    };
    match kind {
        Some(k) => msg(&format!("coverage.scope.{k}"), json!({})),
        None => json!({ "key": "coverage.scope.other", "params": { "scope": recorded }, "fallback": recorded }),
    }
}

/// The plain-language phrase for a process (pathway or GO term), as a nested catalog message.
/// A small reviewed lookup: exact ids first, then conservative name rules in order; anything
/// else gets the honest "the same process in the body" (the scientific term stays one tap down).
pub fn process(id: &str, label: &str) -> Value {
    msg(&format!("questions.process.{}", process_kind(id, label)), json!({}))
}

/// Exact ids reviewed by hand (anchors seen for the DEE slice).
const PROCESS_IDS: [(&str, &str); 6] = [
    ("R-HSA-6794361", "connect"),     // Neurexins and neuroligins
    ("GO:0031629", "release"),        // synaptic vesicle fusion to presynaptic active zone membrane
    ("GO:0031630", "release"),        // regulation of synaptic vesicle fusion to presynaptic active zone membrane
    ("R-HSA-888590", "calming"),      // GABA synthesis, release, reuptake and degradation
    ("GO:0006506", "surface_anchor"), // GPI anchor biosynthetic process
    ("GO:0043490", "energy"),         // malate-aspartate shuttle
];

/// Name rules (lower-case substrings), first match wins: specific before general.
const PROCESS_NAMES: [(&str, &str); 44] = [
    ("neurexin", "connect"),
    ("neuroligin", "connect"),
    ("between l1 and", "connect"),
    ("pathway of l1", "connect"),
    ("synapse assembly", "connect"),
    ("synapse organization", "connect"),
    ("synaptic vesicle", "release"),
    ("snare complex", "release"),
    ("neurotransmitter secretion", "release"),
    ("gaba", "calming"),
    ("gamma-aminobutyric", "calming"),
    ("glutamat", "go_signal"),
    ("nmda", "go_signal"),
    ("ampa", "go_signal"),
    ("excitatory", "go_signal"),
    ("dopamine", "dopamine"),
    ("cardiac", "heart_signal"),
    ("action potential", "electric"),
    ("depolari", "electric"),
    ("sodium channel", "electric"),
    ("potassium channel", "electric"),
    ("calcium ion import", "calcium"),
    ("calcium channel", "calcium"),
    ("synaptic potentiation", "tuning"),
    ("synaptic plasticity", "tuning"),
    ("synaptic transmission", "passing"),
    ("memory", "memory"),
    ("neurotrophi", "growth_signal"),
    ("ntrk", "growth_signal"),
    ("trka", "growth_signal"),
    ("cortex development", "brain_growth"),
    ("amygdala development", "brain_growth"),
    ("dendrite", "brain_growth"),
    ("axon", "brain_growth"),
    ("lysosom", "recycling"),
    ("vacuolar acidification", "recycling"),
    ("endosom", "recycling"),
    ("endocytosis", "intake"),
    ("vesicle scission", "intake"),
    ("tricarboxylic acid", "energy"),
    ("malate", "energy"),
    ("mitochondri", "energy"),
    ("trna aminoacylation", "protein_building"),
    ("apoptotic", "cell_death"),
];

fn process_kind(id: &str, label: &str) -> &'static str {
    if let Some((_, k)) = PROCESS_IDS.iter().find(|(i, _)| *i == id) {
        return k;
    }
    let l = label.to_lowercase();
    PROCESS_NAMES
        .iter()
        .find(|(n, _)| l.contains(n))
        .map_or("unmapped", |(_, k)| k)
}

fn s(p: &Value, k: &str) -> String {
    match &p[k] {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        Value::Array(xs) => list(
            &xs.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
        ),
        Value::Object(o) => o.get("fallback").and_then(Value::as_str).unwrap_or("").to_owned(),
        _ => String::new(),
    }
}

fn n(p: &Value, k: &str) -> u64 {
    p[k].as_u64().unwrap_or(0)
}

/// "a", "a and b", "a, b and c".
fn list(xs: &[String]) -> String {
    match xs.len() {
        0 => String::new(),
        1 => xs[0].clone(),
        k => format!("{} and {}", xs[..k - 1].join(", "), xs[k - 1]),
    }
}

fn upper_first(x: &str) -> String {
    let mut c = x.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

fn count(n: u64, one: &str, other: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {other}")
    }
}

/// The English fallback of a message (the `en` catalog says the same).
pub fn text(key: &str, p: &Value) -> String {
    match key {
        // --- the question, in plain words --------------------------------------------------------
        "questions.hypothesis.shared_process" => format!("Do changes in {} affect {} in the same way?", s(p, "genes"), s(p, "process")),
        "questions.why.shared_process" => {
            let more = match n(p, "more") {
                0 => String::new(),
                1 => " One more gene is part of this question.".into(),
                m => format!(" {m} more genes are part of this question."),
            };
            format!(
                "In each of these {} conditions, the gene involved is linked to {}. If the effect is the same, one line of research could help all of them.{more}",
                n(p, "communities"),
                s(p, "process")
            )
        }
        "questions.shared.process" => format!("Shared: {}", s(p, "process")),
        "questions.term.pathway" => format!("{} ({})", s(p, "pathway"), s(p, "id")),
        "questions.process.connect" => "how brain cells connect".into(),
        "questions.process.release" => "how brain cells release signals".into(),
        "questions.process.calming" => "the brain's main calming signal (GABA)".into(),
        "questions.process.go_signal" => "the brain's main activating signal (glutamate)".into(),
        "questions.process.dopamine" => "the brain's dopamine signal".into(),
        "questions.process.heart_signal" => "the electric signals of the heart".into(),
        "questions.process.electric" => "the electric signals of nerve cells".into(),
        "questions.process.calcium" => "how cells let calcium in".into(),
        "questions.process.tuning" => "how links between brain cells get stronger or weaker".into(),
        "questions.process.passing" => "how brain cells pass signals on".into(),
        "questions.process.memory" => "how memory works".into(),
        "questions.process.growth_signal" => "growth signals for brain cells".into(),
        "questions.process.brain_growth" => "how the brain grows and wires up".into(),
        "questions.process.recycling" => "how cells break down and recycle material".into(),
        "questions.process.intake" => "how cells take in material".into(),
        "questions.process.energy" => "how cells make energy".into(),
        "questions.process.surface_anchor" => "how cells fix proteins to their surface".into(),
        "questions.process.protein_building" => "how cells build proteins".into(),
        "questions.process.cell_death" => "controlled cell death".into(),
        "questions.process.unmapped" => "the same process in the body".into(),
        // --- evidence ledger ---------------------------------------------------------------------
        "questions.for.statement" => format!("{}: {}", s(p, "condition"), s(p, "statement")),
        "questions.against.finding" => s(p, "explanation"),
        "questions.against.same_gene_different_mechanism" => {
            format!("Same gene, maybe a different cause ({}): {}", s(p, "gene"), s(p, "why"))
        }
        "questions.against.mechanism_split" => format!("The cause may differ within one condition ({}): {}", s(p, "gene"), s(p, "why")),
        "questions.against.lookalike" => format!("Looks alike, but the cause may differ ({}): {}", s(p, "gene"), s(p, "why")),
        "questions.against.variant_level" => "Changes in one gene do not all act the same way: some switch the gene off, others change what it does. A link at the level of the gene may not hold for every variant.".into(),
        "questions.unknown.validate" => format!("Still to check: {}", s(p, "text")),
        "questions.unknown.functional_coverage" => format!(
            "{}: ClinVar records lab tests of the effect for {} of {} known gene changes.",
            s(p, "gene"),
            n(p, "with_functional_data"),
            n(p, "variants")
        ),
        // --- the deciding experiment -------------------------------------------------------------
        "questions.experiment" => format!(
            "Do variants linked to {} disrupt the same measurable function in {}?",
            s(p, "conditions"),
            s(p, "pathway")
        ),
        "questions.experiment_note" => format!("Would support: {} Would challenge: {}", s(p, "support"), s(p, "challenge")),
        // --- who could help ----------------------------------------------------------------------
        "questions.bridge.worked_on" => format!(
            "{}: named in {}, {} and {} in public records",
            s(p, "gene"),
            count(n(p, "grants"), "grant", "grants"),
            count(n(p, "papers"), "paper", "papers"),
            count(n(p, "trials"), "trial", "trials")
        ),
        "questions.bridge.identity.same" => "Same person (linked by a researcher ID)".into(),
        "questions.bridge.identity.possible" => "Possibly the same person: records linked by name and workplace, not by an ID".into(),
        "questions.bridge.suggestion" => format!(
            "Has worked on more than one of {}. Our suggestion: contact them through {}.",
            s(p, "genes"),
            s(p, "institution")
        ),
        "questions.drug.indication" => format!(
            "{} has reached {} in clinical studies for {} (Open Targets; this does not show that it works).",
            s(p, "drug"),
            s(p, "stage"),
            s(p, "condition")
        ),
        "questions.drug.indication_related" => format!(
            "{} has reached {} in clinical studies for {}, a related condition (Open Targets; this does not show that it works).",
            s(p, "drug"),
            s(p, "stage"),
            s(p, "condition")
        ),
        "questions.drug.target" => format!(
            "{} acts on {} ({}); furthest clinical stage: {} (Open Targets; this does not show that it works).",
            s(p, "drug"),
            s(p, "gene"),
            s(p, "mechanism"),
            s(p, "stage")
        ),
        // --- what our sources do not record (J3.4, `/api/condition/{id}/gaps`) -------------------
        "gaps.missing.patient_group" => "No patient group on our list names this condition or its gene. Ask your genetic counsellor or a related patient group whether families with this condition are already in touch.".into(),
        "gaps.missing.patient_group_unloaded" => "Our list of patient groups is not loaded, so we could not check for one. Ask your genetic counsellor or a related patient group.".into(),
        "gaps.missing.open_study" => "No study on ClinicalTrials.gov that is recruiting or running names this condition or its gene. Ask the nearest specialist centre about studies that are not registered yet, and check studies for related conditions.".into(),
        "gaps.missing.registry" => "No patient registry or natural history study names this condition or its gene. Ask a related patient group whether its registry can include this gene, or join a registry for the broader group of conditions.".into(),
        "gaps.missing.gene" => "Orphanet and OMIM record no gene that causes this condition. Ask whether genetic testing was done and which gene change was found.".into(),
        "gaps.missing.variant_effect" => "Orphanet does not record how the gene change causes this condition: by reducing what the gene does, or by changing it. Ask a specialist.".into(),
        "gaps.missing.symptoms" => match n(p, "n") {
            0 => "No symptoms of this condition are recorded in the Human Phenotype Ontology (HPO). A patient registry or natural history study would record them.".into(),
            1 => "Only 1 symptom of this condition is recorded in the Human Phenotype Ontology (HPO). A patient registry or natural history study would record more.".into(),
            k => format!("Only {k} symptoms of this condition are recorded in the Human Phenotype Ontology (HPO). A patient registry or natural history study would record more."),
        },
        "gaps.missing.prevalence" => "Orphanet has no figure for how common this condition is. Patient registries and published case reports give a first count.".into(),
        "gaps.question.variant" => format!("Does our {} gene change reduce what the gene does, or alter it? Does that affect which treatment research applies to us?", s(p, "gene")),
        "gaps.question.centre" => "Is there a specialist centre that sees many people with this condition? Can we be referred?".into(),
        "gaps.question.researchers" => format!("Which researchers or studies work on {}? Would they want to hear from us?", s(p, "gene")),
        "gaps.question.unregistered" => "Are any studies starting soon that are not yet on ClinicalTrials.gov?".into(),
        "gaps.note" => "These are things our sources do not record. Take the questions to a specialist.".into(),
        // --- searched, nothing exact (connections / coverage) ------------------------------------
        "coverage.none" => format!(
            "We found no record for {} in {} (checked {}). Patient groups or studies may exist outside these sources.",
            s(p, "condition"),
            s(p, "sources"),
            s(p, "dates")
        ),
        "coverage.partial" => format!(
            "{} was only searched for {}, not for {}.",
            s(p, "source"),
            s(p, "genes"),
            s(p, "missing")
        ),
        "coverage.partial_no_gene" => format!(
            "{} was only searched for {}. This condition has no known gene, so it was not covered.",
            s(p, "source"),
            s(p, "genes")
        ),
        "coverage.scope.ctgov" => "Studies registered on ClinicalTrials.gov, matched to rare conditions by name, MeSH term and gene".into(),
        "coverage.scope.reporter" => "NIH-funded research projects whose title, summary or keywords name the gene (NIH RePORTER)".into(),
        "coverage.scope.pubmed" => "Papers that name the gene in their title or abstract (PubMed, newest 5,000)".into(),
        "coverage.scope.people" => "Researchers with papers or grants on at least two of the genes we studied in depth".into(),
        "coverage.scope.hgnc" => "Other names and earlier symbols of each gene (HGNC)".into(),
        "coverage.scope.contacts" => "Contacts, officials and sites of studies linked to the genes we studied in depth".into(),
        "coverage.scope.wikidata" => "Names of conditions and genes in many languages (Wikidata, used for search only)".into(),
        "coverage.scope.orgs" => "Patient organisations found by web and directory searches in several languages, checked by hand against their own pages".into(),
        "connections.none_found" => match n(p, "related") {
            0 => format!("No {} found for {}.", s(p, "kind_label"), s(p, "condition")),
            1 => format!("No {} found for {}. 1 for a related condition is shown.", s(p, "kind_label"), s(p, "condition")),
            r => format!("No {} found for {}. {r} for related conditions are shown.", s(p, "kind_label"), s(p, "condition")),
        },
        // --- why a card is listed (template why-lines; the LLM sentence replaces them when on) ----
        "why.study_registered" => format!("This {} is registered for {}.", s(p, "kind_label"), s(p, "condition")),
        "why.study_gene" => format!("This {} names {}, a gene that causes {}.", s(p, "kind_label"), s(p, "gene"), s(p, "condition")),
        "why.study_broader" => format!("This {} is for {}, a broader group that includes {}.", s(p, "kind_label"), s(p, "group"), s(p, "condition")),
        "why.grant" => format!("This NIH-funded project at {} studies {}.", s(p, "organisation"), s(p, "about")),
        "why.org_condition" => format!("{} supports people with {}.", s(p, "organisation"), s(p, "condition")),
        "why.org_gene" => format!(
            "{} supports people with changes in {}, the gene that causes {}.",
            s(p, "organisation"),
            s(p, "gene"),
            s(p, "condition")
        ),
        "why.org_broader" => format!(
            "{} supports people with {}, a broader group that includes {}.",
            s(p, "organisation"),
            s(p, "group"),
            s(p, "condition")
        ),
        "why.researcher" => {
            let grants = match n(p, "grants") {
                0 => String::new(),
                g => format!(" and leads {}", count(g, "NIH-funded project", "NIH-funded projects")),
            };
            format!(
                "{} has written {} about {}{grants}.",
                s(p, "name"),
                count(n(p, "papers"), "paper", "papers"),
                s(p, "about")
            )
        }
        // --- card parts -------------------------------------------------------------------------
        "card.subtitle.study" => format!("{} · {}", s(p, "kind_label"), s(p, "status_label")),
        "card.subtitle.grant" => format!("{} grant at {}, {}", s(p, "code"), s(p, "organisation"), s(p, "years")),
        "card.subtitle.kind" => upper_first(&s(p, "kind_label")),
        "card.enrollment_note" => "Planned number of participants for the whole study, across all conditions and genes. It is not a count for this condition.".into(),
        "studykind.trial" => "A clinical trial tests a treatment in people.".into(),
        "studykind.registry" => "A patient registry collects health information from many people over time, so researchers can study the condition.".into(),
        "studykind.natural_history" => "A natural history study follows how the condition develops over time. Nobody receives a new treatment.".into(),
        "studykind.observational" => "An observational study measures and records. Nobody receives a new treatment.".into(),
        "studykind.expanded_access" => "An early access programme offers a treatment that is not yet approved, outside a clinical trial.".into(),
        // --- how to reach them (nodes.rs) -------------------------------------------------------
        "channel.ctgov_central_contact" => "Study contact named by the sponsor on ClinicalTrials.gov".into(),
        "channel.ctgov_contacts" => "Study contacts and locations on ClinicalTrials.gov".into(),
        "channel.reporter_project" => "NIH RePORTER project page, with the lead researcher and institution".into(),
        "channel.orcid" => "Public ORCID profile, with a link to the researcher's institution".into(),
        "channel.pubmed_author" => "List of their papers on PubMed; the affiliation names the institution to contact".into(),
        "channel.contact_form" => "Official contact page".into(),
        "channel.website" => "Official website".into(),
        "channel.none" => "No public contact recorded".into(),
        "channel.none_sponsor" => "No public contact recorded. The sponsor is named in the study record.".into(),
        // Unknown keys are a bug: say so in tests, show the raw key rather than nothing.
        _ => {
            debug_assert!(false, "copy: no English text for {key}");
            key.to_owned()
        }
    }
}

/// Every key this crate emits (tests check it against the web's English catalog).
#[cfg(test)]
const KEYS: [&str; 41] = [
    "questions.hypothesis.shared_process",
    "questions.why.shared_process",
    "questions.shared.process",
    "questions.term.pathway",
    "questions.process.connect",
    "questions.process.release",
    "questions.process.calming",
    "questions.process.go_signal",
    "questions.process.dopamine",
    "questions.process.heart_signal",
    "questions.process.electric",
    "questions.process.calcium",
    "questions.process.tuning",
    "questions.process.passing",
    "questions.process.memory",
    "questions.process.growth_signal",
    "questions.process.brain_growth",
    "questions.process.recycling",
    "questions.process.intake",
    "questions.process.energy",
    "questions.process.surface_anchor",
    "questions.process.protein_building",
    "questions.process.cell_death",
    "questions.process.unmapped",
    "questions.for.statement",
    "questions.against.finding",
    "questions.against.same_gene_different_mechanism",
    "questions.against.mechanism_split",
    "questions.against.lookalike",
    "questions.against.variant_level",
    "questions.unknown.validate",
    "questions.unknown.functional_coverage",
    "questions.experiment",
    "questions.experiment_note",
    "questions.bridge.worked_on",
    "questions.bridge.identity.same",
    "questions.bridge.identity.possible",
    "questions.bridge.suggestion",
    "questions.drug.indication",
    "questions.drug.indication_related",
    "questions.drug.target",
];

/// Keys added by the copy audit (docs/reviews/COPY-AUDIT.md, Backend). Their English text is
/// here (codes.icd.* in codes.rs); the web catalogs get them in the audit's deliverable 2, then
/// they move into [`KEYS`].
#[cfg(test)]
const PENDING_KEYS: [&str; 51] = [
    "coverage.scope.ctgov",
    "coverage.scope.reporter",
    "coverage.scope.pubmed",
    "coverage.scope.people",
    "coverage.scope.hgnc",
    "coverage.scope.contacts",
    "coverage.scope.wikidata",
    "coverage.scope.orgs",
    "coverage.partial",
    "coverage.partial_no_gene",
    "card.subtitle.study",
    "card.subtitle.grant",
    "card.subtitle.kind",
    "card.enrollment_note",
    "studykind.trial",
    "studykind.registry",
    "studykind.natural_history",
    "studykind.observational",
    "studykind.expanded_access",
    "channel.ctgov_central_contact",
    "channel.ctgov_contacts",
    "channel.reporter_project",
    "channel.orcid",
    "channel.pubmed_author",
    "channel.contact_form",
    "channel.website",
    "channel.none",
    "channel.none_sponsor",
    "gaps.missing.patient_group",
    "gaps.missing.patient_group_unloaded",
    "gaps.missing.open_study",
    "gaps.missing.registry",
    "gaps.missing.gene",
    "gaps.missing.variant_effect",
    "gaps.missing.symptoms",
    "gaps.missing.prevalence",
    "gaps.question.variant",
    "gaps.question.centre",
    "gaps.question.researchers",
    "gaps.question.unregistered",
    "gaps.note",
    "coverage.none",
    "connections.none_found",
    "why.study_registered",
    "why.study_gene",
    "why.study_broader",
    "why.grant",
    "why.org_condition",
    "why.org_gene",
    "why.org_broader",
    "why.researcher",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stxbp1_question_reads_plainly() {
        let p = json!({ "genes": ["STXBP1", "GRIN2A", "GRIN2B"], "more": 2,
                        "process": process("R-HSA-6794361", "Neurexins and neuroligins") });
        assert_eq!(
            text("questions.hypothesis.shared_process", &p),
            "Do changes in STXBP1, GRIN2A and GRIN2B affect how brain cells connect in the same way?"
        );
        let w = text(
            "questions.why.shared_process",
            &json!({ "communities": 8, "more": 2, "process": p["process"] }),
        );
        assert!(
            w.starts_with("In each of these 8 conditions, the gene involved is linked to how brain cells connect."),
            "{w}"
        );
        assert!(w.ends_with("2 more genes are part of this question."), "{w}");
    }

    #[test]
    fn process_lookup_is_specific_first_and_honest_otherwise() {
        assert_eq!(
            process_kind("GO:0031630", "regulation of synaptic vesicle fusion"),
            "release"
        );
        assert_eq!(process_kind("R-HSA-888590", "GABA synthesis"), "calming");
        assert_eq!(
            process_kind(
                "GO:0086002",
                "cardiac muscle cell action potential involved in contraction"
            ),
            "heart_signal"
        );
        assert_eq!(
            process_kind("GO:0032229", "negative regulation of synaptic transmission, GABAergic"),
            "calming"
        );
        assert_eq!(process_kind("GO:0018964", "propylene metabolic process"), "unmapped");
    }

    /// The audit's style rules, checked on every English fallback this module renders.
    #[test]
    fn fallbacks_have_no_ai_tells() {
        let p = json!({ "genes": ["STXBP1", "SNAP25"], "gene": "STXBP1", "condition": "DEE4", "process": "x",
            "kind_label": "clinical trial", "organisation": "Org", "name": "A. Researcher", "papers": 2, "grants": 1,
            "about": "STXBP1", "group": "DEE", "sources": ["Orphanet"], "dates": ["2026-10-03"], "n": 3, "related": 2 });
        let banned = [
            "!",
            "\u{2014}",
            "simply",
            "just ",
            "seamless",
            "empower",
            "unlock",
            "journey",
            "explore",
            "discover",
            "dive ",
            "don't worry",
            "not alone",
            "rare disease atlas",
        ];
        for k in KEYS.iter().chain(PENDING_KEYS.iter()) {
            let t = text(k, &p);
            assert_ne!(t, *k, "no English text for {k}");
            for b in banned {
                assert!(!t.to_lowercase().contains(b), "{k}: '{b}' in {t:?}");
            }
        }
        assert_eq!(
            text(
                "why.researcher",
                &json!({ "name": "A", "papers": 1, "about": "STXBP1", "grants": 2 })
            ),
            "A has written 1 paper about STXBP1 and leads 2 NIH-funded projects."
        );
        assert_eq!(
            text(
                "connections.none_found",
                &json!({ "kind_label": "patient group", "condition": "DEE4", "related": 1 })
            ),
            "No patient group found for DEE4. 1 for a related condition is shown."
        );
        assert!(text("gaps.missing.symptoms", &json!({ "n": 1 })).starts_with("Only 1 symptom of"));
    }

    #[test]
    fn every_key_has_text_and_is_in_the_web_catalog() {
        let en: Value = serde_json::from_str(include_str!("../../../web/messages/en.json")).expect("en catalog");
        for k in KEYS {
            assert_ne!(text(k, &json!({})), k, "no English text for {k}");
            let leaf = k.split('.').fold(&en, |v, part| &v[part]);
            assert!(leaf.is_string(), "{k} missing in web/messages/en.json");
        }
        for (_, kind) in PROCESS_IDS.iter().chain(PROCESS_NAMES.iter()) {
            assert!(
                KEYS.contains(&format!("questions.process.{kind}").as_str()),
                "process {kind} not in KEYS"
            );
        }
    }
}
