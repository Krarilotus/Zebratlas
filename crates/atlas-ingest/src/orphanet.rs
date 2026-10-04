//! Orphadata XML products (CC BY 4.0): disorders, genes, prevalence, onset/inheritance.
//!
//! Streams the file and materialises one `Disorder` element at a time as a small tree, queried
//! with ElementTree-style paths (`SynonymList/Synonym`) so the logic mirrors the Python reader.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use atlas_core::evidence::Prevalence;
use atlas_core::identity::Mapping;
use atlas_core::provenance::{EntityIdx, RecordRef};
use indexmap::IndexMap;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::IngestError;

#[derive(Debug, Default)]
struct El {
    name: String,
    id: Option<String>,
    /// Text before the first child (ElementTree `.text`).
    text: String,
    children: Vec<El>,
}

impl El {
    /// Elements at a child path, in document order.
    fn findall<'a>(&'a self, path: &str) -> std::vec::IntoIter<&'a El> {
        let mut out = Vec::new();
        self.collect(path, &mut out);
        out.into_iter()
    }

    fn collect<'a>(&'a self, path: &str, out: &mut Vec<&'a El>) {
        let (head, rest) = match path.split_once('/') {
            Some((h, r)) => (h, Some(r)),
            None => (path, None),
        };
        for c in self.children.iter().filter(|c| c.name == head) {
            match rest {
                Some(r) => c.collect(r, out),
                None => out.push(c),
            }
        }
    }

    fn find(&self, path: &str) -> Option<&El> {
        self.findall(path).next()
    }

    /// Stripped text at `path`, "" if absent.
    fn text(&self, path: &str) -> &str {
        self.find(path).map_or("", |e| e.text.trim())
    }

    fn names(&self, path: &str) -> Vec<String> {
        self.findall(path).map(|e| e.text.trim().to_owned()).collect()
    }
}

/// `date (version)` of the JDBOR root element.
pub type ProductVersion = Option<String>;

/// Call `f` for each `Disorder` that has an `OrphaCode` child; returns the product version.
fn for_each_disorder(path: &Path, mut f: impl FnMut(&El)) -> Result<ProductVersion, IngestError> {
    let xml_err = |source| IngestError::Xml {
        path: path.to_owned(),
        source,
    };
    let file = File::open(path).map_err(IngestError::io(path))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(1 << 20, file));
    let mut buf = Vec::new();
    let mut version = None;
    let mut stack: Vec<El> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).map_err(xml_err)? {
            Event::Start(e) => {
                let el = open(&e);
                if el.name == "JDBOR" {
                    version = Some(jdbor_version(&e));
                }
                if !stack.is_empty() || el.name == "Disorder" {
                    stack.push(el);
                }
            }
            Event::Empty(e) => {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(open(&e));
                }
            }
            Event::Text(t) => {
                if let Some(el) = stack.last_mut().filter(|el| el.children.is_empty()) {
                    el.text.push_str(&t.decode().map_err(|e| xml_err(e.into()))?);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(el) = stack.last_mut().filter(|el| el.children.is_empty()) {
                    let name = r.decode().map_err(|e| xml_err(e.into()))?;
                    if let Some(c) = r.resolve_char_ref().map_err(xml_err)? {
                        el.text.push(c);
                    } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(&name) {
                        el.text.push_str(s);
                    }
                }
            }
            Event::End(_) => {
                if let Some(el) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(el),
                        None if el.find("OrphaCode").is_some() => f(&el),
                        None => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(version)
}

fn open(e: &BytesStart) -> El {
    let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    El {
        name,
        id: attr(e, b"id"),
        ..El::default()
    }
}

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    let a = e.try_get_attribute(key).ok()??;
    Some(
        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .ok()?
            .into_owned(),
    )
}

fn jdbor_version(e: &BytesStart) -> String {
    let date = attr(e, b"date").unwrap_or_default();
    match attr(e, b"version") {
        Some(v) => format!("{date} ({v})"),
        None => date,
    }
}

fn orpha(el: &El) -> String {
    format!("ORPHA:{}", el.text("OrphaCode"))
}

/// `Disorder[OrphaCode=558]`.
fn locator(el: &El) -> String {
    format!("Disorder[OrphaCode={}]", el.text("OrphaCode"))
}

/// `123[PMID]_456[PMID]` -> `PMID:123`, `PMID:456`.
fn pmids(text: &str) -> Vec<String> {
    text.match_indices("[PMID]")
        .filter_map(|(end, _)| {
            let digits = text[..end].bytes().rev().take_while(u8::is_ascii_digit).count();
            (digits > 0).then(|| format!("PMID:{}", &text[end - digits..end]))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disorder {
    /// ORPHA:<code>.
    pub orpha: String,
    pub name: String,
    pub synonyms: Vec<String>,
    /// Disease, Clinical subtype, Malformation syndrome, ...
    pub disorder_type: String,
    /// Group of disorders / Disorder / Subtype of disorder.
    pub group: String,
    pub mappings: Vec<Mapping>,
    pub record: RecordRef,
}

/// Disorders by ORPHA id (a repeated code replaces the earlier entry in place).
pub fn read_disorders(
    path: &Path,
    entity: EntityIdx,
) -> Result<(IndexMap<String, Disorder>, ProductVersion), IngestError> {
    let mut out = IndexMap::new();
    let version = for_each_disorder(path, |el| {
        let mappings = el
            .findall("ExternalReferenceList/ExternalReference")
            .map(|r| Mapping {
                source: r.text("Source").to_owned(),
                reference: r.text("Reference").to_owned(),
                relation: r
                    .text("DisorderMappingRelation/Name")
                    .split(' ')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                validated: r.text("DisorderMappingValidationStatus/Name") == "Validated",
            })
            .collect();
        let d = Disorder {
            orpha: orpha(el),
            name: el.text("Name").to_owned(),
            synonyms: el.names("SynonymList/Synonym"),
            disorder_type: el.text("DisorderType/Name").to_owned(),
            group: el.text("DisorderGroup/Name").to_owned(),
            mappings,
            record: RecordRef::record(entity, locator(el)),
        };
        out.insert(d.orpha.clone(), d);
    })?;
    Ok((out, version))
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneAssociation {
    pub orpha: String,
    pub symbol: String,
    pub gene_name: String,
    pub hgnc: Option<String>,
    /// e.g. "Disease-causing germline mutation(s) in".
    pub association: String,
    /// Assessed / Not yet assessed.
    pub status: String,
    pub pmids: Vec<String>,
    pub record: RecordRef,
}

pub fn read_gene_associations(
    path: &Path,
    entity: EntityIdx,
) -> Result<(Vec<GeneAssociation>, ProductVersion), IngestError> {
    let mut out = Vec::new();
    let version = for_each_disorder(path, |el| {
        for assoc in el.findall("DisorderGeneAssociationList/DisorderGeneAssociation") {
            let Some(gene) = assoc.find("Gene") else {
                continue;
            };
            let hgnc = gene
                .findall("ExternalReferenceList/ExternalReference")
                .find(|r| r.text("Source") == "HGNC")
                .map(|r| r.text("Reference"))
                .filter(|h| !h.is_empty());
            let symbol = gene.text("Symbol");
            out.push(GeneAssociation {
                orpha: orpha(el),
                symbol: symbol.to_owned(),
                gene_name: gene.text("Name").to_owned(),
                hgnc: hgnc.map(|h| format!("HGNC:{h}")),
                association: assoc.text("DisorderGeneAssociationType/Name").to_owned(),
                status: assoc.text("DisorderGeneAssociationStatus/Name").to_owned(),
                pmids: pmids(assoc.text("SourceOfValidation")),
                record: RecordRef::record(
                    entity,
                    format!("{}/DisorderGeneAssociation[Gene={symbol}]", locator(el)),
                ),
            });
        }
    })?;
    Ok((out, version))
}

/// Prevalence items by ORPHA id; disorders without items are left out (as in the Python).
pub fn read_prevalence(
    path: &Path,
    entity: EntityIdx,
) -> Result<(IndexMap<String, Vec<Prevalence>>, ProductVersion), IngestError> {
    let mut out: IndexMap<String, Vec<Prevalence>> = IndexMap::new();
    let version = for_each_disorder(path, |el| {
        let items: Vec<Prevalence> = el
            .findall("PrevalenceList/Prevalence")
            .map(|p| {
                let mean = p.text("ValMoy").parse::<f64>().ok().filter(|v| *v > 0.0);
                let id = p.id.as_deref().unwrap_or("");
                Prevalence {
                    kind: p.text("PrevalenceType/Name").to_owned(),
                    qualification: p.text("PrevalenceQualification/Name").to_owned(),
                    prevalence_class: Some(p.text("PrevalenceClass/Name"))
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned),
                    mean_value: mean,
                    geography: p.text("PrevalenceGeographic/Name").to_owned(),
                    validated: p.text("PrevalenceValidationStatus/Name") == "Validated",
                    pmids: pmids(p.text("Source")),
                    record: RecordRef::record(entity, format!("{}/Prevalence[id={id}]", locator(el))),
                }
            })
            .collect();
        if !items.is_empty() {
            out.insert(orpha(el), items);
        }
    })?;
    Ok((out, version))
}

#[derive(Clone, Debug, PartialEq)]
pub struct NaturalHistory {
    /// Neonatal, Infancy, Childhood, Adolescent, Adult, Elderly, All ages.
    pub onset: Vec<String>,
    pub inheritance: Vec<String>,
    pub record: RecordRef,
}

pub fn read_natural_history(
    path: &Path,
    entity: EntityIdx,
) -> Result<(IndexMap<String, NaturalHistory>, ProductVersion), IngestError> {
    let mut out = IndexMap::new();
    let version = for_each_disorder(path, |el| {
        let nh = NaturalHistory {
            onset: el.names("AverageAgeOfOnsetList/AverageAgeOfOnset/Name"),
            inheritance: el.names("TypeOfInheritanceList/TypeOfInheritance/Name"),
            record: RecordRef::record(entity, locator(el)),
        };
        out.insert(orpha(el), nh);
    })?;
    Ok((out, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmid_extraction() {
        assert_eq!(pmids("123[PMID]_456[PMID]_x[PMID]"), ["PMID:123", "PMID:456"]);
        assert!(pmids("").is_empty());
    }

    #[test]
    fn reads_disorders_with_entities() {
        let dir = std::env::temp_dir().join(format!("atlas-orpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p1.xml");
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?><JDBOR date="2026-06-23" version="1.3"><DisorderList>
<Disorder id="1"><OrphaCode>558</OrphaCode><Name lang="en">Marfan &amp; co</Name>
<SynonymList><Synonym>MFS</Synonym></SynonymList>
<ExternalReferenceList><ExternalReference id="9"><Source>OMIM</Source><Reference>154700</Reference>
<DisorderMappingRelation><Name>E (Exact mapping)</Name></DisorderMappingRelation>
<DisorderMappingValidationStatus><Name>Validated</Name></DisorderMappingValidationStatus><Empty/>
</ExternalReference></ExternalReferenceList></Disorder></DisorderList></JDBOR>"#,
        )
        .unwrap();
        let (map, version) = read_disorders(&path, EntityIdx(0)).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(version.as_deref(), Some("2026-06-23 (1.3)"));
        let d = &map["ORPHA:558"];
        assert_eq!(d.name, "Marfan & co");
        assert_eq!(d.synonyms, ["MFS"]);
        assert_eq!((d.mappings[0].relation.as_str(), d.mappings[0].validated), ("E", true));
    }
}
