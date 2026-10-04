//! Process hierarchy with gene annotations: Reactome pathways and GO biological process share it.
//!
//! IC(p) = -ln(n_p / N): n_p genes annotated to p or a descendant, N annotated genes (the same
//! definition as the Python prototype's Reactome IC). Closures include the term itself.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Dense index into [`ProcessOntology`] terms.
pub type ProcessIdx = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessKind {
    Reactome,
    GoBp,
}

impl ProcessKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reactome => "reactome",
            Self::GoBp => "go_bp",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessOntology {
    pub ids: Vec<String>,
    pub names: Vec<String>,
    pub parents: Vec<Vec<ProcessIdx>>,
    /// Gene key -> directly annotated processes (Reactome: NCBI Gene id; GO: HGNC symbol).
    pub by_gene: HashMap<String, Vec<ProcessIdx>>,
    /// IC per process; `None` when no annotated gene reaches it.
    pub ic: Vec<Option<f64>>,
    /// Genes with at least one annotation (N in the IC).
    pub annotated_genes: usize,
    /// GO `regulates` / `positively_regulates` / `negatively_regulates` targets per term (not part
    /// of the closure used for similarity; used to find the regulators of a process).
    pub regulates: Vec<Vec<ProcessIdx>>,
    /// Named subsets (GO slims), e.g. `goslim_generic` -> member terms.
    pub subsets: std::collections::BTreeMap<String, Vec<ProcessIdx>>,
    #[serde(skip)]
    index: HashMap<String, ProcessIdx>,
    #[serde(skip)]
    ancestors: Vec<Box<[ProcessIdx]>>,
}

impl ProcessOntology {
    /// Index of `id`, adding a nameless term when new.
    pub fn intern(&mut self, id: &str) -> ProcessIdx {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        let i = self.ids.len() as ProcessIdx;
        self.ids.push(id.to_owned());
        self.names.push(String::new());
        self.parents.push(Vec::new());
        self.index.insert(id.to_owned(), i);
        i
    }

    pub fn get(&self, id: &str) -> Option<ProcessIdx> {
        self.index.get(id).copied()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn id(&self, p: ProcessIdx) -> &str {
        &self.ids[p as usize]
    }

    pub fn name(&self, p: ProcessIdx) -> &str {
        &self.names[p as usize]
    }

    /// Reflexive-transitive superclasses, sorted.
    pub fn ancestors(&self, p: ProcessIdx) -> &[ProcessIdx] {
        &self.ancestors[p as usize]
    }

    /// IC, 0 where undefined (as `pw.ic.get(p, 0.0)` in the prototype).
    pub fn ic(&self, p: ProcessIdx) -> f64 {
        self.ic[p as usize].unwrap_or(0.0)
    }

    /// Union of the closures of a gene's direct annotations, sorted.
    pub fn gene_closure(&self, gene_key: &str) -> Vec<ProcessIdx> {
        let mut out: Vec<ProcessIdx> = self
            .by_gene
            .get(gene_key)
            .into_iter()
            .flatten()
            .flat_map(|&p| self.ancestors(p).iter().copied())
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Genes whose closure contains `p`.
    pub fn genes_under(&self, p: ProcessIdx) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .by_gene
            .iter()
            .filter(|(_, ps)| ps.iter().any(|&q| self.ancestors(q).binary_search(&p).is_ok()))
            .map(|(g, _)| g.as_str())
            .collect();
        out.sort_unstable();
        out
    }

    /// Rebuild the derived index and closures (after parsing or loading a snapshot).
    pub fn finish(&mut self) {
        self.index = self
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i as ProcessIdx))
            .collect();
        self.ancestors = closure(&self.parents);
    }

    /// Count genes per process closure and set the IC (call after [`Self::finish`]).
    pub fn compute_ic(&mut self) {
        let mut counts = vec![0u64; self.len()];
        let mut stamp = vec![usize::MAX; self.len()];
        let mut n = 0usize;
        for terms in self.by_gene.values() {
            if terms.is_empty() {
                continue;
            }
            for &t in terms {
                for &a in self.ancestors(t) {
                    if stamp[a as usize] != n {
                        stamp[a as usize] = n;
                        counts[a as usize] += 1;
                    }
                }
            }
            n += 1;
        }
        self.annotated_genes = n;
        let total = n as f64;
        self.ic = counts
            .iter()
            .map(|&c| (c > 0).then(|| -(c as f64 / total).ln()))
            .collect();
    }
}

fn closure(parents: &[Vec<ProcessIdx>]) -> Vec<Box<[ProcessIdx]>> {
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

    #[test]
    fn closure_and_ic() {
        let mut o = ProcessOntology::default();
        let root = o.intern("R");
        let a = o.intern("A");
        let b = o.intern("B");
        o.parents[a as usize].push(root);
        o.parents[b as usize].push(a);
        o.by_gene.insert("g1".into(), vec![b]);
        o.by_gene.insert("g2".into(), vec![a]);
        o.finish();
        o.compute_ic();
        assert_eq!(o.ancestors(b), &[root, a, b]);
        assert_eq!(o.ic(root), 0.0);
        assert!((o.ic(b) - 2f64.ln()).abs() < 1e-12);
        assert_eq!(o.gene_closure("g1"), vec![root, a, b]);
        assert_eq!(o.genes_under(a), vec!["g1", "g2"]);
    }
}
