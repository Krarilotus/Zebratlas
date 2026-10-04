//! Label-free robustness study on existing names/symbols. Not a human-query accuracy test.
use atlas_core::{
    Atlas, Graph,
    search::{SearchOptions, domain::Index},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

fn variants(label: &str, gene: bool) -> Vec<(&'static str, String)> {
    let chars: Vec<_> = label.chars().collect();
    let n = chars.len();
    if n < 4 {
        return vec![];
    }
    let at = (n / 2..n)
        .chain(0..n / 2)
        .find(|&i| chars[i].is_ascii_alphabetic())
        .unwrap_or(n / 2);
    let mut out = vec![];
    let mut c = chars.clone();
    c.remove(at);
    out.push(("deletion", c.iter().collect()));
    if let Some(i) = (0..n - 1)
        .min_by_key(|&i| i.abs_diff(at))
        .filter(|&i| chars[i] != chars[i + 1])
    {
        let mut c = chars.clone();
        c.swap(i, i + 1);
        out.push(("transposition", c.iter().collect()));
    }
    let rows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    if let Some((row, pos)) = rows
        .iter()
        .find_map(|row| row.find(chars[at].to_ascii_lowercase()).map(|p| (*row, p)))
    {
        let next = row.as_bytes()[if pos + 1 < row.len() { pos + 1 } else { pos - 1 }] as char;
        let mut c = chars.clone();
        c[at] = next;
        out.push(("keyboard", c.iter().collect()));
    }
    out.push(("case", label.to_lowercase()));
    if gene {
        let mut c = chars.clone();
        c.insert(at, ' ');
        out.push(("gene_space", c.iter().collect()));
        let mut c = chars;
        c.insert(at, '-');
        out.push(("gene_hyphen", c.iter().collect()));
    }
    out
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * p).round() as usize]
}

pub fn evaluate(atlas: &Atlas, graph: &Graph, index: &Index) -> Value {
    let genes: Vec<_> = atlas
        .genes()
        .iter()
        .filter(|g| g.symbol.len() >= 4)
        .map(|g| (g.id().to_owned(), g.symbol.clone(), true))
        .collect();
    let diseases: Vec<_> = atlas
        .active()
        .filter(|(_, d)| (4..=90).contains(&d.name.chars().count()))
        .map(|(_, d)| (d.id.clone(), d.name.clone(), false))
        .collect();
    let mut names = vec![];
    for list in [&genes, &diseases] {
        for n in (0..list.len()).step_by((list.len() / 100).max(1)).take(100) {
            names.push(list[n].clone());
        }
    }
    let mut rows = vec![];
    let mut prefixes = vec![];
    let mut counts = BTreeMap::<String, (usize, usize)>::new();
    for (id, label, gene) in &names {
        for (mutation, query) in variants(label, *gene) {
            let t = Instant::now();
            let hits = index.suggest(
                atlas,
                graph,
                &query,
                SearchOptions {
                    limit: 3,
                    include_retired: false,
                },
            );
            let elapsed = t.elapsed().as_secs_f64() * 1000.0;
            let success = hits.iter().any(|h| &h.node.id == id);
            let count = counts
                .entry(format!("{}:{mutation}", if *gene { "gene" } else { "condition" }))
                .or_default();
            count.0 += usize::from(success);
            count.1 += 1;
            rows.push(
                json!({"target":id,"label":label,"mutation":mutation,"query":query,"top3":success,
                "elapsed_ms":elapsed,"hits":hits}),
            );
        }
        for len in [1, 2, 3] {
            let q: String = label.chars().take(len).collect();
            let t = Instant::now();
            let hits = index.suggest(
                atlas,
                graph,
                &q,
                SearchOptions {
                    limit: 8,
                    include_retired: false,
                },
            );
            prefixes.push(json!({"query":q,"elapsed_ms":t.elapsed().as_secs_f64()*1000.0,"count":hits.len()}));
        }
    }
    let mut times: Vec<_> = rows.iter().map(|r| r["elapsed_ms"].as_f64().unwrap()).collect();
    let mut prefix_times: Vec<_> = prefixes.iter().map(|r| r["elapsed_ms"].as_f64().unwrap()).collect();
    let successes = rows.iter().filter(|r| r["top3"] == true).count();
    let groups: Vec<_> = counts
        .into_iter()
        .map(|(key, (hits, n))| json!({"group":key,"hits":hits,"n":n,"rate":hits as f64/n as f64}))
        .collect();
    json!({"schema":"search-ir-spelling-evaluation/1","method":"atlas-spelling-v1","sampled_names":names.len(),
        "n":rows.len(),"top3_hits":successes,"top3_rate":successes as f64/rows.len() as f64,"groups":groups,
        "latency_ms":{"spelling_median":percentile(&mut times,0.5),"spelling_p95":percentile(&mut times,0.95),
            "typeahead_median":percentile(&mut prefix_times,0.5),"typeahead_p95":percentile(&mut prefix_times,0.95)},
        "rows":rows,"prefixes":prefixes,
        "limitations":["Synthetic mutations from indexed labels: label-free robustness, not clinical or human-query accuracy.",
            "Development study, no independent confirmatory set; related gene symbols and duplicate condition names are legitimate ambiguities.",
            "Nearby suggestions use bounded lexical/phonetic candidate generation, not an exhaustive all-name nearest-neighbour guarantee.",
            "Type-ahead measures suggestions only: no model, graph traversal or clinical scoring."]})
}
