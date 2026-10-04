//! Deterministic writing templates. Catalog translations arrive in deliverable 2.
use serde_json::{Value, json};

pub fn msg(key: &str, p: Value) -> Value {
    let get = |k: &str| p[k].as_str().unwrap_or_default();
    let fallback = match key {
        "write.template.greeting" => format!("Hello {},", get("recipient")),
        "write.template.role.parent" => format!("Our child has been diagnosed with {}.", get("condition")),
        "write.template.role.patient" => format!("I live with {}.", get("condition")),
        "write.template.role.carer" => format!("I care for someone with {}.", get("condition")),
        "write.template.role.group_leader" => format!("I lead a patient group for {}.", get("condition")),
        "write.template.role.researcher" => format!("I am a researcher working on {}.", get("condition")),
        "write.template.role.clinician" => format!("I am a clinician caring for people with {}.", get("condition")),
        "write.template.found" => "I found your contact on Zebratlas.".into(),
        "write.template.check_links" => "Experts would need to assess these links before we plan work together.".into(),
        "write.template.meeting" => "Could we arrange a 30-minute call in the next few weeks?".into(),
        "write.template.ask" => "Could you tell us how we can take part, or whom we should contact?".into(),
        "write.template.closing" => "Kind regards".into(),
        "write.template.closing_named" => format!("Kind regards,\n{}", get("sender")),
        "write.template.subject" => format!("Question about {}", get("condition")),
        _ => panic!("unknown writing template: {key}"),
    };
    json!({"key": key, "params": p, "fallback": fallback})
}

/// Preserve the existing German template until the translation pass; message fallbacks stay English.
pub fn legacy_text(message: &Value, lang: &str) -> String {
    if !lang.to_ascii_lowercase().starts_with("de") {
        return message["fallback"].as_str().unwrap_or_default().to_owned();
    }
    let p = &message["params"];
    let get = |k: &str| p[k].as_str().unwrap_or_default();
    match message["key"].as_str().unwrap_or_default() {
        "write.template.greeting" => format!("Hallo {},", get("recipient")),
        "write.template.role.parent" => format!("Unser Kind hat die Diagnose {}.", get("condition")),
        "write.template.role.patient" => format!("Ich lebe mit {}.", get("condition")),
        "write.template.role.carer" => format!("Ich betreue einen Menschen mit {}.", get("condition")),
        "write.template.role.group_leader" => format!("Ich leite eine Patientengruppe für {}.", get("condition")),
        "write.template.role.researcher" => format!("Ich forsche zu {}.", get("condition")),
        "write.template.role.clinician" => format!("Ich behandle Menschen mit {}.", get("condition")),
        "write.template.found" => "Ich habe Sie über Zebratlas gefunden.".into(),
        "write.template.check_links" => {
            "Fachleute müssten diese Verbindungen bestätigen, bevor wir gemeinsam etwas planen.".into()
        }
        "write.template.meeting" => "Hätten Sie in den nächsten Wochen Zeit für ein Gespräch von 30 Minuten?".into(),
        "write.template.ask" => {
            "Könnten Sie uns sagen, wie wir teilnehmen können oder an wen wir uns wenden sollten?".into()
        }
        "write.template.closing" => "Mit freundlichen Grüßen".into(),
        "write.template.closing_named" => format!("Mit freundlichen Grüßen\n{}", get("sender")),
        "write.template.subject" => format!("Anfrage zu {}", get("condition")),
        _ => message["fallback"].as_str().unwrap_or_default().to_owned(),
    }
}
