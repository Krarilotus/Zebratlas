//! Catalog messages pending web integration in copy-audit deliverable 2.
use serde_json::{Value, json};

fn value(p: &Value, key: &str) -> String {
    match &p[key] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

pub fn msg(key: &str, p: Value) -> Value {
    let fallback = text(key, &p);
    json!({"key": key, "params": p, "fallback": fallback})
}

pub fn text(key: &str, p: &Value) -> String {
    match key {
        "api.error.matcher_busy" => "The matcher is busy. Try again shortly.".into(),
        "api.error.phenotype_bounds" => "Too many or overlong phenotype identifiers.".into(),
        "api.error.uri_too_long" => "The request address is too long.".into(),
        "questions.clinvar_counts" => "These counts refer to ClinVar submissions. They do not count patients or families.".into(),
        "questions.drug_evidence" => "Open Targets records a clinical stage for this drug. This does not show that it works for these conditions.".into(),
        "questions.design.assay" => format!("Ask a domain expert to select a quantitative assay of {} and check that it measures the relevant function in both conditions.", value(p, "pathway")),
        "questions.design.controls" => "Compare models with verified variants from both conditions against matched corrected controls. Randomize batches and blind the analysis.".into(),
        "questions.design.rescue" => "Include a control that changes the selected pathway and a correction or rescue condition for each disease model. Check cell health to distinguish pathway effects from nonspecific damage.".into(),
        "questions.design.strata" => "Analyze each condition and group of variant effects separately before comparing effects and rescue responses.".into(),
        "questions.validate.annotation" => "A pathway annotation does not establish a shared disease mechanism or treatment response.".into(),
        "questions.validate.models" => "An expert must select a specific pathway measurement, relevant cell type and verified models for the variants. Check whether these models are available.".into(),
        "questions.validate.plan" => "After estimating variation in a pilot, pre-register effect direction and decision criteria. Plan independent biological replicates and sample size with a statistician.".into(),
        "questions.validate.effects" => "Check each variant's effect and inheritance before combining condition groups. Analyze opposite effects in separate groups.".into(),
        "questions.validate.findings" => "Review uncertainty findings linked to these conditions before interpreting a shared assay.".into(),
        "questions.proposal.question" => format!("Test whether variants associated with {} disrupt the same measurable function in {}.", value(p, "conditions"), value(p, "pathway")),
        "questions.proposal.support" => "Both condition models show the planned pathway change compared with controls, and correction restores the same measurement in both. Replicate the result independently.".into(),
        "questions.proposal.challenge" => "Only one condition changes the measurement, effects point in incompatible directions, or differences disappear after accounting for cell health or batch.".into(),
        "facts.person_author" => format!("{} is an author of {}.", value(p, "name"), value(p, "arg0")),
        "facts.person_same" => format!("{} is the same person as {}.", value(p, "name"), value(p, "other")),
        "verify.record.partial" => "Checked records match; some records have not been checked.".into(),
        "verify.record.unavailable" => "No source records are available to check.".into(),
        "facts.person_lead" => format!("{} leads {}.", value(p, "name"), value(p, "arg0")),
        "questions.source.clinvar" => format!("{}: {} gene variants recorded in ClinVar.", value(p, "gene"), value(p, "variants")),
        "cluster.shared_records" => format!("Processes recorded in this group: {}. Symptoms recorded in this group: {}.", value(p, "processes"), value(p, "symptoms")),
        "cluster.counterexample" => format!("{} was assigned to a different group in this analysis. Check the recorded similarities and differences before sharing research findings.", value(p, "condition")),
        "verify.unreadable" => "The source could not be read or checked.".into(),
        "related.shared_gene" => format!("Both conditions involve {}. Recorded effects: {} in this condition; {} in the other.", value(p, "gene"), value(p, "here"), value(p, "there")),
        "related.effect_conflict" => format!("{} has different recorded effects: {} in this condition; {} in the other.", value(p, "gene"), value(p, "here"), value(p, "there")),
        "related.shared_processes" => format!("The linked genes share these processes in the body: {}.", value(p, "processes")),
        "related.shared_symptoms" => format!("Shared recorded symptoms: {}.", value(p, "symptoms")),
        "related.cause_unknown" => "The sources checked record no causal gene for at least one condition. The cause cannot be compared from these records.".into(),
        "related.counter.same_gene" => format!("The recorded effects of {} differ between these conditions. Findings for one may not apply to the other.", value(p, "gene")),
        "related.counter.within_condition" => format!("Gene2Phenotype records different effects of {} within this condition. Check which applies to the gene variant being studied.", value(p, "gene")),
        "related.counter.lookalike" => "These conditions share symptoms, but their recorded gene processes differ. Similar symptoms alone do not establish a shared cause.".into(),
        "api.error.internal" => "We could not complete this request. Try again.".into(),
        "api.error.focus_required" => "Choose an entry.".into(),
        "api.error.unit_limit" => "Limit must be between 1 and 500.".into(),
        "api.error.condition_unknown" => format!("Condition {} was not found.", value(p, "id")),
        "api.error.condition_retired" => format!("Entry {} has been retired. Open its replacement.", value(p, "id")),
        "api.error.item_unknown" => format!("Record {} was not found.", value(p, "id")),
        "api.error.scorer_unknown" => format!("Scorer {} is unknown. Choose fusion, atlas or resnik.", value(p, "name")),
        "api.error.gene_unknown" => format!("Gene {} was not found.", value(p, "id")),
        "api.error.job_unknown" => format!("Task {} is unknown.", value(p, "job")),
        "api.error.question_unknown" => format!("Research question {} was not found.", value(p, "id")),
        "api.error.question_id" => "Use a question ID in the form condition~pathway.".into(),
        "api.error.cluster_unknown" => format!("No research cluster was found for {}.", value(p, "id")),
        "api.error.card_unknown" => "This contact is not linked to the selected condition.".into(),
        "api.error.llm_unavailable" => "The writing assistant is unavailable.".into(),
        "api.related.warming_up" => "Related conditions are loading. Try again in a few seconds.".into(),
        "api.related.unavailable" => "Related conditions are unavailable. Try again later.".into(),
        "api.questions.warming_up" => "Research questions are loading. Try again in a few seconds.".into(),
        "api.questions.unavailable" => "Research questions are unavailable. Try again later.".into(),
        "api.clusters.warming_up" => "Research clusters are loading. Try again in a few seconds.".into(),
        "resolve.reconcile.unconfigured" => "The assistant is unavailable. Search results use recorded names and codes.".into(),
        "resolve.reconcile.certain" => "A matching condition was found in the recorded names and codes.".into(),
        "resolve.reconcile.no_candidates" => "No recorded condition was found for the assistant to check.".into(),
        "verify.record.matched" => "All checked source records match their stored fingerprints.".into(),
        "verify.record.changed" => "A checked source record has changed or could not be read.".into(),
        "verify.file.matched" => "All checked source files match the fingerprints recorded when this entry was built.".into(),
        "verify.file.changed" => "A source file has changed or could not be read.".into(),
        "models.note" => "Check this model's availability, gene variant and relevance to your condition with its owner.".into(),
        "questions.list_note" => "These questions need testing. They are ordered by the recorded evidence and the communities involved.".into(),
        "questions.rank_note" => "The order reflects the communities involved, support in the records and recorded studies or researchers. It does not show how likely a hypothesis is to be true.".into(),
        "cluster.stable" => "This grouping persisted when the analysis was repeated with different starting points and samples.".into(),
        "cluster.unstable" => "This grouping changed when the analysis was repeated with different starting points or samples.".into(),
        "facts.study_indexed" => format!("{arg0} ({arg1}) is indexed with {arg2}, the MeSH term of {label}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1"), arg2 = value(p, "arg2"), label = value(p, "label")),
        "facts.study_registered" => format!("{arg0} ({arg1}) is registered for {label}: {arg2}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1"), label = value(p, "label"), arg2 = value(p, "arg2")),
        "facts.organisation_condition" => format!("{arg0} serves people with {label}", arg0 = value(p, "arg0"), label = value(p, "label")),
        "facts.gene_condition" => format!("{symbol} is a gene that causes {label}", symbol = value(p, "symbol"), label = value(p, "label")),
        "facts.study_record" => format!("{arg0} ({arg1}): {arg2}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1"), arg2 = value(p, "arg2")),
        "facts.organisation_gene" => format!("{arg0} serves people with {symbol} variants", arg0 = value(p, "arg0"), symbol = value(p, "symbol")),
        "facts.condition_parent" => format!("{label} is a kind of {anc_name} ({anc}, MONDO hierarchy)", label = value(p, "label"), anc_name = value(p, "anc_name"), anc = value(p, "anc")),
        "facts.study_parent" => format!("{arg0} ({arg1}) is registered for {anc_name}: {arg2}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1"), anc_name = value(p, "anc_name"), arg2 = value(p, "arg2")),
        "facts.organisation_parent" => format!("{arg0} serves people with {anc_name}", arg0 = value(p, "arg0"), anc_name = value(p, "anc_name")),
        "facts.work_record" => format!("{arg0} {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        "facts.work_gene" => format!("{arg0} names {symbol} ({arg1})", arg0 = value(p, "arg0"), symbol = value(p, "symbol"), arg1 = value(p, "arg1")),
        "facts.condition_onset" => format!("Age at first symptoms recorded for {arg0}: {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        "facts.condition_prevalence" => format!("{arg0} {arg1}: {arg2} ({arg3})", arg0 = value(p, "arg0"), arg1 = value(p, "arg1"), arg2 = value(p, "arg2"), arg3 = value(p, "arg3")),
        "facts.condition_symptom" => format!("{arg0} is a recorded feature of {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        "facts.condition_causal_gene" => format!("Changes in {symbol} are recorded as a cause of {arg0}", arg0 = value(p, "arg0"), symbol = value(p, "symbol")),
        "facts.condition_definition" => format!("{arg0}: {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        _ => panic!("unknown copy key: {key}"),
    }
}
