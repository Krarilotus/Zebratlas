//! Shared-symptom neighbours: conditions that share several uncommon (high information content)
//! HPO terms directly with a condition.

use std::collections::HashMap;

use atlas_core::{Atlas, DiseaseIdx, TermIdx};

/// Symptoms below this information content are too common to say two conditions are alike.
pub const MIN_IC: f64 = 2.5;
pub const MIN_SHARED: usize = 3;

/// One neighbour: its summed IC over shared terms divided by the square root of its phenotype
/// count, and the shared terms, most informative first.
#[derive(Clone, Debug)]
pub struct SharedSymptoms {
    pub disease: DiseaseIdx,
    pub score: f64,
    pub terms: Vec<TermIdx>,
}

/// Conditions sharing at least [`MIN_SHARED`] terms of IC ≥ [`MIN_IC`] with `d`, best first.
pub fn shared_symptoms(atlas: &Atlas, d: DiseaseIdx) -> Vec<SharedSymptoms> {
    let dis = atlas.disease_at(d);
    let mut score: HashMap<DiseaseIdx, (f64, Vec<TermIdx>)> = HashMap::new();
    for e in dis.phenotypes.iter().filter(|e| atlas.hpo.is_phenotype(e.term)) {
        let ic = atlas.hpo.ic(e.term);
        if ic < MIN_IC {
            continue;
        }
        for &o in atlas.by_phenotype(e.term) {
            if o != d {
                let s = score.entry(o).or_default();
                s.0 += ic;
                s.1.push(e.term);
            }
        }
    }
    let mut ranked: Vec<SharedSymptoms> = score
        .into_iter()
        .filter(|(_, (_, t))| t.len() >= MIN_SHARED)
        .map(|(o, (s, mut t))| {
            let n = atlas.disease_at(o).phenotypes.len().max(1) as f64;
            t.sort_by(|a, b| atlas.hpo.ic(*b).total_cmp(&atlas.hpo.ic(*a)));
            SharedSymptoms {
                disease: o,
                score: s / n.sqrt(),
                terms: t,
            }
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.disease.cmp(&b.disease)));
    ranked
}
