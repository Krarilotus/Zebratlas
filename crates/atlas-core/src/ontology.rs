//! Ontology (HPO): id hygiene, hierarchy closure and information content over dense term indexes.

use std::collections::HashMap;

use crate::term::Term;

/// Dense index into [`Ontology::terms`].
pub type TermIdx = u32;

pub const PHENOTYPIC_ABNORMALITY: &str = "HP:0000118";

/// Terms plus derived closures; built once, read-only afterwards except for [`Ontology::compute_ic`].
#[derive(Clone, Debug, Default)]
pub struct Ontology {
    terms: Vec<Term>,
    index: HashMap<String, TermIdx>,
    /// alt id -> owning term (later terms win, as in the Python dict).
    alt: HashMap<String, TermIdx>,
    /// Sorted, include the term itself.
    ancestors: Vec<Box<[TermIdx]>>,
    descendants: Vec<Box<[TermIdx]>>,
    ic: Vec<f64>,
    phenotype_root: Option<TermIdx>,
}

impl Ontology {
    /// Index terms (a repeated id keeps the last term) and compute closures. Unknown parents are ignored.
    pub fn new(terms: Vec<Term>) -> Self {
        let mut index = HashMap::with_capacity(terms.len());
        let mut alt = HashMap::new();
        for (i, t) in terms.iter().enumerate() {
            index.insert(t.id.clone(), i as TermIdx);
            for a in &t.alt_ids {
                alt.insert(a.clone(), i as TermIdx);
            }
        }
        let parents: Vec<Vec<TermIdx>> = terms
            .iter()
            .map(|t| t.parents.iter().filter_map(|p| index.get(p).copied()).collect())
            .collect();
        let ancestors = closure(&parents);
        let mut down: Vec<Vec<TermIdx>> = vec![Vec::new(); terms.len()];
        for (t, anc) in ancestors.iter().enumerate() {
            for &a in anc.iter() {
                down[a as usize].push(t as TermIdx);
            }
        }
        let descendants = down.into_iter().map(Vec::into_boxed_slice).collect();
        let phenotype_root = index.get(PHENOTYPIC_ABNORMALITY).copied();
        let ic = vec![0.0; terms.len()];
        Self {
            terms,
            index,
            alt,
            ancestors,
            descendants,
            ic,
            phenotype_root,
        }
    }

    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    pub fn into_terms(self) -> Vec<Term> {
        self.terms
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn term(&self, t: TermIdx) -> &Term {
        &self.terms[t as usize]
    }

    /// Exact primary id lookup.
    pub fn get(&self, id: &str) -> Option<TermIdx> {
        self.index.get(id).copied()
    }

    /// Resolve alt ids and obsolete terms to the live term, or None if unknown.
    pub fn canonical(&self, id: &str) -> Option<TermIdx> {
        let mut t = self.alt.get(id).or_else(|| self.index.get(id)).copied()?;
        for _ in 0..32 {
            let term = self.term(t);
            if !term.obsolete {
                return Some(t);
            }
            let next = term.replaced_by.as_deref()?;
            t = self.alt.get(next).or_else(|| self.index.get(next)).copied()?;
        }
        None
    }

    /// Name of a primary id, else the id itself.
    pub fn label<'a>(&'a self, id: &'a str) -> &'a str {
        self.get(id).map_or(id, |t| &self.term(t).name)
    }

    /// All superclasses including the term itself, sorted.
    pub fn ancestors(&self, t: TermIdx) -> &[TermIdx] {
        &self.ancestors[t as usize]
    }

    /// All subclasses including the term itself, sorted.
    pub fn descendants(&self, t: TermIdx) -> &[TermIdx] {
        &self.descendants[t as usize]
    }

    pub fn is_phenotype(&self, t: TermIdx) -> bool {
        self.phenotype_root
            .is_some_and(|root| self.ancestors(t).binary_search(&root).is_ok())
    }

    /// Information content -ln p(t); 0 before [`Ontology::compute_ic`].
    pub fn ic(&self, t: TermIdx) -> f64 {
        self.ic[t as usize]
    }

    /// Share of annotated diseases that have the term or a descendant: exp(-IC).
    pub fn background(&self, t: TermIdx) -> f64 {
        (-self.ic[t as usize]).exp()
    }

    /// IC(t) = -ln(p(t)), p(t) = share of diseases annotated with t or a descendant.
    /// Unannotated terms get -ln(1/(total+1)): as specific as one disease.
    pub fn compute_ic<I, T>(&mut self, annotated: I)
    where
        I: IntoIterator<Item = T>,
        T: IntoIterator<Item = TermIdx>,
    {
        let mut counts = vec![0u64; self.terms.len()];
        let mut stamp = vec![u32::MAX; self.terms.len()];
        let mut total = 0u32;
        for terms in annotated {
            for t in terms {
                for &a in self.ancestors(t) {
                    if stamp[a as usize] != total {
                        stamp[a as usize] = total;
                        counts[a as usize] += 1;
                    }
                }
            }
            total += 1;
        }
        let total = f64::from(total);
        let floor = -(1.0 / (total + 1.0)).ln();
        self.ic = counts
            .iter()
            .map(|&c| if c > 0 { -(c as f64 / total).ln() } else { floor })
            .collect();
    }
}

/// Reflexive-transitive closure over `parents`, each set sorted.
fn closure(parents: &[Vec<TermIdx>]) -> Vec<Box<[TermIdx]>> {
    let mut seen = vec![u32::MAX; parents.len()];
    let mut stack = Vec::new();
    (0..parents.len())
        .map(|start| {
            let mark = start as u32;
            let mut out = vec![mark];
            seen[start] = mark;
            stack.push(mark);
            while let Some(t) = stack.pop() {
                for &p in &parents[t as usize] {
                    if seen[p as usize] != mark {
                        seen[p as usize] = mark;
                        out.push(p);
                        stack.push(p);
                    }
                }
            }
            out.sort_unstable();
            out.into_boxed_slice()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(id: &str, parents: &[&str]) -> Term {
        Term {
            parents: parents.iter().map(|p| p.to_string()).collect(),
            ..Term::new(id)
        }
    }

    fn hpo() -> Ontology {
        let mut seizure = term("HP:0001250", &["HP:0000001"]);
        seizure.alt_ids.push("HP:0002279".into());
        let mut obsolete = term("HP:0009999", &[]);
        obsolete.obsolete = true;
        obsolete.replaced_by = Some("HP:0007359".into());
        Ontology::new(vec![
            term("HP:0000001", &[]),
            seizure,
            term("HP:0007359", &["HP:0001250"]),
            obsolete,
        ])
    }

    #[test]
    fn hierarchy_and_ids() {
        let o = hpo();
        let seizure = o.get("HP:0001250").unwrap();
        let focal = o.get("HP:0007359").unwrap();
        assert_eq!(o.canonical("HP:0002279"), Some(seizure));
        assert_eq!(o.canonical("HP:0009999"), Some(focal));
        assert_eq!(o.canonical("HP:1234567"), None);
        assert_eq!(o.ancestors(focal), &[0, 1, 2]);
        assert_eq!(o.descendants(0), &[0, 1, 2]);
    }

    #[test]
    fn information_content() {
        let mut o = hpo();
        o.compute_ic([vec![2], vec![1], vec![0]]);
        assert!(o.ic(2) > o.ic(1) && o.ic(1) > o.ic(0) && o.ic(0) == 0.0);
        assert!((o.ic(3) - 4f64.ln()).abs() < 1e-12); // unannotated: -ln(1/4)
    }
}
