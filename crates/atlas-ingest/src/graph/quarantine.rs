//! Mark quarantined records using the shared withholding owner; no list parser lives here.
use super::builder::Builder;
use atlas_core::graph::Quarantine;
use atlas_core::withhold::{QUARANTINE_FILE, Withhold};

pub const FILE: &str = QUARANTINE_FILE;

/// Records stay with provenance; public callers use `RecordWithhold` to exclude derived items.
pub fn apply(b: &mut Builder<'_>, withhold: &Withhold) -> usize {
    let additional: Vec<_> = b
        .data
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, _record)| {
            withhold.records(&b.data, &[i as u32]).map(|hit| Quarantine {
                record: i as u32,
                reason: hit.reason.to_owned(),
            })
        })
        .collect();
    for q in additional {
        mark(b, q.record, &q.reason);
    }
    b.data.quarantine.sort_by_key(|q| q.record);
    if !b.data.quarantine.is_empty() {
        let mut entities: Vec<_> = b
            .data
            .quarantine
            .iter()
            .map(|q| b.data.records[q.record as usize].entity)
            .collect();
        entities.sort_by_key(|e| e.0);
        entities.dedup();
        let act = b.start(
            "activity:withhold-source-records-v1",
            "Retain source records with public exclusion reasons",
            &entities,
        );
        b.param(
            act,
            "rule",
            "source-integrity-and-curation-v1; acquisition and suppression ledger",
        );
        let ledger = serde_json::to_string(&b.data.quarantine).expect("exclusion ledger serialises");
        b.param(act, "record_exclusions", &ledger);
        b.finish(act, &[("retained_excluded_records", b.data.quarantine.len())]);
    }
    b.data.quarantine.len()
}

/// Retain the source record and a documented exclusion. Used for failed source checks and
/// records whose own curator reports unfinished placeholder content.
pub fn mark(b: &mut Builder<'_>, record: u32, reason: &str) {
    match b.data.quarantine.binary_search_by_key(&record, |q| q.record) {
        Ok(i) => {
            let existing = &mut b.data.quarantine[i].reason;
            if !existing.split("; ").any(|r| r == reason) {
                existing.push_str("; ");
                existing.push_str(reason);
            }
        }
        Err(i) => b.data.quarantine.insert(
            i,
            Quarantine {
                record,
                reason: reason.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use atlas_core::graph::Graph;
    use serde_json::json;

    use super::super::fixtures::{self, TempData};
    use super::super::outcomes;

    #[test]
    fn quarantined_records_are_marked_and_missing_file_means_none() {
        let atlas = fixtures::atlas();
        let d = TempData::new();
        d.envelope(
            outcomes::FILE,
            "outcomes.resources",
            json!([
                {"id": "outcomes:a", "name": "A", "kind": "outcome_measure", "url": "https://a.example", "mappings": []},
                {"id": "outcomes:b", "name": "B", "kind": "outcome_measure", "url": "https://b.example", "mappings": []}
            ]),
        );
        let mut b = fixtures::builder(&atlas);
        outcomes::ingest(&mut b, d.path()).unwrap();
        assert_eq!(
            super::apply(
                &mut b,
                &crate::withhold::load(d.path(), crate::withhold::salt_from_env()).unwrap()
            ),
            0,
            "no file: nothing quarantined"
        );
        super::mark(&mut b, 0, "source integrity failed");
        assert_eq!(
            super::apply(
                &mut b,
                &crate::withhold::load(d.path(), crate::withhold::salt_from_env()).unwrap()
            ),
            1
        );
        let q = json!({
            "schema": "quarantine", "version": 1, "header": {},
            "records": [{"file": "outcomes/resources.json", "record_locator": "/records/1", "reason": "fetched_after_block"}]
        });
        d.write(super::FILE, q.to_string().as_bytes());
        assert_eq!(
            super::apply(
                &mut b,
                &crate::withhold::load(d.path(), crate::withhold::salt_from_env()).unwrap()
            ),
            2
        );
        let g = Graph::new(b.data);
        assert_eq!(
            g.node_quarantined(g.node("outcomes:a").unwrap()),
            Some("source integrity failed")
        );
        assert_eq!(
            g.node_quarantined(g.node("outcomes:b").unwrap()),
            Some("fetched_after_block")
        );
    }
}
