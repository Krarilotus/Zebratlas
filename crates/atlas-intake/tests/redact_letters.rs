//! Redaction on synthetic letters (de, en, fr). Every person, number and address below is invented;
//! no real patient data. Each letter lists the identifiers that must disappear and the clinical
//! words that must survive.

use atlas_intake::redact::{Kind, redact};

struct Letter {
    text: &'static str,
    gone: &'static [&'static str],
    kept: &'static [&'static str],
    counts: &'static [(Kind, usize)],
}

const DE: Letter = Letter {
    text: "Universitätsklinikum Musterstadt · Klinik für Neuropädiatrie
Beispielweg 12, 07740 Musterstadt
Tel.: 03641 123456 · Fax: 03641 123457 · neuro.paed@klinikum-muster.example

Sehr geehrte Frau Dr. Wagner,

wir berichten über Ihre Patientin Lena Schmidt, geb. 03.04.2017,
wohnhaft Lindenstraße 5, 99423 Weimar.
Patienten-Nr.: 4711-2023 · Versichertennummer: A123456789 · Fallnummer 20230815

Diagnose: Entwicklungs- und epileptische Enzephalopathie (DEE), STXBP1-assoziiert.
Genetik: heterozygote Variante STXBP1 c.1162C>T, p.(Arg388*), de novo.
Befund: Krampfanfälle seit dem 3. Lebensmonat, muskuläre Hypotonie, Entwicklungsverzögerung.
Kein Hinweis auf Mikrozephalie. EEG vom 12.05.2021: Burst-Suppression.
Lena besucht eine Fördereinrichtung.

Bankverbindung für Rückfragen: DE89 3704 0044 0532 0130 00

Mit freundlichen Grüßen

Prof. Dr. med. Anna Becker
Oberärztin
",
    gone: &[
        "Wagner",
        "Lena",
        "Schmidt",
        "03.04.2017",
        "Lindenstraße 5",
        "99423 Weimar",
        "Beispielweg 12",
        "07740 Musterstadt",
        "03641 123456",
        "03641 123457",
        "neuro.paed@klinikum-muster.example",
        "4711-2023",
        "A123456789",
        "20230815",
        "DE89 3704",
        "Anna",
        "Becker",
    ],
    kept: &[
        "STXBP1 c.1162C>T",
        "p.(Arg388*)",
        "Krampfanfälle",
        "muskuläre Hypotonie",
        "Entwicklungsverzögerung",
        "Mikrozephalie",
        "EEG vom 12.05.2021",
        "Burst-Suppression",
        "Enzephalopathie",
    ],
    counts: &[
        (Kind::Name, 3),
        (Kind::DateOfBirth, 1),
        (Kind::Phone, 2),
        (Kind::Email, 1),
        (Kind::RecordNumber, 2),
        (Kind::InsuranceNumber, 1),
        (Kind::Iban, 1),
        (Kind::Address, 2),
    ],
};

const EN: Letter = Letter {
    text: "Department of Clinical Genetics, Example Children's Hospital
24 Harbour View Road, Exampleton EX1 2AB
Telephone: +44 20 7946 0123   Email: genetics@example-hospital.example

Dear Dr. Okafor,

Re: Oliver James Smith, DOB 14/02/2016
Hospital number: K882193   NHS number: 943 476 5919

Thank you for referring Oliver, who was seen with his mother, Mrs. Clara Smith.
Exome sequencing identified a heterozygous pathogenic variant in SCN1A (NM_001165963.4:c.2589+3A>T).
Clinical features: febrile seizures from 6 months, status epilepticus, ataxia and intellectual disability.
No hearing loss. Findings are consistent with Dravet syndrome.

Yours sincerely,

Priya Natarajan, MD
Consultant Clinical Geneticist
",
    gone: &[
        "Okafor",
        "Oliver",
        "James",
        "Smith",
        "14/02/2016",
        "K882193",
        "943 476 5919",
        "+44 20 7946 0123",
        "genetics@example-hospital.example",
        "24 Harbour View Road",
        "EX1 2AB",
        "Clara",
        "Priya",
        "Natarajan",
    ],
    kept: &[
        "SCN1A",
        "c.2589+3A>T",
        "febrile seizures",
        "status epilepticus",
        "ataxia",
        "intellectual disability",
        "hearing loss",
        "Dravet syndrome",
    ],
    counts: &[
        (Kind::DateOfBirth, 1),
        (Kind::Phone, 1),
        (Kind::Email, 1),
        (Kind::RecordNumber, 1),
        (Kind::InsuranceNumber, 1),
        (Kind::Address, 1),
    ],
};

const FR: Letter = Letter {
    text: "Centre de référence des épilepsies rares, Hôpital Exemple
12 rue des Lilas, 75015 Paris
Tél. : 01 42 34 56 78 — secretariat.neuro@hopital-exemple.example

Cher Confrère,

Je vous adresse Mme Camille DUBOIS, née le 21 mars 2014,
N° de dossier : 2019-ABC-4471, N° de sécurité sociale : 2 14 03 75 115 042 17.

Patiente suivie pour une encéphalopathie développementale et épileptique liée à CDKL5.
Variant : CDKL5 c.2152G>A. Spasmes infantiles, hypotonie axiale, troubles visuels d'origine centrale.
Absence de microcéphalie.

Bien confraternellement,

Dr Julien Moreau
",
    gone: &[
        "Camille",
        "DUBOIS",
        "21 mars 2014",
        "2019-ABC-4471",
        "2 14 03 75 115 042 17",
        "01 42 34 56 78",
        "secretariat.neuro@hopital-exemple.example",
        "12 rue des Lilas",
        "75015 Paris",
        "Julien",
        "Moreau",
    ],
    kept: &[
        "CDKL5 c.2152G>A",
        "Spasmes infantiles",
        "hypotonie axiale",
        "encéphalopathie développementale",
        "microcéphalie",
        "troubles visuels",
    ],
    counts: &[
        (Kind::Name, 2),
        (Kind::DateOfBirth, 1),
        (Kind::Phone, 1),
        (Kind::Email, 1),
        (Kind::RecordNumber, 1),
        (Kind::InsuranceNumber, 1),
    ],
};

fn check(name: &str, l: &Letter) {
    let r = redact(l.text);
    for g in l.gone {
        assert!(!r.text.contains(g), "{name}: {g:?} survived:\n{}", r.text);
    }
    for k in l.kept {
        assert!(
            r.text.contains(k),
            "{name}: clinical text {k:?} was removed:\n{}",
            r.text
        );
    }
    for (kind, n) in l.counts {
        assert_eq!(
            r.counts.get(kind).copied().unwrap_or(0),
            *n,
            "{name}: count of {kind:?}: {:?}\n{}",
            r.counts,
            r.text
        );
    }
}

#[test]
fn german_letter() {
    check("de", &DE);
}

#[test]
fn english_letter() {
    check("en", &EN);
}

#[test]
fn french_letter() {
    check("fr", &FR);
}
