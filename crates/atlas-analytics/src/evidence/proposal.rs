use super::*;
use atlas_core::node::EdgeKind;

/// Hypothesis only. Two diseases need a positive sourced shared pathway; sharing a gene or
/// a generic LoF label is insufficient. No model, biomarker or researcher is invented.
pub fn propose_shared_mechanism(review: &Review, disease_a: &str, disease_b: &str) -> Result<Proposal, String> {
    if disease_a == disease_b {
        return Err("Two distinct disease IDs are required".into());
    }
    let for_disease = |id: &str| review.statements.iter().filter(|s| s.subject == id).collect::<Vec<_>>();
    let a = for_disease(disease_a);
    let b = for_disease(disease_b);
    if a.is_empty() || b.is_empty() {
        return Err("No supplied assertions for one or both disease IDs".into());
    }
    let positive = |s: &&Statement| {
        s.value == Value::Present
            && s.relation == Relation::Pathway
            && !matches!(s.curation, Curation::Refuted | Curation::Disputed)
            && s.kind != EdgeKind::Hypothesis
            && (s.kind != EdgeKind::Extracted || s.quote.as_ref().is_some_and(|q| !q.trim().is_empty()))
            && !s.citations.is_empty()
    };
    let shared = a.iter().copied().filter(positive).find_map(|x| {
        b.iter()
            .copied()
            .filter(positive)
            .find(|y| y.object == x.object)
            .map(|y| (x, y))
    });
    let mut activity = review.activity.clone();
    activity.id = format!("activity:shared-mechanism-question-v1:{disease_a}:{disease_b}");
    activity.label = "Derive a research question from supplied pathway evidence".into();
    activity.parameters.insert("disease_a".into(), disease_a.into());
    activity.parameters.insert("disease_b".into(), disease_b.into());
    activity.counts.clear();
    activity.count("examined_assertions", (a.len() + b.len()) as u64);
    let Some((x, y)) = shared else {
        return Ok(Proposal { kind:EdgeKind::Hypothesis, policy_version:POLICY_VERSION.into(),activity,
            question:"Is there evidence that these conditions affect the same biological process?".into(),
            rationale:"No positive shared pathway assertion was found in the supplied graph slice.".into(),
            statement_ids:Vec::new(),citations:Vec::new(),
            design:vec!["Ask a mechanism curator to map the reported variants and functional evidence to a specific common pathway before planning a shared assay.".into()],
            would_support:"Independent functional evidence mapping both conditions to the same specific process.".into(),
            would_challenge:"Evidence for separate processes or incompatible variant effects.".into(),
            must_validate:vec!["This is a coverage gap, not evidence that the diseases have no shared mechanism.".into()] });
    };
    let relevant: Vec<_> = review
        .findings
        .iter()
        .filter(|f| f.statements.iter().any(|id| a.iter().chain(&b).any(|s| &s.id == id)))
        .collect();
    let mut inputs = vec![x, y];
    // Carry mechanism assertions as conditions on the experiment, never as proof of equivalence.
    inputs.extend(
        a.iter()
            .chain(&b)
            .copied()
            .filter(|s| s.relation == Relation::Mechanism && s.value != Value::Unknown),
    );
    inputs.sort_by(|a, b| a.id.cmp(&b.id));
    inputs.dedup_by(|a, b| a.id == b.id);
    activity.count("supporting_and_context_assertions", inputs.len() as u64);
    let mut must_validate = vec!["A pathway annotation does not establish a shared disease mechanism or treatment response.".into(),
        "An expert must choose a specific pathway readout, relevant cell type and verified variant-specific models; their availability is not established here.".into(),
        "Pre-register effect direction and decision criteria after pilot variance estimates; choose independent biological replicates and sample size with a statistician.".into(),
        "Resolve each variant's effect and inheritance before combining disease groups; opposite effects require separate strata.".into()];
    if !relevant.is_empty() {
        must_validate.push(format!(
            "Review {} uncertainty findings attached to these diseases before interpreting a shared assay.",
            relevant.len()
        ));
    }
    Ok(Proposal {kind:EdgeKind::Hypothesis,policy_version:POLICY_VERSION.into(),activity,
        question:format!("Do variants associated with {disease_a} and {disease_b} disrupt the same measurable function in {}?",x.object),
        rationale:format!("Supplied assertions {} and {} connect the two conditions to pathway {}. This motivates a test, not a claim of equivalence.",x.id,y.id,x.object),
        statement_ids:inputs.iter().map(|s| s.id.clone()).collect(),citations:inputs.iter().flat_map(|s| s.citations.clone()).collect(),
        design:vec![format!("Ask a domain expert to select one quantitative assay of {} and verify that it measures the relevant function in both conditions.",x.object),
            "Compare variant-specific models from both conditions with matched corrected controls in the same assay; randomize batches and blind analysis.".into(),
            "Include a control perturbation of the chosen pathway and a correction/rescue condition for each disease model. Check general cell health to distinguish pathway effects from nonspecific damage.".into(),
            "Analyze each condition and variant-effect stratum separately before testing whether effects and rescue responses agree.".into()],
        would_support:"Both disease models show the pre-specified pathway defect relative to controls, and correction restores the same readout in both; replicate independently.".into(),
        would_challenge:"Only one condition changes the readout, effects point in incompatible directions, or differences disappear after controlling for cell health or batch.".into(),must_validate })
}
