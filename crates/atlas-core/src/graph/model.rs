//! Connected-layer node and edge types: studies, grants, papers, people, organisations, and the
//! edges that tie them to conditions and genes. Every node and edge points at the upstream records
//! it derives from ([`SourceRecord`]: cache file + locator + fetched_at + sha256 of the record).

use serde::{Deserialize, Serialize};

use crate::node::EdgeKind;
use crate::provenance::{ActivityIdx, EntityIdx, Locator};

/// SHA-256 digest.
pub type Sha256 = [u8; 32];

/// Index into [`super::GraphData::records`].
pub type RecIdx = u32;

pub fn hex(digest: &Sha256) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// How a record's sha256 was computed; verification must use the same form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordHash {
    /// Bytes of one JSON-lines line, without the newline (as the fetcher wrote it).
    JsonLine,
    /// Canonical JSON of one `records[i]` entry: sorted keys, `,`/`:` separators, UTF-8
    /// (the form SOURCES.md uses for the file-level header checksum).
    CanonicalJson,
    /// One tab-separated line of a raw file, without the line break.
    TsvLine,
}

/// `prov:wasDerivedFrom` target: one upstream record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceRecord {
    /// Cache or raw file (`prov:Entity`) in the graph's provenance registry.
    pub entity: EntityIdx,
    /// `Line(n)` (1-based) for JSON-lines/TSV, `Record("records[i]")` for JSON envelopes.
    pub locator: Locator,
    /// Upstream record id (`NCT…`, `PMID:…`, core project number, `person:…`).
    pub id: String,
    /// Human-readable upstream page, when the source has one.
    pub url: Option<String>,
    pub fetched_at: Option<String>,
    pub hash: RecordHash,
    pub sha256: Sha256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyKind {
    Trial,
    Registry,
    NaturalHistory,
    Observational,
    ExpandedAccess,
}

impl StudyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trial => "trial",
            Self::Registry => "registry",
            Self::NaturalHistory => "natural_history",
            Self::Observational => "observational",
            Self::ExpandedAccess => "expanded_access",
        }
    }

    /// One plain line per kind (J1.4).
    pub fn explain(self) -> &'static str {
        match self {
            Self::Trial => "A clinical trial tests a treatment in people.",
            Self::Registry => "A registry collects information from many patients over time; joining helps research.",
            Self::NaturalHistory => "A natural-history study follows how the condition develops, without a treatment.",
            Self::Observational => "An observational study watches and measures; nobody is given a new treatment.",
            Self::ExpandedAccess => "Expanded access offers an unapproved treatment outside a trial.",
        }
    }
}

/// CT.gov statuses that a family could still act on.
pub const OPEN_STATUSES: [&str; 4] = [
    "RECRUITING",
    "NOT_YET_RECRUITING",
    "ENROLLING_BY_INVITATION",
    "ACTIVE_NOT_RECRUITING",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Study {
    /// NCT id.
    pub id: String,
    pub title: String,
    pub status: String,
    pub kind: StudyKind,
    pub phases: Vec<String>,
    pub sponsor: String,
    pub sponsor_class: String,
    pub start: String,
    pub completion: String,
    pub enrollment: Option<u64>,
    pub countries: Vec<String>,
    pub interventions: Vec<String>,
    pub record: RecIdx,
}

impl Study {
    pub fn is_open(&self) -> bool {
        OPEN_STATUSES.contains(&self.status.as_str())
    }

    pub fn is_recruiting(&self) -> bool {
        matches!(
            self.status.as_str(),
            "RECRUITING" | "NOT_YET_RECRUITING" | "ENROLLING_BY_INVITATION"
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    /// `REPORTER:<core project number>`.
    pub id: String,
    pub title: String,
    pub activity_code: String,
    /// Administering NIH institute code (`NINDS`).
    pub agency: String,
    pub organisation: String,
    pub country: String,
    pub fiscal_years: Vec<u16>,
    pub award_total: Option<u64>,
    pub start: String,
    pub end: String,
    pub url: String,
    /// One record per gene file that lists the grant.
    pub records: Vec<RecIdx>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    /// `PMID:<n>`.
    pub id: String,
    pub title: String,
    pub journal: String,
    pub year: Option<u16>,
    pub doi: Option<String>,
    pub review: bool,
    pub records: Vec<RecIdx>,
}

/// Why two person records are thought to be one person (from people/overlap.json `merge_basis`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergeEvidence {
    pub records: Vec<String>,
    pub names: Vec<String>,
    /// `orcid` / `name+affiliation` / `full name+affiliation`.
    pub basis: String,
    /// ORCID or shared affiliation words.
    pub detail: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonSource {
    /// Resolved across papers and grants by the people-overlap step.
    Resolved,
    /// A PubMed author with an ORCID, not in the overlap file.
    Orcid,
    /// An NIH RePORTER principal investigator profile.
    ReporterPi,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub name_variants: Vec<String>,
    pub orcids: Vec<String>,
    /// Affiliation strings with e-mail addresses removed (D13: no scraped personal contacts).
    pub affiliations: Vec<String>,
    pub source: PersonSource,
    pub genes: Vec<String>,
    pub communities: Vec<String>,
    pub cross_community: bool,
    /// Identity bases used (`orcid`, `name+affiliation`, ...).
    pub matched_by: Vec<String>,
    pub merge_basis: Vec<MergeEvidence>,
    pub records: Vec<RecIdx>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgKind {
    PatientGroup,
    ExpertCentre,
    Sponsor,
    Institution,
    Other,
}

impl OrgKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PatientGroup => "patient_group",
            Self::ExpertCentre => "expert_centre",
            Self::Sponsor => "sponsor",
            Self::Institution => "institution",
            Self::Other => "organisation",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Organisation {
    pub id: String,
    pub name: String,
    pub kind: OrgKind,
    pub url: Option<String>,
    /// Official contact form or contact page.
    pub contact_url: Option<String>,
    pub country: Option<String>,
    /// How the country was established (`site address`, `directory`, `domain`, `serp` = weakest).
    pub country_basis: Option<String>,
    pub description: Option<String>,
    pub languages: Vec<String>,
    /// Date the curator checked the official channel (`verified_on`), when given.
    pub verified_on: Option<String>,
    /// Official channels as the organisation publishes them (website, contact form, org mailbox, social).
    pub channels: Vec<Channel>,
    pub records: Vec<RecIdx>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    /// `website`, `contact_form`, `email`, `facebook`, ...
    pub kind: String,
    pub value: String,
    pub evidence_url: Option<String>,
}

/// A central contact as the sponsor publishes it on ClinicalTrials.gov.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub role: String,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Official {
    pub name: String,
    pub affiliation: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub facility: String,
    pub city: String,
    pub country: String,
    pub status: Option<String>,
}

/// Official contact fields of one study (`contacts.ctgov` v1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyContacts {
    /// NCT id.
    pub study: String,
    pub central: Vec<Contact>,
    pub officials: Vec<Official>,
    pub sites: Vec<Site>,
    pub last_update: String,
    pub record: RecIdx,
}

/// A Wikidata item whose xrefs resolve to atlas conditions or genes (search only, never merged).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiItem {
    /// `wikidata:Q…`.
    pub qid: String,
    /// Atlas condition / gene ids reached through the item's MONDO, ORPHA, OMIM or symbol xrefs.
    pub targets: Vec<String>,
    pub record: RecIdx,
}

/// One Wikidata label or alias in one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiName {
    pub text: Box<str>,
    pub lang: Box<str>,
    /// Aliases can be broader or narrower than the item: candidates, never certain.
    pub alias: bool,
    pub item: u32,
}

/// HGNC symbol, aliases and previous symbols of an atlas gene (search only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneAlias {
    pub hgnc: String,
    pub symbol: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub previous: Vec<String>,
    pub record: RecIdx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// study → condition.
    StudiesCondition,
    /// study → gene (symbol in a condition, title or keyword).
    NamesGene,
    /// paper / grant → gene.
    AboutGene,
    /// paper / grant → condition (name in title or abstract).
    AboutCondition,
    /// person → paper.
    AuthorOf,
    /// person → grant.
    PrincipalInvestigatorOf,
    /// study → organisation.
    SponsoredBy,
    /// grant → organisation.
    AwardedTo,
    /// organisation → condition.
    ServesCondition,
    /// organisation → gene.
    ServesGene,
    /// person → person (identity).
    SameAs,
    /// asset → organisation (holder, sponsor, operator, funder).
    HeldBy,
    /// model / cell line → condition or gene.
    ModelOf,
    /// programme / designation / drug → condition (studied for; never "treats", D36 §4).
    StudiedFor,
    /// programme / drug → gene (molecular target).
    Targets,
    /// registry / biobank / dataset / outcome measure → condition or gene.
    ResourceFor,
    /// funding call → condition or gene.
    Funds,
    /// paper → gene or condition: an LLM-extracted claim with a verified quote span.
    ClaimsAbout,
    /// any node → phenotype (KGX `biolink:has_phenotype`).
    HasPhenotype,
    /// gene → gene (KGX `biolink:orthologous_to`).
    OrthologousTo,
    /// gene → condition (KGX gene–disease predicates).
    GeneAssociatedWithCondition,
    /// any → any (SSSOM non-exact or label-only mapping; never a merge, D30).
    CandidateSameAs,
    /// any → any (other Biolink predicates; the predicate is kept in the edge reason).
    RelatedTo,
}

impl Relation {
    pub const ALL: [Relation; 23] = [
        Self::StudiesCondition,
        Self::NamesGene,
        Self::AboutGene,
        Self::AboutCondition,
        Self::AuthorOf,
        Self::PrincipalInvestigatorOf,
        Self::SponsoredBy,
        Self::AwardedTo,
        Self::ServesCondition,
        Self::ServesGene,
        Self::SameAs,
        Self::HeldBy,
        Self::ModelOf,
        Self::StudiedFor,
        Self::Targets,
        Self::ResourceFor,
        Self::Funds,
        Self::ClaimsAbout,
        Self::HasPhenotype,
        Self::OrthologousTo,
        Self::GeneAssociatedWithCondition,
        Self::CandidateSameAs,
        Self::RelatedTo,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StudiesCondition => "studies_condition",
            Self::NamesGene => "names_gene",
            Self::AboutGene => "about_gene",
            Self::AboutCondition => "about_condition",
            Self::AuthorOf => "author_of",
            Self::PrincipalInvestigatorOf => "principal_investigator_of",
            Self::SponsoredBy => "sponsored_by",
            Self::AwardedTo => "awarded_to",
            Self::ServesCondition => "serves_condition",
            Self::ServesGene => "serves_gene",
            Self::SameAs => "same_as",
            Self::HeldBy => "held_by",
            Self::ModelOf => "model_of",
            Self::StudiedFor => "studied_for",
            Self::Targets => "targets",
            Self::ResourceFor => "resource_for",
            Self::Funds => "funds",
            Self::ClaimsAbout => "claims_about",
            Self::HasPhenotype => "has_phenotype",
            Self::OrthologousTo => "orthologous_to",
            Self::GeneAssociatedWithCondition => "gene_associated_with_condition",
            Self::CandidateSameAs => "candidate_same_as",
            Self::RelatedTo => "related_to",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// How a link to a condition or gene was made (SOURCES.md levels, plus text and curated links).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkLevel {
    /// Record names the condition (name or exact synonym, normalised).
    Exact,
    /// Record's MeSH id is the condition's MONDO-equivalent MeSH id.
    Mesh,
    /// Record names a gene of the condition.
    Gene,
    /// Record links to a MONDO parent or grandparent.
    Umbrella,
    /// Gene or condition named in title/abstract (PubMed `[tiab]`, RePORTER text search).
    Text,
    /// Asserted by a curated source (organisation list, identity file).
    Curated,
    /// A curated link to a broader or neighbouring group (organisation cover marked `related`).
    Related,
}

impl LinkLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Mesh => "mesh",
            Self::Gene => "gene",
            Self::Umbrella => "umbrella",
            Self::Text => "text",
            Self::Curated => "curated",
            Self::Related => "related",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub relation: Relation,
    pub to: String,
    pub kind: EdgeKind,
    pub level: LinkLevel,
    /// Structured reason as one short phrase (`condition "STXBP1 encephalopathy"`).
    pub reason: String,
    /// `prov:wasGeneratedBy`.
    pub activity: ActivityIdx,
    /// `prov:wasDerivedFrom`.
    pub records: Vec<RecIdx>,
}

impl GraphEdge {
    /// `from|relation|to` (same scheme as [`crate::node::edge_id`]).
    pub fn id(&self) -> String {
        crate::node::edge_id(&self.from, self.relation.as_str(), &self.to)
    }
}

/// What one cache source contributed, for coverage blocks and the integrity report.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    /// `ctgov`, `reporter`, `pubmed`, `people`, `orgs`, `hgnc`.
    pub source: String,
    pub label: String,
    /// `loaded` / `absent` / `rejected: <why>`.
    pub status: String,
    pub files: Vec<String>,
    pub retrieved_at: Option<String>,
    /// What was searched (`all studies`, `genes STXBP1, …`).
    pub scope: String,
    /// Gene symbols the source was fetched for (empty = not gene-scoped).
    pub genes: Vec<String>,
    pub records: u64,
    /// File-level header checksums recomputed at build time and equal to the header.
    pub header_checksums_verified: u64,
    pub header_checksums_failed: u64,
    /// Nodes and edges this source added (research caches, KGX).
    #[serde(default)]
    pub nodes: u64,
    #[serde(default)]
    pub edges: u64,
    /// Records the source marks excluded or the reader kept out of scope (counted, never dropped silently).
    #[serde(default)]
    pub excluded: u64,
    #[serde(default)]
    pub licence_class: Option<super::LicenceClass>,
}
