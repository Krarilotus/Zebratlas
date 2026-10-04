//! Focus selection and composition; original graph records remain immutable.
use super::*;

/// Build a focus-specific view, preserving statements beneath presentation groupings.
/// Direct pathway membership is a co-membership lead, not a mechanism/effect compatibility claim.
pub fn build(atlas: &Atlas, graph: &Graph, pathways: &PathwayData, focus_id: &str) -> Option<UnitCollection> {
    build_with_filter(atlas, graph, pathways, focus_id, graph)
}

/// Runtime filtering can change without cloning or modifying the immutable source graph.
pub fn build_with_filter(
    atlas: &Atlas,
    graph: &Graph,
    pathways: &PathwayData,
    focus_id: &str,
    filter: &dyn RecordWithhold,
) -> Option<UnitCollection> {
    let started_at = crate::provenance::rfc3339(std::time::SystemTime::now());
    let focus = resolve_focus(atlas, graph, pathways, focus_id)?;
    if !visible(graph, filter, &focus.id) {
        return None;
    }
    let mut targets = BTreeSet::from([focus.id.clone()]);
    let mut genes = BTreeSet::new();
    if let Some(d) = atlas.disease(&focus.id) {
        if !d.is_active() {
            return None;
        }
        for g in d.genes.iter().filter(|g| g.is_causal()) {
            if let Some(n) = node(atlas, graph, &g.symbol) {
                genes.insert(n.id);
            }
        }
    } else if focus.kind == NodeKind::Gene {
        genes.insert(focus.id.clone());
    }
    let selected_pathways: BTreeSet<_> = pathways
        .annotations
        .iter()
        .filter(|a| genes.contains(&a.gene) || a.pathway.id == focus.id)
        .map(|a| a.pathway.id.clone())
        .collect();
    let annotations: Vec<_> = pathways
        .annotations
        .iter()
        .filter(|a| selected_pathways.contains(&a.pathway.id))
        .collect();
    genes.extend(annotations.iter().map(|a| a.gene.clone()));
    targets.extend(genes.iter().cloned());
    for id in &genes {
        if let Some(g) = atlas.gene(id) {
            targets.extend(atlas.gene_at(g).diseases.iter().filter_map(|&d| {
                let d = atlas.disease_at(d);
                d.genes
                    .iter()
                    .any(|link| link.is_causal() && link.symbol == atlas.gene_at(g).symbol)
                    .then(|| d.id.clone())
            }));
        }
    }
    let mut all = BTreeMap::<String, SemanticUnit>::new();
    let mut excluded = BTreeMap::<String, usize>::new();
    let mut edge_indexes = BTreeSet::new();
    for id in &targets {
        edge_indexes.extend(graph.incident(id).map(|e| e.idx));
    }
    // Holder/sponsor routes are followed only from actual reached resources.
    let reached: BTreeSet<_> = edge_indexes
        .iter()
        .flat_map(|&i| {
            let e = graph.edge(i);
            [e.from.clone(), e.to.clone()]
        })
        .collect();
    for id in reached {
        edge_indexes.extend(
            graph
                .incident(&id)
                .filter(|e| {
                    matches!(
                        e.edge.relation,
                        crate::graph::Relation::HeldBy | crate::graph::Relation::SponsoredBy
                    )
                })
                .map(|e| e.idx),
        );
    }
    for i in edge_indexes {
        let e = graph.edge(i);
        if e.records.is_empty() {
            *excluded.entry("missing_record_evidence".into()).or_default() += 1;
        } else if let Some(u) = edge_unit(atlas, graph, filter, e) {
            all.entry(u.id.clone())
                .and_modify(|old| {
                    old.evidence.extend(u.evidence.clone());
                    old.status = old.status.max(u.status);
                })
                .or_insert(u);
        } else {
            *excluded.entry("withheld_or_unresolved_edge".into()).or_default() += 1;
        }
    }
    // Disease-gene associations live outside GraphData; retain their exact atlas record pointers.
    for id in &targets {
        if let Some(d) = atlas.disease(id) {
            for link in &d.genes {
                let Some(g) = node(atlas, graph, &link.symbol) else {
                    continue;
                };
                let subject = atlas.disease_ref(atlas.disease_idx(id).unwrap());
                let key = crate::node::edge_id(&d.id, "has_associated_gene", &g.id);
                let u = all.entry(unit_id("statement", &key)).or_insert_with(|| {
                    let mut u = new_unit(
                        UnitType::Statement,
                        &key,
                        subject.clone(),
                        format!("{} · {}", subject.label, g.label),
                    );
                    u.status = AssertionStatus::Asserted;
                    u.relation = Some("has_associated_gene".into());
                    u.members = vec![subject, g];
                    u.support.push(key);
                    u
                });
                u.evidence.push(atlas_evidence(atlas, &link.record));
            }
        }
    }
    let mut by_pathway = BTreeMap::<String, Vec<String>>::new();
    for a in annotations {
        let Some(g) = node(atlas, graph, &a.gene) else {
            continue;
        };
        let key = crate::node::edge_id(&g.id, "participates_in", &a.pathway.id);
        let mut u = new_unit(
            UnitType::Statement,
            &key,
            g.clone(),
            format!("{} · {}", g.label, a.pathway.label),
        );
        u.status = if a.evidence.evidence_code.as_deref() == Some("IEA") {
            AssertionStatus::Inferred
        } else {
            AssertionStatus::Asserted
        };
        u.relation = Some("participates_in".into());
        u.members = vec![g, a.pathway.clone()];
        u.support.push(key);
        u.evidence.push(a.evidence.clone());
        by_pathway.entry(a.pathway.id.clone()).or_default().push(u.id.clone());
        all.entry(u.id.clone())
            .and_modify(|old| {
                old.evidence.extend(u.evidence.clone());
                old.status = old.status.max(u.status);
            })
            .or_insert(u);
    }
    // Items strictly group statements with the same subject; communities are application compounds.
    let statements: Vec<_> = all.values().cloned().collect();
    let mut by_member = BTreeMap::<String, Vec<usize>>::new();
    for (i, statement) in statements.iter().enumerate() {
        for member in &statement.members {
            by_member.entry(member.id.clone()).or_default().push(i);
        }
    }
    for s in &statements {
        let key = unit_id("item", &s.subject.id);
        let u = all.entry(key).or_insert_with(|| {
            new_unit(
                UnitType::Item,
                &s.subject.id,
                s.subject.clone(),
                s.subject.label.clone(),
            )
        });
        u.children.push(s.id.clone());
        u.members.extend(s.members.clone());
        u.evidence.extend(s.evidence.clone());
    }
    let mut roots = Vec::new();
    for id in targets.iter().filter(|id| atlas.disease(id).is_some()) {
        let subject = node(atlas, graph, id).unwrap();
        let mut u = new_unit(UnitType::Community, id, subject.clone(), subject.label.clone());
        let own_genes: BTreeSet<_> = atlas
            .disease(id)
            .unwrap()
            .genes
            .iter()
            .filter(|g| g.is_causal())
            .filter_map(|g| node(atlas, graph, &g.symbol).map(|n| n.id))
            .collect();
        let candidates: BTreeSet<_> = std::iter::once(id)
            .chain(own_genes.iter())
            .flat_map(|id| by_member.get(id).into_iter().flatten().copied())
            .collect();
        for i in candidates {
            let s = &statements[i];
            let touches = s.members.iter().any(|n| &n.id == id || own_genes.contains(&n.id));
            let actionable = s
                .members
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Organisation | NodeKind::Study | NodeKind::Asset));
            if touches && (actionable || s.relation.as_deref() == Some("has_associated_gene")) {
                u.children.push(s.id.clone());
                u.members.extend(s.members.clone());
                u.evidence.extend(s.evidence.clone());
            }
        }
        let resources: BTreeSet<_> = u
            .members
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Study | NodeKind::Asset))
            .map(|n| n.id.clone())
            .collect();
        let holder_statements: BTreeSet<_> = resources
            .iter()
            .flat_map(|id| by_member.get(id).into_iter().flatten().copied())
            .collect();
        for i in holder_statements {
            let s = &statements[i];
            if resources.contains(&s.subject.id) && matches!(s.relation.as_deref(), Some("held_by" | "sponsored_by")) {
                u.children.push(s.id.clone());
                u.members.extend(s.members.clone());
                u.evidence.extend(s.evidence.clone());
            }
        }
        u.members.sort_by(|a, b| a.id.cmp(&b.id));
        u.members.dedup_by(|a, b| a.id == b.id);
        let mut resource_proofs = BTreeMap::<String, BTreeSet<String>>::new();
        for child in &u.children {
            for member in &all[child].members {
                resource_proofs
                    .entry(member.id.clone())
                    .or_default()
                    .insert(child.clone());
            }
        }
        // A route belongs to its real holder, never to an unrelated study or community.
        let members = u.members.clone();
        let mut resource_ids = BTreeSet::new();
        for member in &members {
            if let Some(k) = graph.node(&member.id) {
                let role = match k.kind {
                    NodeKind::Organisation => Some(graph.org(k.idx).kind.as_str()),
                    NodeKind::Study => Some(graph.study(k.idx).kind.as_str()),
                    NodeKind::Asset => Some(graph.asset(k.idx).kind.as_str()),
                    _ => None,
                };
                if let Some(role) = role
                    && resource_ids.insert(member.id.clone())
                {
                    let support = resource_proofs.get(&member.id).into_iter().flatten().cloned().collect();
                    u.resources.push(UnitResource {
                        node: member.clone(),
                        role: role.into(),
                        support,
                    });
                }
                if k.kind == NodeKind::Organisation && visible(graph, filter, &member.id) {
                    let org = graph.org(k.idx);
                    let evidence = record_evidence(graph, &org.records, None);
                    if let Some(url) = org
                        .contact_url
                        .as_ref()
                        .or(org.url.as_ref())
                        .filter(|_| !evidence.is_empty())
                    {
                        u.contacts.push(UnitContact {
                            node: org.id.clone(),
                            kind: "official_page".into(),
                            url: url.clone(),
                            evidence,
                        });
                    }
                } else if k.kind == NodeKind::Study {
                    let evidence = record_evidence(graph, &[graph.study(k.idx).record], None);
                    if let Some(url) = evidence.first().and_then(|e| e.record_url.clone()) {
                        u.contacts.push(UnitContact {
                            node: member.id.clone(),
                            kind: "study_record".into(),
                            url,
                            evidence,
                        });
                    }
                } else if k.kind == NodeKind::Asset {
                    let asset = graph.asset(k.idx);
                    if let Some(url) = asset.access.url.as_ref().filter(|_| !asset.records.is_empty()) {
                        u.contacts.push(UnitContact {
                            node: member.id.clone(),
                            kind: asset.access.route.clone(),
                            url: url.clone(),
                            evidence: record_evidence(graph, &asset.records, None),
                        });
                    }
                    let context = asset.access_context();
                    for route in context["access_routes"].as_array().into_iter().flatten() {
                        let Some(url) = route["request_url"].as_str() else {
                            continue;
                        };
                        if asset.records.is_empty()
                            || !(url.starts_with("https://") || url.starts_with("http://"))
                            || u.contacts.iter().any(|c| c.node == member.id && c.url == url)
                        {
                            continue;
                        }
                        u.contacts.push(UnitContact {
                            node: member.id.clone(),
                            kind: route["route_type"].as_str().unwrap_or("official_page").into(),
                            url: url.into(),
                            evidence: record_evidence(graph, &asset.records, None),
                        });
                    }
                }
            }
        }
        if u.evidence.is_empty() {
            u.evidence.extend(
                atlas
                    .disease(id)
                    .unwrap()
                    .derived_from
                    .iter()
                    .map(|r| atlas_evidence(atlas, r)),
            );
        }
        u.members.push(subject);
        roots.push(u.id.clone());
        all.insert(u.id.clone(), u);
    }
    for (pathway, children) in by_pathway {
        let p = pathways
            .annotations
            .iter()
            .find(|a| a.pathway.id == pathway)
            .unwrap()
            .pathway
            .clone();
        let mut group = new_unit(UnitType::MechanismGroup, &pathway, p.clone(), p.label.clone());
        let mut per_gene = BTreeMap::<String, Vec<String>>::new();
        for child in children {
            let s = &all[&child];
            per_gene.entry(s.subject.id.clone()).or_default().push(child);
        }
        let gene_ids: Vec<_> = per_gene.keys().cloned().collect();
        if gene_ids.len() < 2 {
            continue;
        }
        // Only focus-to-neighbour connections: no quadratic clique of unrelated genes.
        for a in &gene_ids {
            let is_focus_gene = (focus.kind == NodeKind::Pathway && gene_ids.first() == Some(a))
                || (focus.kind == NodeKind::Gene && a == &focus.id)
                || atlas.disease(&focus.id).is_some_and(|d| {
                    d.genes
                        .iter()
                        .filter(|g| g.is_causal())
                        .any(|g| node(atlas, graph, &g.symbol).is_some_and(|n| &n.id == a))
                });
            if !is_focus_gene {
                continue;
            }
            for b in &gene_ids {
                if a == b {
                    continue;
                }
                let key = format!("{}|shared_pathway:{}|{}", a.min(b), pathway, a.max(b));
                let left = node(atlas, graph, a.min(b)).unwrap();
                let right = node(atlas, graph, a.max(b)).unwrap();
                let mut u = new_unit(
                    UnitType::Statement,
                    &key,
                    left.clone(),
                    format!("{} ↔ {} · {}", left.label, right.label, p.label),
                );
                u.relation = Some("shares_pathway_with".into());
                u.members = vec![left, p.clone(), right];
                u.support.extend(per_gene[a].iter().chain(&per_gene[b]).cloned());
                for s in &u.support {
                    u.evidence.extend(all[s].evidence.clone());
                }
                group.children.push(u.id.clone());
                all.insert(u.id.clone(), u);
            }
        }
        for g in gene_ids {
            let item = unit_id("item", &g);
            if let Some(u) = all.get(&item) {
                group.members.extend(u.members.clone());
                group.evidence.extend(u.evidence.clone());
            }
            group.children.push(item);
        }
        roots.push(group.id.clone());
        all.insert(group.id.clone(), group);
    }
    if roots.is_empty() {
        let item = unit_id("item", &focus.id);
        if all.contains_key(&item) {
            roots.push(item);
        } else {
            // An organisation/asset may occur only as an object (e.g. a holder).
            // Its incident statement subjects still have meaningful item roots.
            roots.extend(
                statements
                    .iter()
                    .filter(|s| s.members.iter().any(|n| n.id == focus.id))
                    .map(|s| unit_id("item", &s.subject.id)),
            );
            roots.sort();
            roots.dedup();
        }
    }
    // Include derived connections in their subject's item too (same-subject rule).
    let connections: Vec<_> = all
        .values()
        .filter(|u| u.relation.as_deref() == Some("shares_pathway_with"))
        .cloned()
        .collect();
    for s in connections {
        if let Some(item) = all.get_mut(&unit_id("item", &s.subject.id)) {
            item.children.push(s.id);
            item.members.extend(s.members);
            item.evidence.extend(s.evidence);
        }
    }
    for u in all.values_mut() {
        finalize(u);
    }
    let mut links = Vec::new();
    for community in all.values().filter(|u| u.unit_type == UnitType::Community) {
        let disease = atlas.disease(&community.subject.id).unwrap();
        for group in all.values().filter(|u| u.unit_type == UnitType::MechanismGroup) {
            let relevant: Vec<_> = disease
                .genes
                .iter()
                .filter(|g| g.is_causal())
                .filter_map(|g| node(atlas, graph, &g.symbol))
                .filter(|g| group.children.contains(&unit_id("item", &g.id)))
                .collect();
            if relevant.is_empty() {
                continue;
            }
            let mut support = Vec::new();
            for gene in relevant {
                support.push(unit_id(
                    "statement",
                    &crate::node::edge_id(&community.subject.id, "has_associated_gene", &gene.id),
                ));
                support.push(unit_id(
                    "statement",
                    &crate::node::edge_id(&gene.id, "participates_in", &group.subject.id),
                ));
            }
            support.sort();
            support.dedup();
            let mut evidence: Vec<_> = support.iter().flat_map(|id| all[id].evidence.clone()).collect();
            evidence.sort_by_cached_key(|e| serde_json::to_string(e).unwrap());
            evidence.dedup();
            let mut link = UnitLink {
                id: unit_id("link", &format!("{}|{}", community.id, group.id)),
                from: community.id.clone(),
                to: group.id.clone(),
                relation: "shares_pathway_group".into(),
                status: AssertionStatus::Inferred,
                support,
                evidence,
                generated_by: "activity:semantic-units-v1".into(),
                sha256: String::new(),
            };
            link.sha256 = digest(&serde_json::to_vec(&link).unwrap());
            links.push(link);
        }
    }
    roots.sort_by_key(|id| {
        let u = &all[id];
        (
            u.subject.id != focus.id,
            u.unit_type != UnitType::MechanismGroup,
            id.clone(),
        )
    });
    let mut overview = BTreeMap::new();
    for u in all.values() {
        let k = match u.unit_type {
            UnitType::Statement => "statements",
            UnitType::Item => "items",
            UnitType::Community => "communities",
            UnitType::MechanismGroup => "mechanism_groups",
        };
        *overview.entry(k.into()).or_default() += 1;
    }
    // Counts describe unique, visible resources, never a claim that every resource is actionable.
    let members: BTreeSet<_> = all
        .values()
        .flat_map(|u| u.members.iter().map(|n| n.id.clone()))
        .collect();
    for id in members {
        if let Some(k) = graph.node(&id) {
            let category = match k.kind {
                NodeKind::Organisation if graph.org(k.idx).kind == OrgKind::PatientGroup => "patient_groups",
                NodeKind::Study if graph.study(k.idx).kind == StudyKind::Registry => "registries",
                NodeKind::Study => "studies",
                NodeKind::Asset if graph.asset(k.idx).kind == AssetKind::Registry => "registries",
                _ => continue,
            };
            *overview.entry(category.into()).or_default() += 1;
        }
    }
    let mut activity = Activity {
        id: "activity:semantic-units-v1".into(),
        label: "Build semantic units from visible sourced statements".into(),
        started_at: Some(started_at),
        ended_at: Some(crate::provenance::rfc3339(std::time::SystemTime::now())),
        agent: Agent {
            name: "atlas-core/semantic-units".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commit: None,
        },
        ..Activity::default()
    };
    activity.parameters.insert("rule".into(), RULE.into());
    activity.parameters.insert("focus".into(), focus.id.clone());
    activity.count("kept", all.len() as u64);
    for (reason, n) in &excluded {
        activity.count(&format!("skipped:{reason}"), *n as u64);
    }
    Some(UnitCollection {
        schema: SCHEMA.into(),
        focus,
        root_summaries: roots
            .iter()
            .map(|id| {
                let u = &all[id];
                UnitSummary {
                    id: id.clone(),
                    unit_type: u.unit_type,
                    subject: u.subject.clone(),
                    status: u.status,
                    resources: u.resources.len(),
                    contacts: u.contacts.len(),
                }
            })
            .collect(),
        roots,
        links,
        units: all.into_values().collect(),
        overview,
        activity,
        pathway_activity: pathways.activity.clone(),
        pathways_available: pathways.available,
        excluded,
    })
}
