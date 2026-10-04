//! Candidate generation only: spelling and phonetics never establish identity.
use super::*;

#[derive(Debug, Default)]
pub(super) struct Spelling {
    compact_genes: BTreeMap<String, Vec<u32>>,
    grams: HashMap<String, Vec<u32>>,
    sounds: HashMap<String, Vec<u32>>,
    lengths: BTreeMap<usize, Vec<u32>>,
    lens: Vec<usize>,
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn grams(text: &str) -> BTreeSet<String> {
    let chars: Vec<_> = text.chars().collect();
    chars.windows(2).map(|g| g.iter().collect()).collect()
}

// Conservative Latin-name Soundex. Non-Latin names still use Unicode lexical retrieval.
fn sound(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut out = String::new();
            let mut last = '0';
            for c in word.chars().filter(char::is_ascii_alphabetic) {
                let code = match c {
                    'b' | 'f' | 'p' | 'v' => '1',
                    'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => '2',
                    'd' | 't' => '3',
                    'l' => '4',
                    'm' | 'n' => '5',
                    'r' => '6',
                    _ => '0',
                };
                if out.is_empty() {
                    out.push(c);
                } else if code != '0' && code != last && out.len() < 4 {
                    out.push(code);
                }
                last = code;
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn keyboard(a: &str, b: &str) -> bool {
    let pairs: Vec<_> = a.chars().zip(b.chars()).filter(|(a, b)| a != b).collect();
    pairs.len() == 1
        && ["qwertyuiop", "asdfghjkl", "zxcvbnm", "qwertzuiop", "yxcvbnm"]
            .iter()
            .any(|row| {
                row.as_bytes().windows(2).any(|p| {
                    (p[0] as char == pairs[0].0 && p[1] as char == pairs[0].1)
                        || (p[1] as char == pairs[0].0 && p[0] as char == pairs[0].1)
                })
            })
}

impl Spelling {
    pub(super) fn build(entries: &[Entry]) -> Self {
        let mut index = Self {
            lens: entries.iter().map(|e| e.key.chars().count()).collect(),
            ..Self::default()
        };
        for (i, e) in entries.iter().enumerate() {
            if e.node.kind == NodeKind::Gene {
                index.compact_genes.entry(compact(&e.key)).or_default().push(i as u32);
            }
            if e.field == Reason::ExactId
                || !matches!(
                    e.node.kind,
                    NodeKind::Gene
                        | NodeKind::Disease
                        | NodeKind::Phenotype
                        | NodeKind::Organisation
                        | NodeKind::Person
                )
                || e.key.chars().count() > 128
            {
                continue;
            }
            index.lengths.entry(e.key.chars().count()).or_default().push(i as u32);
            for gram in grams(&e.key) {
                index.grams.entry(gram).or_default().push(i as u32);
            }
            if e.node.kind != NodeKind::Gene {
                let key = sound(&e.key);
                if !key.trim().is_empty() {
                    index.sounds.entry(key).or_default().push(i as u32);
                }
            }
        }
        index
    }

    pub(super) fn genes(&self, query: &str) -> &[u32] {
        self.compact_genes
            .get(&compact(query))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn nearest(
        &self,
        entries: &[Entry],
        query: &str,
        limit: usize,
        prefer_genes: bool,
        allowed: impl Fn(u32) -> bool,
    ) -> Vec<(u32, usize, bool)> {
        let clipped: String = query.chars().take(128).collect();
        let query = clipped.as_str();
        let n = query.chars().count();
        if n == 0 || limit == 0 {
            return vec![];
        }
        let mut counts = HashMap::<u32, usize>::new();
        for gram in grams(query) {
            if let Some(postings) = self.grams.get(&gram) {
                for &i in postings {
                    if self.lens[i as usize].abs_diff(n) <= 2 {
                        *counts.entry(i).or_default() += 1;
                    }
                }
            }
        }
        // Small symbols can lose every bigram through a transposition. Include all nearby
        // symbol lengths so those candidates are not lost at the blocking stage.
        if n <= 12 {
            for (_, ids) in self.lengths.range(n.saturating_sub(2)..=n + 2) {
                for &i in ids.iter().filter(|&&i| entries[i as usize].node.kind == NodeKind::Gene) {
                    counts.entry(i).or_default();
                }
            }
        }
        let mut shortlist: Vec<_> = counts.into_iter().filter(|(i, _)| allowed(*i)).collect();
        shortlist.sort_by_key(|&(i, count)| (std::cmp::Reverse(count), self.lens[i as usize].abs_diff(n), i));
        // Keep nearby genes as well as the lexical shortlist; cap work for long names.
        let mut ids: BTreeSet<u32> = shortlist.iter().take(256).map(|x| x.0).collect();
        if n <= 12 {
            ids.extend(
                shortlist
                    .iter()
                    .filter(|&&(i, _)| entries[i as usize].node.kind == NodeKind::Gene)
                    .map(|x| x.0),
            );
        }
        let phonetics = self.sounds.get(&sound(query));
        if let Some(ids2) = phonetics {
            ids.extend(ids2.iter().copied().filter(|&i| allowed(i)));
        }
        if ids
            .iter()
            .map(|&i| entries[i as usize].node)
            .collect::<HashSet<_>>()
            .len()
            < limit
        {
            // An unfamiliar script or completely wrong spelling still gets real choices.
            // This is a bounded length-based fallback, explicitly marked as a suggestion.
            let mut lengths: Vec<_> = self.lengths.iter().collect();
            lengths.sort_by_key(|(len, _)| len.abs_diff(n));
            ids.extend(
                lengths
                    .into_iter()
                    .flat_map(|(_, v)| v.iter())
                    .copied()
                    .filter(|&i| allowed(i))
                    .take(256),
            );
        }
        let max = if n >= 8 { 2 } else { 1 };
        let phonetics: HashSet<_> = phonetics.into_iter().flatten().copied().collect();
        // First find genuine edits with an early-exit distance. Arbitrary distant names
        // need full distances only when those candidates cannot fill the choices.
        let mut scored: Vec<_> = ids
            .iter()
            .copied()
            .filter_map(|i| {
                let e = &entries[i as usize];
                let distance = if self.lens[i as usize].abs_diff(n) > max {
                    max + 1
                } else {
                    super::super::edit_distance(query, &e.key, max)
                };
                let phonetic = phonetics.contains(&i) && distance > max;
                (distance <= max || phonetic).then_some((i, distance, phonetic, keyboard(query, &e.key)))
            })
            .collect();
        if scored
            .iter()
            .map(|x| entries[x.0 as usize].node)
            .collect::<HashSet<_>>()
            .len()
            < limit
        {
            let present: HashSet<_> = scored.iter().map(|x| x.0).collect();
            for i in ids.into_iter().filter(|i| !present.contains(i)).take(256) {
                let e = &entries[i as usize];
                let distance = super::super::edit_distance(query, &e.key, n.max(self.lens[i as usize]));
                scored.push((i, distance, false, keyboard(query, &e.key)));
            }
        }
        scored.sort_by(|a, b| {
            let fit = |x: &(u32, usize, bool, bool)| {
                if x.2 {
                    1.5
                } else {
                    x.1 as f64 - if x.3 { 0.1 } else { 0.0 }
                }
            };
            fit(a)
                .total_cmp(&fit(b))
                .then_with(|| {
                    let priority = |i| {
                        let k = entries[i as usize].node.kind;
                        if prefer_genes && k == NodeKind::Gene {
                            0
                        } else {
                            1 + kind_priority(k)
                        }
                    };
                    priority(a.0).cmp(&priority(b.0))
                })
                .then(field_order(entries[a.0 as usize].field).cmp(&field_order(entries[b.0 as usize].field)))
                .then(a.0.cmp(&b.0))
        });
        let mut seen = HashSet::new();
        scored
            .into_iter()
            .filter(|x| seen.insert(entries[x.0 as usize].node))
            .take(limit)
            .map(|(i, d, p, _)| (i, d, p))
            .collect()
    }
}
