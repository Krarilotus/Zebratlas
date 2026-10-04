//! simGIC, quantiles and the seeded shuffle the similarity index uses.

/// simGIC over sorted (key, IC) lists: (score, shared keys).
pub fn sim_gic(a: &[(u32, f64)], b: &[(u32, f64)]) -> (f64, Vec<u32>) {
    let (mut i, mut j) = (0, 0);
    let mut shared = Vec::new();
    let (mut s_shared, mut s_b_shared) = (0.0, 0.0);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared.push(a[i].0);
                s_shared += a[i].1;
                s_b_shared += b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    let sum_a: f64 = a.iter().map(|x| x.1).sum();
    let sum_b: f64 = b.iter().map(|x| x.1).sum();
    let union = sum_a + sum_b - s_b_shared;
    (if union > 0.0 { s_shared / union } else { 0.0 }, shared)
}

/// Score only, no allocation of the shared list.
pub(super) fn sim_gic_score(a: &[(u32, f64)], b: &[(u32, f64)]) -> f64 {
    let (mut i, mut j) = (0, 0);
    let (mut s_shared, mut s_b_shared) = (0.0, 0.0);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                s_shared += a[i].1;
                s_b_shared += b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    let sum_a: f64 = a.iter().map(|x| x.1).sum();
    let sum_b: f64 = b.iter().map(|x| x.1).sum();
    let union = sum_a + sum_b - s_b_shared;
    if union > 0.0 { s_shared / union } else { 0.0 }
}

/// Deterministic generator for sampling (splitmix64).
#[derive(Clone, Debug)]
pub struct SplitMix(pub u64);

impl SplitMix {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

pub(super) fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let pos = q * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gic_matches_definition() {
        let a = [(1, 1.0), (2, 2.0), (3, 3.0)];
        let b = [(2, 2.0), (3, 3.0), (4, 4.0)];
        let (s, shared) = sim_gic(&a, &b);
        assert_eq!(shared, vec![2, 3]);
        assert!((s - 5.0 / 10.0).abs() < 1e-12);
        assert_eq!(sim_gic_score(&a, &b), s);
        assert_eq!(sim_gic(&[], &[]).0, 0.0);
    }

    #[test]
    fn quantiles_and_rng() {
        assert_eq!(quantile(vec![3.0, 1.0, 2.0], 0.5), 2.0);
        let mut r = SplitMix(1);
        let mut v: Vec<u32> = (0..10).collect();
        r.shuffle(&mut v);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, (0..10).collect::<Vec<_>>());
    }
}
