//! Provenance in PROV-O terms (D10): source files are `prov:Entity`, every transformation is a
//! `prov:Activity` that `prov:used` entities and `prov:wasAssociatedWith` an agent; nodes and
//! evidence records point back with `prov:wasGeneratedBy` / `prov:wasDerivedFrom`.
//!
//! Records are never dropped silently: an activity counts what it read, kept and skipped (with why).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable activity ids. One activity per transformation step of a build.
pub mod activity {
    pub const INGEST_HPO: &str = "activity:ingest-hpo";
    pub const INGEST_MONDO: &str = "activity:ingest-mondo";
    pub const INGEST_ORPHANET_DISORDERS: &str = "activity:ingest-orphanet-disorders";
    pub const INGEST_HPOA: &str = "activity:ingest-hpoa";
    pub const INGEST_GENES_TO_DISEASE: &str = "activity:ingest-genes-to-disease";
    pub const INGEST_ORPHANET_GENES: &str = "activity:ingest-orphanet-genes";
    pub const INGEST_ORPHANET_PREVALENCE: &str = "activity:ingest-orphanet-prevalence";
    pub const INGEST_ORPHANET_NATURAL_HISTORY: &str = "activity:ingest-orphanet-natural-history";
    /// Merge on `MONDO:equivalentTo` xrefs.
    pub const IDENTITY_MONDO_EXACT: &str = "activity:identity-mondo-exact";
    /// Merge on validated Orphanet `E` mappings.
    pub const IDENTITY_ORPHANET_EXACT: &str = "activity:identity-orphanet-exact";
    /// Guarded nomination on equal names / exact synonyms: `candidate_same_as` links, never merges (D30.1).
    pub const IDENTITY_LABEL: &str = "activity:identity-label";
    /// Orphanet `OBSOLETE:` / `MOVED TO` -> retired; `NON RARE IN EUROPE:` -> not rare.
    pub const CLASSIFY_ORPHANET_STATUS: &str = "activity:classify-orphanet-status";
    /// Gene2Phenotype conditions without an OMIM/Orphanet/MONDO node (newly described).
    pub const INGEST_G2P_CONDITIONS: &str = "activity:ingest-g2p-conditions";
    /// IC(t) = -ln p(t) over active diseases with phenotype annotations.
    pub const COMPUTE_IC: &str = "activity:compute-ic";
}

/// Index of a [`SourceEntity`] in [`Provenance::entities`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityIdx(pub u16);

/// Index of an [`Activity`] in [`Provenance::activities`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActivityIdx(pub u16);

/// Where in a source a record sits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locator {
    /// 1-based physical line (tabular files).
    Line(u32),
    /// Record path, e.g. `Disorder[OrphaCode=558]/Prevalence[id=123]` or a term id.
    Record(String),
}

impl fmt::Display for Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line(n) => write!(f, "L{n}"),
            Self::Record(r) => f.write_str(r),
        }
    }
}

/// Pointer from an evidence record to its source entity (`prov:wasDerivedFrom`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordRef {
    pub entity: EntityIdx,
    pub locator: Locator,
}

impl RecordRef {
    pub fn line(entity: EntityIdx, line: u32) -> Self {
        Self {
            entity,
            locator: Locator::Line(line),
        }
    }

    pub fn record(entity: EntityIdx, record: impl Into<String>) -> Self {
        Self {
            entity,
            locator: Locator::Record(record.into()),
        }
    }
}

/// `prov:Entity`: one source file as read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceEntity {
    /// `source:<file>`.
    pub id: String,
    pub url: String,
    pub file: String,
    /// From the file header where available (OBO `data-version`, HPOA `#version`, JDBOR date).
    pub version: Option<String>,
    /// File modification time, RFC 3339.
    pub retrieved_at: Option<String>,
    pub sha256: Option<String>,
    pub bytes: u64,
    pub licence: Option<String>,
}

/// `prov:SoftwareAgent`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub version: String,
    /// `git describe --always --dirty` of the working tree that ran the build.
    pub commit: Option<String>,
}

/// `prov:Activity`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    /// One of [`activity`].
    pub id: String,
    pub label: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    /// `prov:used`.
    pub used: Vec<EntityIdx>,
    pub parameters: BTreeMap<String, String>,
    /// `prov:wasAssociatedWith`.
    pub agent: Agent,
    /// What happened to the input: `read`, `kept`, `skipped:<reason>`, `merged`, ...
    pub counts: BTreeMap<String, u64>,
}

impl Activity {
    /// Add `n` to counter `key`.
    pub fn count(&mut self, key: &str, n: u64) {
        *self.counts.entry(key.to_owned()).or_default() += n;
    }
}

/// Registry of entities and activities of one snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub entities: Vec<SourceEntity>,
    pub activities: Vec<Activity>,
}

impl Provenance {
    pub fn add_entity(&mut self, entity: SourceEntity) -> EntityIdx {
        self.entities.push(entity);
        EntityIdx(u16::try_from(self.entities.len() - 1).expect("too many source entities"))
    }

    pub fn add_activity(&mut self, activity: Activity) -> ActivityIdx {
        self.activities.push(activity);
        ActivityIdx(u16::try_from(self.activities.len() - 1).expect("too many activities"))
    }

    pub fn entity(&self, idx: EntityIdx) -> &SourceEntity {
        &self.entities[usize::from(idx.0)]
    }

    pub fn activity(&self, idx: ActivityIdx) -> &Activity {
        &self.activities[usize::from(idx.0)]
    }

    pub fn activity_mut(&mut self, idx: ActivityIdx) -> &mut Activity {
        &mut self.activities[usize::from(idx.0)]
    }

    pub fn activity_by_id(&self, id: &str) -> Option<&Activity> {
        self.activities.iter().find(|a| a.id == id)
    }

    pub fn entity_by_file(&self, file: &str) -> Option<EntityIdx> {
        let i = self.entities.iter().position(|e| e.file == file)?;
        Some(EntityIdx(u16::try_from(i).ok()?))
    }

    /// The activity that ingested `entity` (the first that used it), for `prov:wasGeneratedBy`.
    pub fn generator_of(&self, entity: EntityIdx) -> Option<&Activity> {
        self.activities
            .iter()
            .find(|a| a.id.starts_with("activity:ingest-") && a.used.contains(&entity))
    }

    /// `file#L12` / `file#Disorder[OrphaCode=1]`.
    pub fn cite(&self, record: &RecordRef) -> String {
        format!("{}#{}", self.entity(record.entity).file, record.locator)
    }
}

/// Parameters as a sorted map (for activities built elsewhere).
pub fn params<I, K, V>(items: I) -> std::collections::BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: ToString,
{
    items.into_iter().map(|(k, v)| (k.into(), v.to_string())).collect()
}

/// RFC 3339 time with milliseconds (UTC), as every retrieval and activity time is written.
pub fn rfc3339(t: std::time::SystemTime) -> String {
    humantime::format_rfc3339_millis(t).to_string()
}
