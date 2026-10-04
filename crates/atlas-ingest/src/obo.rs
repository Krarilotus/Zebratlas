//! Minimal OBO 1.4 reader for HPO and MONDO (only the tags we use).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use atlas_core::{Scope, Synonym, Term, Xref};
use indexmap::IndexMap;

use crate::error::IngestError;

/// Header tags and `[Term]` stanzas in file order (a repeated id replaces the term in place).
#[derive(Debug, Default)]
pub struct Obo {
    pub header: Vec<(String, String)>,
    pub terms: Vec<Term>,
}

impl Obo {
    /// `data-version` header tag.
    pub fn data_version(&self) -> Option<&str> {
        self.header
            .iter()
            .find(|(k, _)| k == "data-version")
            .map(|(_, v)| v.as_str())
    }
}

pub fn read_obo(path: &Path) -> Result<Obo, IngestError> {
    let file = File::open(path).map_err(IngestError::io(path))?;
    parse_obo(BufReader::with_capacity(1 << 20, file)).map_err(IngestError::io(path))
}

pub fn parse_obo(reader: impl BufRead) -> std::io::Result<Obo> {
    let mut header = Vec::new();
    let mut terms: IndexMap<String, Term> = IndexMap::new();
    let mut term: Option<Term> = None;
    let mut in_term = false;
    let mut in_header = true;
    let mut flush = |term: &mut Option<Term>| {
        if let Some(t) = term.take() {
            terms.insert(t.id.clone(), t);
        }
    };
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('[') {
            flush(&mut term);
            in_term = line == "[Term]";
            in_header = false;
            continue;
        }
        if in_header {
            if let Some((tag, value)) = line.split_once(": ") {
                header.push((tag.to_owned(), value.to_owned()));
            }
            continue;
        }
        if !in_term || line.is_empty() || !line.contains(':') {
            continue;
        }
        let (tag, value) = line.split_once(": ").unwrap_or((&line, ""));
        if tag == "id" {
            flush(&mut term);
            term = Some(Term::new(value));
            continue;
        }
        let Some(t) = term.as_mut() else { continue };
        match tag {
            "name" => t.name = value.to_owned(),
            "def" => t.definition = unquote(value).0,
            "synonym" => {
                let (text, rest) = unquote(value);
                let mut tokens = rest.splitn(3, ' ');
                let scope = tokens.next().filter(|s| !s.is_empty()).unwrap_or("RELATED");
                let kind = tokens.next().filter(|k| !k.starts_with('[')).map(str::to_owned);
                t.synonyms.push(Synonym {
                    text,
                    scope: Scope::parse(scope),
                    kind,
                });
            }
            "is_a" => t.parents.push(first_word(value).to_owned()),
            "xref" => {
                let (id, rest) = value.split_once(' ').unwrap_or((value, ""));
                let sources = provenance_sources(rest);
                match t.xrefs.iter_mut().find(|x| x.id == id) {
                    Some(x) => x.sources = sources,
                    None => t.xrefs.push(Xref {
                        id: id.to_owned(),
                        sources,
                    }),
                }
            }
            "alt_id" => t.alt_ids.push(value.to_owned()),
            "subset" => t.subsets.push(first_word(value).to_owned()),
            "is_obsolete" => t.obsolete = value == "true",
            "replaced_by" => t.replaced_by = Some(value.to_owned()),
            _ => {}
        }
    }
    flush(&mut term);
    Ok(Obo {
        header,
        terms: terms.into_values().collect(),
    })
}

fn first_word(value: &str) -> &str {
    value.split(' ').next().unwrap_or(value)
}

/// `"text with \" escapes" rest` -> (text, rest); unquoted input -> (input, "").
fn unquote(value: &str) -> (String, String) {
    let Some(body) = value.strip_prefix('"') else {
        return (value.to_owned(), String::new());
    };
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    break;
                }
            }
            '"' => {
                let text = body[..i].replace("\\\"", "\"");
                let rest = body[i + 1..].trim_start();
                return (text, rest.to_owned());
            }
            _ => {}
        }
    }
    (value.to_owned(), String::new())
}

/// Values of `source="..."` in an xref's trailing qualifiers.
fn provenance_sources(rest: &str) -> Vec<String> {
    const KEY: &str = "source=\"";
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = rest[pos..].find(KEY) {
        let start = pos + i + KEY.len();
        match rest[start..].find('"') {
            Some(0) => pos = start,
            Some(end) => {
                out.push(rest[start..start + end].to_owned());
                pos = start + end + 1;
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBO: &str = r#"format-version: 1.2
data-version: hp/releases/2026-09-01

[Term]
id: HP:0001250
name: Seizure
def: "A \"sudden\" event." [HPO:probinson]
alt_id: HP:0002279
synonym: "Seizures" EXACT []
synonym: "Fits" EXACT layperson [ORCID:0000-0001-0000-0000]
synonym: "bare"
xref: UMLS:C0036572 {source="MONDO:equivalentTo", source="other"}
is_a: HP:0000001 ! All

[Typedef]
id: part_of
name: part of
"#;

    #[test]
    fn parses_terms_and_header() {
        let obo = parse_obo(OBO.as_bytes()).unwrap();
        assert_eq!(obo.data_version(), Some("hp/releases/2026-09-01"));
        assert_eq!(obo.terms.len(), 1);
        let t = &obo.terms[0];
        assert_eq!(t.definition, "A \"sudden\" event.");
        assert_eq!(t.synonyms[1].kind.as_deref(), Some("layperson"));
        assert_eq!(t.synonyms[0].kind, None);
        assert_eq!(t.synonyms[2].scope, Scope::Related);
        assert_eq!(t.xrefs[0].sources, ["MONDO:equivalentTo", "other"]);
        assert_eq!(t.parents, ["HP:0000001"]);
    }
}
