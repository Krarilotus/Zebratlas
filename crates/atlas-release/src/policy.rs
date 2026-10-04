//! D35 is stricter than some upstream licences: only independently verified CC0/CC BY
//! inputs carry fields into this release. A file's self-reported licence never grants access.
use atlas_core::provenance::SourceEntity;
use serde::Serialize;

pub const CC_BY: &str = "https://creativecommons.org/licenses/by/4.0/";
pub const CC0: &str = "https://creativecommons.org/publicdomain/zero/1.0/";
pub const CHECKED: &str = "2026-10-04";

#[derive(Clone, Debug, Serialize)]
pub struct Policy {
    pub source: &'static str,
    pub license: &'static str,
    pub terms_url: &'static str,
    pub checked_on: &'static str,
    pub copy_fields: bool,
    pub attribution: &'static str,
}

pub fn for_entity(e: &SourceEntity) -> Policy {
    // Exact file ownership, not substring matching of URLs or trusting cache metadata.
    let f = e.file.replace('\\', "/");
    let (source, license, terms_url, copy_fields, attribution) = match f.as_str() {
        "mondo.obo" => (
            "mondo",
            CC_BY,
            "https://mondo.monarchinitiative.org/pages/download/",
            true,
            "Mondo Disease Ontology, Monarch Initiative. CC BY 4.0. This release is an identifier-based derived projection; see source versions and changes in provenance.",
        ),
        "en_product1.xml" | "en_product6.xml" | "en_product9_prev.xml" | "en_product9_ages.xml" => (
            "orphadata",
            CC_BY,
            "https://sciences.orphadata.com/faq/",
            true,
            "Orphadata Science: Free access data from Orphanet. © INSERM 1999. https://sciences.orphadata.com/. Data version: see source manifest. Derived projection. Orphanet/Orphadata data was used to develop this service; outputs may not faithfully reflect the source, and Orphanet is not responsible for their quality. No endorsement.",
        ),
        "hgnc_complete_set.txt" | "raw/hgnc_complete_set.txt" => (
            "hgnc",
            CC0,
            "https://www.genenames.org/about/license/",
            true,
            "HGNC (RRID:SCR_002827), Genenames.org. CC0. Versions and source links are retained.",
        ),
        "go-basic.obo" => (
            "go",
            CC_BY,
            "https://geneontology.org/docs/go-citation-policy/",
            true,
            "Gene Ontology Consortium. CC BY 4.0. See source release/version; identifiers are a derived projection. No warranties or endorsement.",
        ),
        "ReactomePathways.txt"
        | "ReactomePathwaysRelation.txt"
        | "NCBI2Reactome.txt"
        | "NCBI2Reactome_All_Levels.txt" => (
            "reactome",
            CC0,
            "https://reactome.org/license",
            true,
            "Reactome pathway database data, CC0. See source versions.",
        ),
        "hp.obo" | "phenotype.hpoa" | "genes_to_disease.txt" | "genes_to_phenotype.txt" => (
            "hpo",
            "HPO custom integrity terms",
            "https://human-phenotype-ontology.github.io/license.html",
            false,
            "This service uses the Human Phenotype Ontology Consortium resources. Versions are retained. This release includes identifiers and links only; no HPO vocabulary or annotation content is copied.",
        ),
        "allG2P_2026-09-28.csv.gz" => (
            "g2p",
            "EMBL-EBI terms; record permissions unverified",
            "https://www.ebi.ac.uk/about/terms-of-use/",
            false,
            "Gene2Phenotype, EMBL-EBI. Identifiers and links only; open availability is not a licence grant.",
        ),
        "ClinGen_gene_curation_list_GRCh38.tsv" => (
            "clingen",
            "ClinGen terms; unverified",
            "https://www.clinicalgenome.org/docs/terms-of-use/",
            false,
            "ClinGen Clinical Genome Resource. Identifiers and links only.",
        ),
        _ if f.starts_with("cache/labels/") => (
            "wikidata",
            CC0,
            "https://www.wikidata.org/wiki/Wikidata:Licensing",
            true,
            "Wikidata structured data, CC0. Revision URLs are retained in the upstream cache; this release uses identifiers only.",
        ),
        _ if f.starts_with("cache/trials/") || f == "cache/contacts/ctgov.json" => (
            "ctgov",
            "ClinicalTrials.gov terms; redistribution scope unverified",
            "https://clinicaltrials.gov/about-site/terms-conditions",
            false,
            "ClinicalTrials.gov, US National Library of Medicine. Identifiers and links only; registry metadata rights are not inferred from US government hosting.",
        ),
        _ if f.starts_with("cache/pubmed/") => (
            "pubmed",
            "NCBI policies; third-party copyright",
            "https://www.ncbi.nlm.nih.gov/home/about/policies/",
            false,
            "PubMed, US National Library of Medicine. Identifiers and links only; no titles, abstracts or author fields are redistributed.",
        ),
        _ if f.starts_with("cache/reporter/") => (
            "reporter",
            "NIH RePORTER; record-level rights unverified",
            "https://reporter.nih.gov/faq",
            false,
            "NIH RePORTER. Identifiers and links only; no grant abstracts or personal fields are copied.",
        ),
        _ if f.starts_with("cache/people/") => (
            "people",
            "Derived from multiple upstream terms",
            "https://www.ncbi.nlm.nih.gov/home/about/policies/",
            false,
            "Rare Disease Atlas identity resolution. Upstream restrictions retained; identifiers and public source links only.",
        ),
        _ if f.starts_with("cache/orgs/") => (
            "orgs",
            "Individual website terms unverified",
            "https://rarediseases.org/terms-conditions/",
            false,
            "Official organisation websites. See per-record source links. Identifiers and links only; no copied page text or quotes.",
        ),
        _ => (
            "unknown",
            "UNKNOWN: link only",
            "https://creativecommons.org/share-your-work/cclicenses/",
            false,
            "Permissions unverified. Identifiers and source links only. The terms URL explains licence categories; it is not a licence grant for this source.",
        ),
    };
    let mut policy = Policy {
        source,
        license,
        terms_url,
        checked_on: CHECKED,
        copy_fields,
        attribution,
    };
    // Additive research adapters: copied text is never inferred from curation's licence.
    if source == "unknown" {
        let family = [
            ("cache/pipelines/", "pipelines"),
            ("cache/regulatory/", "regulatory"),
            ("cache/org-assets/", "org-assets"),
            ("cache/directories/", "directories"),
            ("cache/models/", "models"),
            ("cache/outcomes/", "outcomes"),
            ("cache/funders/", "funders"),
            ("cache/research_intl/", "research-intl"),
            ("cache/claims/", "claims"),
            ("cache/groups-wide/", "groups-wide"),
            ("cache/mappings/", "mapping-candidates"),
        ]
        .into_iter()
        .find(|(prefix, _)| f.starts_with(prefix));
        if let Some((_, family)) = family {
            policy.source = family;
        }
        if let Some(tail) = f.strip_prefix("cache/kgx/") {
            let kgx = tail.split('/').next().unwrap_or("");
            let (family, licence, terms, copy) = match kgx {
                "cellosaurus" => ("cellosaurus", CC_BY, "https://www.cellosaurus.org/faq", true),
                "impc" => (
                    "impc",
                    CC_BY,
                    "https://www.mousephenotype.org/about-impc/terms-of-use/",
                    true,
                ),
                "opentargets" => (
                    "opentargets",
                    CC0,
                    "https://platform-docs.opentargets.org/licence",
                    true,
                ),
                "openalex" => ("openalex", CC0, "https://help.openalex.org/access/overview/", true),
                "ror" => ("ror", CC0, "https://ror.org/", true),
                "crossref_funders" => (
                    "crossref-funders",
                    CC0,
                    "https://www.crossref.org/services/funder-registry/",
                    true,
                ),
                "monarch" => (
                    "monarch",
                    "mixed upstream terms; link only",
                    "https://monarch-app.monarchinitiative.org/Licensing/",
                    false,
                ),
                "alliance" => (
                    "alliance",
                    "export-level permission unresolved; link only",
                    "https://github.com/alliance-genome/agr_open_data/blob/main/DATA_DOCUMENTATION.md",
                    false,
                ),
                "clingen" => (
                    "clingen",
                    "unresolved artifact scope; link only",
                    "https://clinicalgenome.org/docs/terms-of-use/",
                    false,
                ),
                "gard" => (
                    "gard",
                    "unverified; link only",
                    "https://rarediseases.info.nih.gov/disclaimer",
                    false,
                ),
                "chembl" => (
                    "chembl",
                    "CC-BY-SA-3.0; link only",
                    "https://chembl.gitbook.io/chembl-interface-documentation/about",
                    false,
                ),
                "fda" => (
                    "fda",
                    "third-party scope unresolved; link only",
                    "https://www.fda.gov/about-fda/website-policies",
                    false,
                ),
                "ema" => (
                    "ema",
                    "custom terms; link only",
                    "https://www.ema.europa.eu/en/about-us/legal-notice",
                    false,
                ),
                "ctgov" => (
                    "ctgov",
                    "redistribution scope unverified; link only",
                    "https://clinicaltrials.gov/about-site/terms-conditions",
                    false,
                ),
                "bbmri" => (
                    "bbmri",
                    "unverified; link only",
                    "https://directory.bbmri-eric.eu/",
                    false,
                ),
                "ega" => (
                    "ega",
                    "controlled data; link only",
                    "https://ega-archive.org/access/request-data/how-to-request-data/",
                    false,
                ),
                _ => ("unknown", "UNKNOWN: link only", policy.terms_url, false),
            };
            policy.source = family;
            policy.license = licence;
            policy.terms_url = terms;
            // Even a reviewed provider may carry third-party rows with stricter terms.
            policy.copy_fields = copy
                && e.licence.as_deref().is_some_and(|s| {
                    atlas_core::graph::LicenceClass::classify(s) == atlas_core::graph::LicenceClass::Open
                });
            policy.attribution = "Source publisher and originating providers named in LICENCES.md; exact release, source links and changes retained in PROV-O. No endorsement. Only the reviewed artifact scope is copied.";
        }
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forged_licence_and_confusable_paths_do_not_grant_copying() {
        for file in [
            "cache/unknown/mondo.obo",
            "cache/orgs/mondo.obo",
            "cache/labels-evil/x.json",
            "new-source.json",
        ] {
            let entity = SourceEntity {
                file: file.into(),
                licence: Some(CC0.into()),
                ..Default::default()
            };
            assert!(!for_entity(&entity).copy_fields, "{file}");
        }
    }
}
