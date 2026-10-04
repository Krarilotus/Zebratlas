//! Mechanism layer report: related conditions for the demo genes, GO-BP vs Reactome comparison,
//! clusters of the DEE / vesicle-release slice and the held-out validation.
//!
//! `cargo run -p atlas-analytics --example mechanism_report --release -- [related|compare|clusters|validate|all] [out.json]`

use std::path::PathBuf;
use std::sync::Arc;

use atlas_analytics::cluster::{self, ClusterParams, ClusterReport, Slice};
use atlas_analytics::validation::{self, Run};
use atlas_analytics::{EffectRule, ProcessSet, RelatedReport, SimilarityIndex, SimilarityParams};
use atlas_core::DiseaseIdx;
use atlas_ingest::mechanism;

const DEMO: [(&str, &str); 4] = [
    ("STXBP1 DEE4", "MONDO:0012812"),
    ("KCNQ2 DEE7", "MONDO:0013387"),
    ("KCNQ2 benign neonatal", "MONDO:0007365"),
    ("UNC13A biallelic LoF", "G2P:G2P03909"),
];

fn print_related(r: &RelatedReport) {
    println!("\n== {} {}", r.query.id, r.query.label);
    for g in &r.genes {
        let eff: Vec<&str> = g.effects.iter().map(|e| e.as_str()).collect();
        let src: Vec<String> = g
            .sources
            .iter()
            .map(|s| {
                format!(
                    "{}:{}{}",
                    s.source,
                    s.effect.map_or("-", |e| e.as_str()),
                    s.allelic_requirement
                        .as_ref()
                        .map(|a| format!("[{a}]"))
                        .unwrap_or_default()
                )
            })
            .collect();
        let hi = g
            .dosage
            .as_ref()
            .map(|d| format!(" HI={}", d.haploinsufficiency))
            .unwrap_or_default();
        println!("   gene {} effects {:?} sources {:?}{hi}", g.symbol, eff, src);
    }
    for it in &r.items {
        let genes: Vec<&str> = it.mechanism.shared_genes.iter().map(|g| g.symbol.as_str()).collect();
        let pr: Vec<String> = it
            .mechanism
            .shared_processes
            .iter()
            .take(2)
            .map(|p| format!("{}:{}", p.source, p.name))
            .collect();
        let ph: Vec<&str> = it
            .phenotype
            .shared
            .iter()
            .take(3)
            .map(|t| t.term.label.as_str())
            .collect();
        println!(
            "  {:.3} ph {:.3} me {:.3} {:?} {} {} | genes {:?} | {:?} | {:?}",
            it.score,
            it.phenotype.score,
            it.mechanism.score,
            it.verdict,
            it.neighbour.id,
            trunc(&it.neighbour.label, 45),
            genes,
            pr,
            ph
        );
    }
    for c in &r.counterexamples {
        let n = c
            .neighbour
            .as_ref()
            .map(|n| format!("{} {}", n.neighbour.id, trunc(&n.neighbour.label, 40)))
            .unwrap_or_default();
        println!("  COUNTER {:?} {n}: {}", c.kind, c.why);
    }
}

const TOP: usize = 15;
/// Presynaptic release machinery (SNARE/SM/priming/Ca-sensor genes), from the SNAREopathy literature
/// (Verhage & Sorensen 2020, Neuron 107:22-37): used only to judge, never to rank.
const RELEASE_MACHINERY: [&str; 14] = [
    "STXBP1", "STX1A", "STX1B", "SNAP25", "VAMP2", "SYT1", "SYT2", "CPLX1", "UNC13A", "UNC13B", "RIMS1", "RAB3A",
    "STXBP5L", "DNM1",
];

fn print_clusters(r: &ClusterReport) {
    println!(
        "\nslice {} clusters {} modularity {:.3} seed ARI {:.2} bootstrap ARI {:.2}",
        r.slice_size,
        r.clusters.len(),
        r.modularity,
        r.seed_ari,
        r.bootstrap_ari
    );
    for c in &r.clusters {
        println!(
            "\n[{}] {} members, stability seed {:.2} boot {:.2} ({}) | {}",
            c.id,
            c.members.len(),
            c.stability.seed,
            c.stability.bootstrap,
            c.stability.verdict,
            c.label
        );
        println!("     genes {:?}", c.genes);
        for w in &c.why {
            println!("     - {w}");
        }
        for e in c.edges.iter().take(2) {
            println!(
                "     edge {:.2} {} -- {} : {}",
                e.weight,
                trunc(&e.a.label, 30),
                trunc(&e.b.label, 30),
                e.reason
            );
        }
        for x in c.counterexamples.iter().take(2) {
            println!(
                "     counter {} vs {} ({}): {}",
                x.member.id, x.other.id, x.other_cluster, x.why
            );
        }
    }
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let what = args.get(1).map_or("all", String::as_str);
    let data = atlas_ingest::data_dir();
    let (atlas, _) = atlas_ingest::load_or_build(&data, false).expect("atlas");
    let (m, _) = mechanism::load_or_build(&data, &atlas, false).expect("mechanism");
    let (atlas, m) = (Arc::new(atlas), Arc::new(m));
    let t = std::time::Instant::now();
    let index = SimilarityIndex::new(atlas.clone(), m.clone(), SimilarityParams::default());
    eprintln!(
        "index {:.1}s thresholds {:?}",
        t.elapsed().as_secs_f64(),
        index.thresholds()
    );
    let mut out = serde_json::Map::new();
    if what == "related" || what == "all" {
        let mut reports = Vec::new();
        for (_, id) in DEMO {
            match index.related(id, 10) {
                Ok(r) => {
                    print_related(&r);
                    reports.push(r);
                }
                Err(e) => println!("{e}"),
            }
        }
        out.insert("related".into(), serde_json::to_value(&reports).unwrap());
    }
    if what == "compare" || what == "all" {
        let mut compare = Vec::new();
        for set in [ProcessSet::Reactome, ProcessSet::GoBp, ProcessSet::Both] {
            let idx = SimilarityIndex::new(
                atlas.clone(),
                m.clone(),
                SimilarityParams {
                    processes: set,
                    ..SimilarityParams::default()
                },
            );
            println!("\n##### processes = {set:?} thresholds {:?}", idx.thresholds());
            print_related(&idx.related("MONDO:0012812", 10).unwrap());
            // process-only neighbourhood: no shared gene, ranked by process simGIC; judged by
            // (a) phenotype similarity to STXBP1 DEE4 (a signal the ranking did not use) and
            // (b) causal genes in a literature list of presynaptic release-machinery genes
            let q = idx.resolve("MONDO:0012812").unwrap();
            let mut rows: Vec<(f64, f64, DiseaseIdx)> = atlas
                .active()
                .map(|(i, _)| i)
                .filter(|&i| i != q)
                .filter(|&i| {
                    idx.profile(i)
                        .is_some_and(|p| !p.phenotype.is_empty() && p.genes.len() <= 20)
                })
                .map(|i| (idx.score(q, i), i))
                .filter(|(s, _)| !s.compatible_gene)
                .map(|(s, i)| (s.process, s.phenotype, i))
                .collect();
            rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)));
            let top: Vec<_> = rows.iter().take(TOP).collect();
            let mean_ph = top.iter().map(|r| r.1).sum::<f64>() / top.len() as f64;
            let release = top
                .iter()
                .filter(|r| {
                    idx.profile(r.2)
                        .unwrap()
                        .genes
                        .iter()
                        .any(|g| RELEASE_MACHINERY.contains(&g.symbol.as_str()))
                })
                .count();
            let strong_ph = top.iter().filter(|r| r.1 >= idx.thresholds().phenotype_strong).count();
            println!(
                "  process-only top {TOP}: mean phenotype {mean_ph:.3}, phenotype-strong {strong_ph}/{TOP}, release-machinery genes {release}/{TOP}"
            );
            let mut listed = Vec::new();
            for (pr, ph, i) in &top {
                let genes: Vec<String> = idx
                    .profile(*i)
                    .unwrap()
                    .genes
                    .iter()
                    .map(|g| g.symbol.clone())
                    .collect();
                let me = idx.mechanism(q, *i);
                let proc: Vec<String> = me.shared_processes.iter().take(2).map(|p| p.name.clone()).collect();
                println!(
                    "    pr {pr:.3} ph {ph:.3} {} {} {:?} {:?}",
                    atlas.disease_at(*i).id,
                    trunc(&atlas.disease_at(*i).name, 40),
                    genes,
                    proc
                );
                listed.push(serde_json::json!({"id": atlas.disease_at(*i).id, "name": atlas.disease_at(*i).name, "genes": genes, "process": pr, "phenotype": ph, "shared_processes": proc}));
            }
            compare.push(serde_json::json!({"processes": set, "mean_phenotype": mean_ph, "phenotype_strong": strong_ph, "release_machinery": release, "top": listed}));
        }
        out.insert("compare".into(), serde_json::Value::Array(compare));
    }
    if what == "genes" || what == "all" {
        let mut views = Vec::new();
        for g in ["UNC13A", "KCNQ2", "STXBP1"] {
            let v = index.gene_mechanisms(g);
            println!("\n== gene {g} split={} {}", v.split, v.why.clone().unwrap_or_default());
            for c in &v.curations {
                let conds: Vec<&str> = c.conditions.iter().map(|n| n.id.as_str()).collect();
                println!(
                    "   {} {} [{}] {} {:?} {} {:?}",
                    c.g2p_id, c.confidence, c.allelic_requirement, c.mechanism, c.support, c.disease_name, conds
                );
            }
            views.push(v);
        }
        out.insert("genes".into(), serde_json::to_value(&views).unwrap());
    }
    if what == "prototype" || what == "all" {
        // the prototype's run: Reactome only, unioned effects, DEE subtrees only, all diseases kept
        let idx = SimilarityIndex::new(
            atlas.clone(),
            m.clone(),
            SimilarityParams {
                max_neighbour_genes: usize::MAX,
                ..SimilarityParams::prototype()
            },
        );
        let mut members: Vec<DiseaseIdx> = cluster::DEE_ROOTS
            .iter()
            .flat_map(|r| idx.subtree(r))
            .filter(|&d| {
                idx.profile(d)
                    .is_some_and(|p| !p.phenotype.is_empty() && !p.genes.is_empty())
            })
            .collect();
        members.sort_unstable();
        members.dedup();
        let slice = Slice::custom(
            &idx,
            members,
            "DEE subtrees, phenotypes and genes (explore_clusters.py)",
        );
        let r = cluster::clusters(&idx, &slice, &ClusterParams::default());
        println!("\n##### prototype setting (python: 126 diseases, 6 clusters, modularity 0.422)");
        print_clusters(&r);
        out.insert("prototype_clusters".into(), serde_json::to_value(&r).unwrap());
    }
    let slice = cluster::dee_slice(&index);
    if what == "clusters" || what == "all" {
        println!(
            "\nslice: {} from DEE + {} via vesicle genes ({} genes); seeds {:?}",
            slice.from_dee,
            slice.from_vesicle_genes,
            slice.seed_genes.len(),
            slice.seed_processes
        );
        let r = cluster::clusters(&index, &slice, &ClusterParams::default());
        print_clusters(&r);
        if let Some(c) = r.cluster_of(&index, "MONDO:0012812") {
            println!("\nSTXBP1 DEE4 is in {} {}", c.id, c.label);
        }
        out.insert("slice".into(), serde_json::to_value(&slice).unwrap());
        out.insert("clusters".into(), serde_json::to_value(&r).unwrap());
    }
    if what == "validate" || what == "all" {
        let make = |processes, effects| {
            SimilarityIndex::new(
                atlas.clone(),
                m.clone(),
                SimilarityParams {
                    processes,
                    effects,
                    ..SimilarityParams::default()
                },
            )
        };
        let both = make(ProcessSet::Both, EffectRule::Ignore);
        let reactome = make(ProcessSet::Reactome, EffectRule::Ignore);
        let go = make(ProcessSet::GoBp, EffectRule::Ignore);
        let runs = [
            Run {
                name: "phenotype",
                signals: "HPO only (alpha 1)",
                index: &both,
                alpha: 1.0,
            },
            Run {
                name: "phen+reactome",
                signals: "HPO + shared genes + Reactome (alpha 0.5)",
                index: &reactome,
                alpha: 0.5,
            },
            Run {
                name: "phen+go",
                signals: "HPO + shared genes + GO-BP (alpha 0.5)",
                index: &go,
                alpha: 0.5,
            },
            Run {
                name: "phen+both",
                signals: "HPO + shared genes + Reactome + GO-BP (alpha 0.5)",
                index: &both,
                alpha: 0.5,
            },
            Run {
                name: "mechanism",
                signals: "shared genes + Reactome + GO-BP (alpha 0)",
                index: &both,
                alpha: 0.0,
            },
        ];
        let g2p = |d| validation::g2p_mechanism(&index, d);
        let slim = |d| validation::go_slim(&index, d);
        let top = |d| validation::reactome_top(&index, d);
        let held: [validation::HeldOut<'_>; 3] = [
            ("g2p_mechanism", &g2p, &[]),
            ("go_slim", &slim, &["phen+go", "phen+both", "mechanism"]),
            ("reactome_top", &top, &["phen+reactome", "phen+both", "mechanism"]),
        ];
        let rows = validation::validate(&slice.members, &runs, &held, &ClusterParams::default());
        println!(
            "\n| run | signals | held out | labelled | groups | clusters | AMI | ARI | AMI shuffled | ARI shuffled | modularity | seed ARI |"
        );
        for r in &rows {
            println!(
                "| {} | {} | {} | {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.2} |",
                r.run,
                r.signals,
                r.held_out,
                r.labelled,
                r.groups,
                r.clusters,
                r.ami,
                r.ari,
                r.shuffled_ami,
                r.shuffled_ari,
                r.modularity,
                r.seed_ari
            );
        }
        for (name, f, _) in &held {
            println!("{name}: {:?}", validation::distribution(&slice.members, *f));
        }
        out.insert("validation".into(), serde_json::to_value(&rows).unwrap());

        let pw = validation::pairwise_same_mechanism(&both, &slice.members, 0.5);
        println!(
            "
| score | pairs | n | positives | prevalence | AUROC | AUPRC |"
        );
        for p in &pw {
            println!(
                "| {} | {} | {} | {} | {:.3} | {:.3} | {:.3} |",
                p.score, p.pairs, p.n, p.positives, p.prevalence, p.auroc, p.auprc
            );
        }
        out.insert("pairwise".into(), serde_json::to_value(&pw).unwrap());

        // resolution sweep on the default index (effects used, as served)
        let pm = cluster::PairMatrix::new(&index, &slice.members);
        let w = pm.combined(0.5);
        let n = pm.len();
        let (base, _) = cluster::partition(&w, n, 8, 1.0, 7);
        println!(
            "
| resolution | clusters | modularity | ARI vs resolution 1 |"
        );
        for res in [0.5, 1.0, 1.5] {
            let (c, q) = cluster::partition(&w, n, 8, res, 7);
            let k = c.iter().max().map_or(0, |m| m + 1);
            println!("| {res} | {k} | {q:.3} | {:.2} |", validation::ari(&base, &c));
        }

        // same-gene challenge: G2P models of one gene must not merge on gene identity alone
        println!(
            "
same-gene challenge:"
        );
        for (gene, a, b) in [
            ("KCNQ2", "MONDO:0013387", "MONDO:0007365"),
            ("SCN2A", "MONDO:0013388", "MONDO:0001071"),
        ] {
            let (Some(x), Some(y)) = (index.resolve(a), index.resolve(b)) else {
                println!("  {gene}: {a} or {b} not an active node");
                continue;
            };
            let me = index.mechanism(x, y);
            println!(
                "  {gene} {a} vs {b}: mechanism {:.3}, compatible shared genes {:?}, conflicts {:?}",
                me.score,
                me.shared_genes
                    .iter()
                    .map(|g| (&g.symbol, &g.effects_a, &g.effects_b))
                    .collect::<Vec<_>>(),
                me.effect_conflicts.iter().map(|c| &c.why).collect::<Vec<_>>()
            );
        }
        let g = index.gene_mechanisms("UNC13A");
        println!("  UNC13A: split {} -> {}", g.split, g.why.unwrap_or_default());
    }
    if let Some(path) = args.get(2) {
        std::fs::write(PathBuf::from(path), serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}
