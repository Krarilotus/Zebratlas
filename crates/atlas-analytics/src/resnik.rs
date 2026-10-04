//! The Resnik baseline of the evaluation (`eval/baselines.py: Baselines.resnik`), one-sided and
//! patient-IC-normalised: for each patient term q, IC of the most informative ancestor of q that is
//! in the disease's annotation closure; summed over q in query order, divided by the patient's total
//! IC. Ignores frequencies, evidence codes and excluded terms. Same float operations in the same
//! order as the Python, so scores are bit-identical on the same atlas.

use atlas_core::{Atlas, DiseaseIdx, TermIdx};

pub struct Resnik {
    /// Term -> candidate slots whose annotation closure contains the term.
    by_term: Vec<Vec<u32>>,
    /// Term -> its ancestors (itself included) with IC > 0, most informative first.
    ranked: Vec<Vec<TermIdx>>,
    candidates: usize,
}

impl Resnik {
    /// Index the positive annotation closures of `candidates` (slot order = score order).
    pub fn new(atlas: &Atlas, candidates: &[DiseaseIdx]) -> Self {
        let hpo = &atlas.hpo;
        let mut by_term = vec![Vec::new(); hpo.len()];
        let mut mark = vec![u32::MAX; hpo.len()];
        for (slot, &did) in candidates.iter().enumerate() {
            for e in &atlas.disease_at(did).phenotypes {
                for &a in hpo.ancestors(e.term) {
                    if mark[a as usize] != slot as u32 {
                        mark[a as usize] = slot as u32;
                        by_term[a as usize].push(slot as u32);
                    }
                }
            }
        }
        let ranked = (0..hpo.len() as TermIdx)
            .map(|t| {
                let mut r: Vec<TermIdx> = hpo.ancestors(t).iter().copied().filter(|&a| hpo.ic(a) > 0.0).collect();
                r.sort_by(|&a, &b| hpo.ic(b).total_cmp(&hpo.ic(a)));
                r
            })
            .collect();
        Self {
            by_term,
            ranked,
            candidates: candidates.len(),
        }
    }

    /// Score vector over the candidate slots for canonical patient terms.
    pub fn scores(&self, atlas: &Atlas, present: &[TermIdx]) -> Vec<f64> {
        let ic = |t: TermIdx| atlas.hpo.ic(t);
        let mut scores = vec![0.0; self.candidates];
        // Python >= 3.12 sum() over floats is compensated
        let total = crate::matcher::python_fsum(present.iter().map(|&q| ic(q)));
        let total = if total == 0.0 { 1.0 } else { total };
        let mut seen = vec![false; self.candidates];
        for &q in present {
            seen.fill(false);
            for &a in &self.ranked[q as usize] {
                let value = ic(a);
                for &s in &self.by_term[a as usize] {
                    if !seen[s as usize] {
                        seen[s as usize] = true;
                        scores[s as usize] += value;
                    }
                }
            }
        }
        scores.iter_mut().for_each(|s| *s /= total);
        scores
    }
}
