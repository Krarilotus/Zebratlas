//! Identifier removal before any model sees a document (D41). Rule-based, de/en/fr, ordered:
//! e-mail, IBAN, labelled dates of birth, labelled record / insurance / case numbers, phone numbers,
//! street addresses and postcode lines, names (after salutations and titles, on label lines, before
//! degrees, after sign-offs), then every later mention of a removed name's words.
//!
//! It errs towards removing: a removed symptom word costs less than a leaked name. Only counts leave
//! this module besides the redacted text; nothing is logged.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Name,
    DateOfBirth,
    Address,
    Phone,
    Email,
    RecordNumber,
    InsuranceNumber,
    Iban,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::DateOfBirth => "date_of_birth",
            Self::Address => "address",
            Self::Phone => "phone",
            Self::Email => "email",
            Self::RecordNumber => "record_number",
            Self::InsuranceNumber => "insurance_number",
            Self::Iban => "iban",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Name => "[NAME]",
            Self::DateOfBirth => "[DATE-OF-BIRTH]",
            Self::Address => "[ADDRESS]",
            Self::Phone => "[PHONE]",
            Self::Email => "[EMAIL]",
            Self::RecordNumber => "[RECORD-NO]",
            Self::InsuranceNumber => "[INSURANCE-NO]",
            Self::Iban => "[IBAN]",
        }
    }
}

/// The redacted text and how many identifiers of each kind were removed.
#[derive(Clone, Debug, Default)]
pub struct Redacted {
    pub text: String,
    /// Kinds with at least one removal. Names count distinct people; addresses count address blocks.
    pub counts: BTreeMap<Kind, usize>,
}

impl Redacted {
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
    kind: Kind,
}

// ---------------------------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------------------------

const MONTHS: &str = "jan(?:uary|uar|vier)?|feb(?:ruary|ruar)?|f[ée]vr(?:ier)?|m[äa]r(?:ch|z|s)?|apr(?:il)?|avr(?:il)?|ma[iy]|mai|june?|juni?|juin|july?|juli?|juil(?:let)?|aug(?:ust)?|ao[ûu]t|sep(?:t(?:ember|embre)?)?|o[ck]t(?:ober|obre)?|nov(?:ember|embre)?|de[cz](?:ember)?|d[ée]c(?:embre)?";

fn date_pattern() -> String {
    format!(
        r"(?:\d{{1,2}}[./-]\d{{1,2}}[./-](?:\d{{4}}|\d{{2}})|\d{{4}}-\d{{2}}-\d{{2}}|\d{{1,2}}(?:\.|er)?[ \t]+(?i:{MONTHS})\.?[ \t]+\d{{4}}|(?i:{MONTHS})\.?[ \t]+\d{{1,2}}(?:st|nd|rd|th)?,?[ \t]+\d{{4}})"
    )
}

/// One capitalised name word, an initial, or a name particle followed by a word.
const WORD: &str = r"(?:\p{Lu}[\p{Ll}'’]+(?:-\p{Lu}[\p{Ll}'’]+)?|\p{Lu}{2,}(?:-\p{Lu}{2,})?)\b";
const PARTICLE: &str = r"(?:von|van|der|den|de|du|da|di|le|la|zu|ter|dos|del|af)";

fn name_pattern(min_words: usize) -> String {
    let unit = format!(r"(?:{WORD}|\p{{Lu}}\.)");
    let more = format!(r"(?:[ \t]+(?:{PARTICLE}[ \t]+)?{unit})");
    let (lo, hi) = (min_words.saturating_sub(1), 3);
    format!(r"(?:{PARTICLE}[ \t]+)?{WORD}{more}{{{lo},{hi}}}")
}

const SALUTATION: &str = r"(?:Herrn?|Frau|Hr\.|Fr\.|Mr\.?|Mrs\.?|Ms\.?|Miss|Mx\.?|Monsieur|Madame|Mademoiselle|Mme\.?|Mlle\.?|M\.|Dr\.?|Prof\.?|Pr\.|Professeur|Docteur|Dear|Liebe[rs]?|Lieber|Cher|Ch[èe]re|geehrte[rs]?)";
const TITLE: &str = r"(?:Dr\.?|Prof\.?|med\.|rer\.|nat\.|dent\.|univ\.|PD|Priv\.-Doz\.|Dipl\.-[\p{L}]+\.?|Frau|Herrn?|le|la|Docteur|Professeur|Pr\.|Dr\.\s?med\.)";

/// Words that follow a salutation but are not names.
const NOT_NAMES: &[&str] = &[
    "Damen",
    "Herren",
    "Sir",
    "Madam",
    "Colleague",
    "Colleagues",
    "Kollegin",
    "Kollege",
    "Kolleginnen",
    "Kollegen",
    "Collègue",
    "Collègues",
    "Confrère",
    "Consœur",
    "Consoeur",
    "Team",
    "Doctor",
    "Doktor",
    "Patient",
    "Patientin",
    "Parents",
    "Eltern",
    "Family",
    "Familie",
    "Famille",
    "All",
    "Alle",
    "Madame",
    "Monsieur",
    "Sirs",
    "Mesdames",
    "Messieurs",
    "Professor",
    "Professorin",
    "Dr",
    "Prof",
    "Herr",
    "Herrn",
    "Frau",
    "Mr",
    "Mrs",
    "Ms",
    "Mme",
    "Mlle",
    "Pr",
    "Docteur",
    "Professeur",
];

struct Rule {
    re: Regex,
    group: usize,
    kind: Kind,
}

fn rule(pattern: &str, group: usize, kind: Kind) -> Rule {
    Rule {
        re: Regex::new(pattern).unwrap_or_else(|e| panic!("redaction pattern: {e}")),
        group,
        kind,
    }
}

static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("^(?:{})$", date_pattern())).expect("date"));
static DATE_AFTER_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^[ \t]*[,(][ \t]*(?:\*[ \t]*)?({})", date_pattern())).expect("date after"));

/// Rules other than names, in priority order.
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let date = date_pattern();
    let value = r"([A-Z0-9]*\d[A-Z0-9]*(?:[./ -][A-Z0-9]*\d[A-Z0-9]*){0,4})";
    let record = r"Patienten-?(?:nummer|nr\.?|ID)|Pat\.?-?(?:Nr|ID)\.?|PID|Fall-?(?:nummer|nr\.?)|Aufnahme-?(?:nummer|nr\.?)|Aktenzeichen|Az\.|Unser Zeichen|Befund-?(?:nummer|nr\.?)|Labor-?(?:nummer|nr\.?)|Auftrags-?(?:nummer|nr\.?)|Proben-?(?:nummer|nr\.?)|Einsende-?(?:nummer|nr\.?)|MRN|Medical record(?: number| no\.?)?|Hospital (?:number|no\.?)|Record (?:number|no\.?)|Case (?:number|no\.?|ID)|Patient (?:ID|number|no\.?)|Sample (?:ID|number|no\.?)|Lab(?:oratory)? (?:ID|number|no\.?)|Order (?:ID|number|no\.?)|Our ref(?:erence)?\.?|Your ref(?:erence)?\.?|N°[ \t]*(?:de[ \t]+)?dossier|Num[ée]ro de dossier|IPP|NIP|N°[ \t]*patient|Num[ée]ro (?:de )?patient|N°[ \t]*(?:d'|de )?[ée]chantillon|R[ée]f\.";
    let insurance = r"Versicherten-?(?:nummer|nr\.?)|Versicherungs-?(?:nummer|nr\.?)|KVNR|Krankenversichertennummer|Kassen-?(?:nummer|nr\.?)|Insurance (?:number|no\.?|ID)|Policy (?:number|no\.?)|Member (?:ID|number)|NHS (?:number|no\.?)|Medicaid(?: ID| number)?|Medicare(?: ID| number)?|SSN|Social security(?: number| no\.?)?|N°[ \t]*(?:de[ \t]+)?s[ée]curit[ée] sociale|Num[ée]ro de s[ée]curit[ée] sociale|N°[ \t]*SS|NIR|Carte vitale";
    vec![
        rule(r"[\p{L}0-9._%+-]+@[\p{L}0-9-]+(?:\.[\p{L}0-9-]+)+", 0, Kind::Email),
        rule(
            r"\b[A-Z]{2}\d{2}[ ]?(?:[A-Z0-9]{4}[ ]?){2,7}[A-Z0-9]{1,4}\b",
            0,
            Kind::Iban,
        ),
        rule(
            &format!(
                r"(?i:geb(?:oren)?\.?(?:[ \t]+am)?|Geburtsdatum|Geb\.-?Dat(?:um|\.)?|date of birth|DOB|D\.O\.B\.?|born(?:[ \t]+on)?|n[ée]e?[ \t]+le|date de naissance|DDN)[ \t]*:?[ \t]*({date})"
            ),
            1,
            Kind::DateOfBirth,
        ),
        rule(&format!(r"(?:^|[\s(])\*[ \t]?({date})"), 1, Kind::DateOfBirth),
        rule(
            &format!(r"(?i:{insurance})[ \t]*[:#]?[ \t]*(?:No\.?|Nr\.?|N°)?[ \t]*{value}"),
            1,
            Kind::InsuranceNumber,
        ),
        rule(
            &format!(r"(?i:{record})[ \t]*[:#]?[ \t]*(?:No\.?|Nr\.?|N°)?[ \t]*{value}"),
            1,
            Kind::RecordNumber,
        ),
        // German health insurance number (KVNR) and French NIR, unlabelled
        rule(r"\b[A-Z]\d{9}\b", 0, Kind::InsuranceNumber),
        rule(
            r"\b[12][ ]?\d{2}[ ]?(?:0[1-9]|1[0-2])[ ]?(?:\d{2}|2[AB])[ ]?\d{3}[ ]?\d{3}(?:[ ]?\d{2})?\b",
            0,
            Kind::InsuranceNumber,
        ),
        rule(
            r"(?i:\b(?:Tel(?:efon)?|Phone|Telephone|Fax|Mobil(?:e|telefon)?|Handy|T[ée]l(?:[ée]phone)?|Portable|Cell)\.?)[ \t]*(?:\([\p{L} ]+\))?[ \t]*[:.]?[ \t]*(\+?\d[\d \t/().-]{5,}\d)",
            1,
            Kind::Phone,
        ),
        rule(
            r"(?:\+|\b00)\d{1,3}[ \t./-]?(?:\(0\)[ \t]?)?\d{1,5}(?:[ \t./-]?\d{2,}){1,4}\b",
            0,
            Kind::Phone,
        ),
        rule(r"\b0\d{1,4}(?:[ \t/.-]\d{2,}){2,5}\b", 0, Kind::Phone),
        // streets: German compounds, English and French forms
        rule(
            r"\b(?:\p{Lu}[\p{L}.-]*[ \t-])?\p{Lu}[\p{L}-]*(?:straße|strasse|str\.|weg|gasse|platz|allee|chaussee)[ \t]+\d{1,4}[ \t]?[a-zA-Z]?\b",
            0,
            Kind::Address,
        ),
        rule(
            r"\b(?:\p{Lu}[\p{L}-]*[ \t]+)?(?:Straße|Strasse|Str\.|Weg|Gasse|Platz|Allee)[ \t]+\d{1,4}[ \t]?[a-zA-Z]?\b",
            0,
            Kind::Address,
        ),
        rule(
            r"\b\d{1,5}[ \t]+(?:\p{Lu}[\p{L}'-]+[ \t]+){1,3}(?:Street|St\.|Road|Rd\.?|Avenue|Ave\.?|Lane|Ln\.?|Drive|Close|Way|Court|Ct\.?|Boulevard|Blvd\.?|Place|Terrace|Crescent|Gardens|Grove|Square)\b",
            0,
            Kind::Address,
        ),
        rule(
            r"\b\d{1,4}(?:[ \t]?(?:bis|ter))?,?[ \t]+(?i:rue|avenue|av\.|boulevard|bd|chemin|place|all[ée]e|impasse|quai|cours|route|square)[ \t]+[^\n,]{2,40}",
            0,
            Kind::Address,
        ),
        rule(
            r"\b(?:D-|F-|DE-|FR-)?\d{5}[ \t]+\p{Lu}[\p{L}-]+(?:[ \t](?:\p{Lu}[\p{L}-]+|a\.|am|an|der|sur|en|le|la|Cedex|CEDEX|\d{1,2}e?))*",
            0,
            Kind::Address,
        ),
        rule(r"\b[A-Z]{1,2}\d[A-Z\d]?[ \t]+\d[A-Z]{2}\b", 0, Kind::Address),
        rule(
            r"\b(?:\p{Lu}[\p{L}]+[ \t]?)+,[ \t]*[A-Z]{2}[ \t]+\d{5}(?:-\d{4})?\b",
            0,
            Kind::Address,
        ),
    ]
});

/// Name rules: (pattern, group). Each captures one name.
static NAME_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let name = name_pattern(1);
    let name2 = name_pattern(2);
    let label = r"Patient(?:in)?|Patientenname|Name(?:,[ \t]*Vorname)?(?:[ \t]+des[ \t]+Patienten|[ \t]+der[ \t]+Patientin)?|Vorname|Nachname|Nom(?:[ \t]+du[ \t]+patient|[ \t]+de[ \t]+naissance|[ \t]+et[ \t]+pr[ée]nom)?|Pr[ée]nom|Patient name|Full name|Surname|First name|Given name|Mother|Father|Mutter|Vater|M[èe]re|P[èe]re|Parents?|Eltern|Kind|Child|Enfant|Unterschrift|Signature|Signed|Arzt|[ÄA]rztin|M[ée]decin(?:[ \t]+traitant)?|Physician|Referring physician|[ÜU]berweisender Arzt|Hausarzt|Kinderarzt|Kinder[äa]rztin|Contact|Ansprechpartner(?:in)?|Interlocuteur";
    let degree = r"MD|M\.D\.|PhD|Ph\.D\.|FRCPCH|FRCP|MBBS|MRCPCH|Oberärztin|Oberarzt|Fachärztin|Facharzt|Assistenzärztin|Assistenzarzt|Chefärztin|Chefarzt|Consultant|Registrar|Genetic Counsellor|Médecin|Praticien hospitalier|Docteur en médecine";
    let context = r"[ \t]*(?:,|\(|\*|geb|born|n[ée]e?\b|DOB|D\.O\.B|aged|age\b|im Alter|[âa]g[ée]e?\b|\d)";
    let relation = r"(?i:Ihre[rn]?[ \t]+Patient(?:in)?|unsere[rn]?[ \t]+Patient(?:in)?|your patient|our patient|votre patiente?|notre patiente?|Patient(?:in|e)?|Sohn|Tochter|son|daughter|fils|fille|Kind|child|enfant)";
    let signoff = r"(?:Grüßen|Grüße|Gruß|regards|Regards|Sincerely|sincerely|faithfully|Cordialement|cordialement|distingu[ée]es|confraternelles?|confraternellement)";
    vec![
        rule(
            &format!(r"\b{SALUTATION}(?:[ \t]+{TITLE})*[ \t]+({name})"),
            1,
            Kind::Name,
        ),
        rule(
            &format!(r"(?m)^[ \t]*(?i:{label})[ \t]*:[ \t]*({name}(?:,[ \t]*{name})?)"),
            1,
            Kind::Name,
        ),
        rule(&format!(r"\b{relation}[ \t]+({name}){context}"), 1, Kind::Name),
        rule(&format!(r"({name2})[ \t]*,?[ \t]*(?:{degree})\b"), 1, Kind::Name),
        // a name right before a birth marker ("Lena Schmidt, geb. …", "Oliver Smith (DOB …")
        rule(
            &format!(r"({name2})[ \t]*[,(]?[ \t]*(?:geb(?:oren)?\.?|born|n[ée]e?[ \t]+le|DOB|D\.O\.B|\*[ \t]?\d)"),
            1,
            Kind::Name,
        ),
        rule(
            &format!(r"{signoff}[,.!]?[ \t]*\n(?:[ \t]*\n){{0,3}}[ \t]*(?:{TITLE}[ \t]+)*({name2})"),
            1,
            Kind::Name,
        ),
        rule(
            &format!(r"(?i:\b(?:Re|Betreff|Objet|Concerne)[ \t]*:)[ \t]*({name2}){context}"),
            1,
            Kind::Name,
        ),
    ]
});

static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}'’-]+").expect("word"));

// ---------------------------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------------------------

/// Remove identifiers. Pure function of the text.
pub fn redact(text: &str) -> Redacted {
    let mut spans: Vec<Span> = Vec::new();
    for r in RULES.iter() {
        for c in r.re.captures_iter(text) {
            let Some(m) = c.get(r.group) else { continue };
            // a "phone" that is really a date is left to the date rules
            if r.kind == Kind::Phone && DATE.is_match(m.as_str().trim()) {
                continue;
            }
            // "ORPHA:33069 Dravet" is an identifier of a condition, not a postcode
            if r.kind == Kind::Address && text[..m.start()].ends_with(':') {
                continue;
            }
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: r.kind,
            });
        }
    }
    // names, and dates of birth right after a name ("Max Muster, 12.03.2015")
    let mut people: BTreeSet<String> = BTreeSet::new();
    let mut name_words: BTreeSet<String> = BTreeSet::new();
    for r in NAME_RULES.iter() {
        for c in r.re.captures_iter(text) {
            let Some(m) = c.get(r.group) else { continue };
            let name = m.as_str().trim();
            let first = name.split([' ', '\t', ',']).next().unwrap_or("");
            if NOT_NAMES.contains(&first) || name.chars().count() < 2 {
                continue;
            }
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Name,
            });
            people.insert(person_key(name));
            for w in WORD_RE.find_iter(name) {
                let w = w.as_str().trim_end_matches('.');
                if w.chars().count() >= 3 && !is_particle(w) && !NOT_NAMES.contains(&w) {
                    name_words.insert(w.to_owned());
                }
            }
            if let Some(d) = DATE_AFTER_NAME.captures(&text[m.end()..]).and_then(|c| c.get(1)) {
                spans.push(Span {
                    start: m.end() + d.start(),
                    end: m.end() + d.end(),
                    kind: Kind::DateOfBirth,
                });
            }
        }
    }
    // later mentions of a removed name's words ("Emma presented with …")
    for w in &name_words {
        let re = Regex::new(&format!(r"\b{}\b", regex::escape(w))).expect("escaped word");
        for m in re.find_iter(text) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Name,
            });
        }
    }
    let kept = merge(spans);
    let mut counts: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut last_address_end: Option<usize> = None;
    for s in &kept {
        match s.kind {
            Kind::Name => {}
            Kind::Address => {
                // street + postcode line of one address count once
                if last_address_end.is_none_or(|e| s.start > e + 80) {
                    *counts.entry(Kind::Address).or_default() += 1;
                }
                last_address_end = Some(s.end);
            }
            k => *counts.entry(k).or_default() += 1,
        }
    }
    let named = merge_people(&people);
    if named > 0 {
        counts.insert(Kind::Name, named);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for s in &kept {
        out.push_str(&text[at..s.start]);
        out.push_str(s.kind.placeholder());
        at = s.end;
    }
    out.push_str(&text[at..]);
    Redacted { text: out, counts }
}

fn is_particle(w: &str) -> bool {
    matches!(
        w,
        "von" | "van" | "der" | "den" | "de" | "du" | "da" | "di" | "le" | "la" | "zu" | "ter" | "dos" | "del" | "af"
    )
}

/// A person's key: their name words, sorted ("Muster, Max" = "Max Muster").
fn person_key(name: &str) -> String {
    let mut words: Vec<&str> = WORD_RE
        .find_iter(name)
        .map(|m| m.as_str())
        .filter(|w| !is_particle(w))
        .collect();
    words.sort_unstable();
    words.join(" ")
}

/// Distinct people: a name whose words are a subset of another's ("Müller" ⊂ "Anna Müller") is the same person.
fn merge_people(people: &BTreeSet<String>) -> usize {
    let sets: Vec<BTreeSet<&str>> = people.iter().map(|p| p.split(' ').collect()).collect();
    (0..sets.len())
        .filter(|&i| {
            !sets
                .iter()
                .enumerate()
                .any(|(j, s)| j != i && sets[i].is_subset(s) && (sets[i].len() < s.len() || j < i))
        })
        .count()
}

/// Overlapping spans merge; the earlier (then longer) span's kind wins.
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for s in spans {
        if s.start >= s.end {
            continue;
        }
        match out.last_mut() {
            Some(last) if s.start < last.end => last.end = last.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salutation_and_mentions() {
        let r = redact(
            "Sehr geehrte Frau Dr. Müller,\nwir berichten über Lena Schmidt, geb. 03.04.2017. Lena hat Anfälle.",
        );
        assert!(!r.text.contains("Müller"), "{}", r.text);
        assert!(!r.text.contains("Lena"), "{}", r.text);
        assert!(!r.text.contains("03.04.2017"), "{}", r.text);
        assert!(r.text.contains("Anfälle"));
        assert_eq!(r.counts.get(&Kind::DateOfBirth), Some(&1));
    }

    #[test]
    fn keeps_clinical_terms() {
        let t = "Heterozygous variant in STXBP1 c.1162C>T p.(Arg388*) detected. EEG on 12.05.2021 showed burst suppression.";
        let r = redact(t);
        assert_eq!(r.text, t);
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn not_names_after_salutation() {
        let r = redact("Sehr geehrte Damen und Herren,\nDear colleague,\nDear Sir or Madam,");
        assert_eq!(r.counts.get(&Kind::Name), None, "{}", r.text);
    }
}
