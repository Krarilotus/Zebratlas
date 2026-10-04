//! Snapshot-derived, compact facet postings for typed graph plans. No stored facts change.
//! Candidate indexes retain source order; callers must still check node/edge visibility.
use std::collections::HashMap;

use super::{GraphData, Relation};
use crate::text::normalize_label;

#[derive(Debug, Default)]
pub struct QueryIndex {
    relations: HashMap<Relation, Vec<u32>>,
    countries: HashMap<String, Vec<u32>>,
    statuses: HashMap<String, Vec<u32>>,
    studies: Vec<u32>,
}

impl QueryIndex {
    pub(super) fn build(data: &GraphData) -> Self {
        let mut index = Self::default();
        for (i, edge) in data.edges.iter().enumerate() {
            index.relations.entry(edge.relation).or_default().push(i as u32);
        }
        for (i, study) in data.studies.iter().enumerate() {
            let i = i as u32;
            index.studies.push(i);
            index
                .statuses
                .entry(normalize_label(&study.status))
                .or_default()
                .push(i);
            for country in &study.countries {
                let list = index.countries.entry(normalize_label(country)).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
        }
        index
    }

    pub fn edges(&self, relation: Relation) -> &[u32] {
        self.relations.get(&relation).map_or(&[], Vec::as_slice)
    }

    /// Start with the smaller posting list; intersect only its candidates, never the graph.
    pub fn studies(&self, country: Option<&str>, status: Option<&str>) -> impl Iterator<Item = u32> + '_ {
        let countries = country.map(|c| self.countries.get(&normalize_label(c)).map_or(&[][..], Vec::as_slice));
        let statuses = status.map(|s| self.statuses.get(&normalize_label(s)).map_or(&[][..], Vec::as_slice));
        let (first, second) = match (countries, statuses) {
            (Some(c), Some(s)) if c.len() <= s.len() => (c, Some(s)),
            (Some(c), Some(s)) => (s, Some(c)),
            (Some(c), None) => (c, None),
            (None, Some(s)) => (s, None),
            (None, None) => (self.studies.as_slice(), None),
        };
        first
            .iter()
            .copied()
            .filter(move |i| second.is_none_or(|s| s.binary_search(i).is_ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Study, StudyKind};

    fn study(status: &str, countries: &[&str]) -> Study {
        Study {
            id: "fixture".into(),
            title: "Synthetic fixture".into(),
            status: status.into(),
            kind: StudyKind::Trial,
            phases: vec![],
            sponsor: String::new(),
            sponsor_class: String::new(),
            start: String::new(),
            completion: String::new(),
            enrollment: None,
            countries: countries.iter().map(|s| s.to_string()).collect(),
            interventions: vec![],
            record: 0,
        }
    }

    #[test]
    fn facets_equal_reference_scan_and_do_not_duplicate_candidates() {
        let data = GraphData {
            studies: vec![
                study("RECRUITING", &["Germany", "Germany"]),
                study("COMPLETED", &["Germany"]),
                study("RECRUITING", &["France"]),
            ],
            ..Default::default()
        };
        let index = QueryIndex::build(&data);
        for country in [None, Some("Germany"), Some("France"), Some("missing")] {
            for status in [None, Some("RECRUITING"), Some("COMPLETED"), Some("missing")] {
                let reference: Vec<_> = data
                    .studies
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        country.is_none_or(|c| s.countries.iter().any(|x| x == c))
                            && status.is_none_or(|x| s.status == x)
                    })
                    .map(|(i, _)| i as u32)
                    .collect();
                assert_eq!(index.studies(country, status).collect::<Vec<_>>(), reference);
            }
        }
        assert_eq!(
            index.studies(Some("germany"), Some("recruiting")).collect::<Vec<_>>(),
            vec![0]
        );
    }
}
