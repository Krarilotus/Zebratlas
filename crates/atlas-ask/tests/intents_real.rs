//! Every intent executor on the real atlas (skipped when data/raw is missing). Read-only: uses the
//! snapshots when current, else builds in memory. `ASK_DUMP=1` prints each chip's facts.

mod common;

use atlas_ask::exec::{self, Ctx};
use atlas_ask::facts::FactBook;
use atlas_ask::{Chip, ChipStatus, IntentKind};

fn run(intent: IntentKind, slots: &[(&str, &str)]) -> Option<(Chip, FactBook)> {
    let (atlas, graph) = common::real()?;
    let mut chip = Chip::new(intent, slots);
    chip.id = "c1".into();
    let mut book = FactBook::new();
    let t = std::time::Instant::now();
    exec::execute(Ctx::new(&atlas, &graph), &mut chip, &mut book);
    if std::env::var_os("ASK_DUMP").is_some() {
        eprintln!(
            "\n### {}({slots:?}) {:?} in {:?}\nresolved: {:?}\nnotes: {:?}",
            intent.as_str(),
            chip.status,
            t.elapsed(),
            chip.resolved
                .iter()
                .map(|(k, r)| (k, &r.node.id, &r.node.label))
                .collect::<Vec<_>>(),
            chip.notes
        );
        for f in book.all() {
            eprintln!("[{}] {} <{}> {}", f.key, f.text, f.id, f.url.as_deref().unwrap_or(""));
        }
    }
    Some((chip, book))
}

fn ids(book: &FactBook) -> Vec<&str> {
    book.all().iter().map(|f| f.id.as_str()).collect()
}

#[test]
fn resolve_finds_stxbp1_by_symbol_alias_and_typo() {
    let Some((chip, book)) = run(IntentKind::Resolve, &[("text", "STXBP1")]) else {
        return;
    };
    assert_eq!(chip.status, Some(ChipStatus::Found));
    assert!(ids(&book).contains(&"HGNC:11444"), "{:?}", ids(&book));
    let cond = &chip.resolved["text"].node;
    assert!(cond.id.starts_with("MONDO:"), "{cond:?}");
    let (alias, _) = run(IntentKind::Resolve, &[("text", "Munc18-1")]).unwrap();
    assert_eq!(
        alias.resolved["text"].node.id, cond.id,
        "alias resolves to the same condition"
    );
    let (typo, _) = run(IntentKind::Resolve, &[("text", "Dravet syndrom")]).unwrap();
    assert!(
        typo.resolved["text"].node.label.to_lowercase().contains("dravet"),
        "{:?}",
        typo.resolved
    );
}

#[test]
fn summary_facts_cite_gene_and_phenotype_edges() {
    let Some((chip, book)) = run(IntentKind::ConditionSummaryFacts, &[("condition", "STXBP1")]) else {
        return;
    };
    assert_eq!(chip.status, Some(ChipStatus::Found));
    let mondo = &chip.resolved["condition"].node.id;
    assert!(
        book.all()
            .iter()
            .any(|f| f.edge && f.id == format!("{mondo}|has_associated_gene|HGNC:11444"))
    );
    assert!(book.all().iter().any(|f| f.edge && f.id.contains("|has_phenotype|HP:")));
    assert!(book.all().iter().all(|f| !f.text.is_empty() && !f.id.is_empty()));
}

#[test]
fn connections_and_assets_have_official_channels() {
    let Some((chip, book)) = run(IntentKind::Connections, &[("condition", "STXBP1"), ("kind", "any")]) else {
        return;
    };
    assert!(matches!(chip.status, Some(ChipStatus::Found | ChipStatus::Empty)));
    for f in book.all().iter().filter(|f| !f.edge && f.id.starts_with("NCT")) {
        assert!(
            f.url
                .as_deref()
                .unwrap()
                .starts_with("https://clinicaltrials.gov/study/NCT"),
            "{f:?}"
        );
    }
    let (studies, sbook) = run(
        IntentKind::Assets,
        &[("condition", "Dravet syndrome"), ("kind", "trial"), ("status", "any")],
    )
    .unwrap();
    assert_eq!(studies.status, Some(ChipStatus::Found), "{:?}", studies.notes);
    assert!(
        sbook.all().iter().any(|f| f.edge && f.id.starts_with("NCT")),
        "{:?}",
        ids(&sbook)
    );
}

#[test]
fn related_by_gene_and_symptom() {
    let Some((chip, book)) = run(IntentKind::Related, &[("condition", "SCN1A"), ("by", "gene")]) else {
        return;
    };
    assert_eq!(chip.status, Some(ChipStatus::Found), "{:?}", chip.notes);
    assert!(
        book.all()
            .iter()
            .filter(|f| f.id.ends_with("|has_associated_gene|HGNC:10585"))
            .count()
            >= 2
    );
    let (sym, sbook) = run(IntentKind::Related, &[("condition", "STXBP1"), ("by", "symptom")]).unwrap();
    assert_eq!(sym.status, Some(ChipStatus::Found), "{:?}", sym.notes);
    assert!(sbook.all().iter().any(|f| f.kind == "inferred"));
}

#[test]
fn diseases_with_gene_symptom_and_process() {
    let Some((g, gb)) = run(IntentKind::DiseasesWith, &[("gene", "SCN2A")]) else {
        return;
    };
    assert_eq!(g.status, Some(ChipStatus::Found));
    assert!(gb.len() >= 2);
    let (s, sb) = run(IntentKind::DiseasesWith, &[("symptom", "infantile spasms")]).unwrap();
    assert_eq!(s.status, Some(ChipStatus::Found), "{:?}", s.notes);
    assert!(sb.all()[0].text.contains("conditions in the atlas list"));
    let (p, _) = run(IntentKind::DiseasesWith, &[("process", "synaptic vesicle exocytosis")]).unwrap();
    assert_eq!(p.status, Some(ChipStatus::Empty));
    assert_eq!(p.notes_msg[0]["key"], "ask.note.process_unavailable");
    assert_eq!(p.notes_msg[0]["params"]["process"], "synaptic vesicle exocytosis");
}

#[test]
fn shared_people_path_gaps_details() {
    let Some((sp, _)) = run(IntentKind::SharedPeople, &[("a", "STXBP1"), ("b", "SCN2A")]) else {
        return;
    };
    assert!(
        matches!(sp.status, Some(ChipStatus::Found | ChipStatus::Empty)),
        "{:?}",
        sp.notes
    );
    assert!(sp.notes.iter().any(|n| n.contains("shared")));
    let (path, pbook) = run(IntentKind::Path, &[("from", "STXBP1"), ("to", "SCN1A")]).unwrap();
    assert_eq!(path.status, Some(ChipStatus::Found), "{:?}", path.notes);
    assert!(pbook.all().iter().all(|f| f.edge));
    let (gaps, _) = run(IntentKind::Gaps, &[("condition", "STXBP1")]).unwrap();
    assert!(gaps.status.is_some());
    let (det, dbook) = run(IntentKind::NodeDetails, &[("id", "STXBP1")]).unwrap();
    assert_eq!(det.status, Some(ChipStatus::Found));
    assert!(
        dbook
            .all()
            .iter()
            .any(|f| f.id == "HGNC:11444" || f.id.ends_with("HGNC:11444"))
    );
}

#[test]
fn invalid_chips_say_why() {
    let Some((c, _)) = run(IntentKind::Connections, &[("condition", "qqqzzzxx")]) else {
        return;
    };
    assert_eq!(c.status, Some(ChipStatus::Invalid));
    assert!(c.notes[0].contains("qqqzzzxx"));
    let (c, _) = run(IntentKind::Connections, &[("kind", "pizza")]).unwrap();
    assert_eq!(c.status, Some(ChipStatus::Invalid));
}

#[test]
fn rules_parse_english_and_german() {
    let Some((atlas, graph)) = common::real() else { return };
    let ctx = Ctx::new(&atlas, &graph);
    let en = atlas_ask::rules::parse(&ctx, "Is there a patient group for STXBP1?");
    assert_eq!(en[0].intent, IntentKind::Connections);
    assert_eq!(en[0].slot("condition"), Some("STXBP1"));
    let de = atlas_ask::rules::parse(&ctx, "Meine Tochter hat eine STXBP1-Mutation. Gibt es Studien?");
    assert_eq!(de[0].intent, IntentKind::Assets, "{de:?}");
}
