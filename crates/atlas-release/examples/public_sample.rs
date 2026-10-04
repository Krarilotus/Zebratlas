//! A small, real Orphadata/HGNC sample, without cross-source disease merges.
use std::{collections::{BTreeMap, BTreeSet}, path::Path};
use atlas_core::{Atlas, DiseaseIdentity, Graph, disease::Disease, evidence::GeneLink,
    provenance::{Activity, Agent, Provenance, SourceEntity}, snapshot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 { return Err("usage: public_sample DATA OUTPUT INPUT-MANIFEST".into()); }
    let data = Path::new(&args[1]); let output = Path::new(&args[2]);
    if output.exists() { return Err("output must not exist".into()); }
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let agent = Agent { name:"zebratlas-public-sample".into(), version:"1.0.0".into(),
        commit:manifest["source_commit"].as_str().map(str::to_owned) };
    let mut provenance = Provenance::default();
    let mut entities = BTreeMap::new();
    for item in manifest["sources"].as_array().ok_or("missing sources")? {
        let file = item["file"].as_str().ok_or("missing file")?;
        if !["en_product1.xml", "en_product6.xml", "hgnc_complete_set.txt"].contains(&file) { return Err("unapproved source".into()); }
        let path = data.join("raw").join(file);
        if atlas_ingest::sources::sha256(&path)? != item["sha256"].as_str().ok_or("missing hash")? { return Err("source checksum mismatch".into()); }
        let entity = provenance.add_entity(SourceEntity {
            id:format!("source:{file}"), file:file.to_owned(),
            url:item["url"].as_str().ok_or("missing URL")?.into(),
            version:Some(format!("snapshot-sha256:{}", item["sha256"].as_str().unwrap())),
            retrieved_at:Some(item["retrieved_at"].as_str().ok_or("missing retrieval")?.into()),
            sha256:Some(item["sha256"].as_str().unwrap().into()), bytes:std::fs::metadata(path)?.len(),
            licence:Some(item["license"].as_str().ok_or("missing licence")?.into()),
        }); entities.insert(file.to_owned(),entity);
    }
    if entities.len()!=3 {return Err("exact three reviewed sources required".into());}
    let (disorders, version) = atlas_ingest::orphanet::read_disorders(&data.join("raw/en_product1.xml"),entities["en_product1.xml"])?;
    provenance.entities[entities["en_product1.xml"].0 as usize].version=version;
    let (associations,version)=atlas_ingest::orphanet::read_gene_associations(&data.join("raw/en_product6.xml"),entities["en_product6.xml"])?;
    provenance.entities[entities["en_product6.xml"].0 as usize].version=version;
    let mut hgnc=atlas_ingest::mechanism::hgnc::read(&data.join("raw/hgnc_complete_set.txt"),entities["hgnc_complete_set.txt"])?;
    let seeds:BTreeSet<_> = manifest["genes"].as_array().ok_or("missing genes")?.iter().map(|x|x.as_str().ok_or("bad gene")).collect::<Result<_,_>>()?;
    hgnc.genes.retain(|g|seeds.contains(g.symbol.as_str())); hgnc.finish();
    if hgnc.genes.len()!=seeds.len(){return Err("missing approved HGNC seed".into());}
    let withhold=atlas_ingest::withhold::load(data,atlas_ingest::withhold::salt_from_env())?;
    if withhold.closed_reason().is_some(){return Err("withholding unavailable".into());}
    for entity in &provenance.entities {
        if withhold.source_url(&entity.url).is_some(){return Err("sample source URL withheld".into());}
    }
    for gene in &hgnc.genes {
        let key=withhold.salt().key(atlas_core::withhold::KeyKind::Node,&gene.hgnc_id).ok_or("invalid gene identifier")?;
        if withhold.keys(&[key]).is_some() || withhold.record("raw/hgnc_complete_set.txt",&gene.record.locator).is_some(){
            return Err("sample HGNC seed source withheld".into());
        }
    }
    let mut activity=Activity {id:"activity:public-sample-source-projection".into(),
        label:"Selected source-asserted Orphadata disease-gene associations; no disease identity merges".into(),
        used:entities.values().copied().collect(),agent:agent.clone(),..Default::default()};
    activity.parameters.insert("selection".into(),seeds.iter().copied().collect::<Vec<_>>().join(","));
    activity.parameters.insert("identity".into(),"source identifiers retained; no merges".into());
    activity.count("read:associations",associations.len() as u64);
    let act=provenance.add_activity(activity);
    let mut diseases=BTreeMap::<String,Disease>::new();let mut excluded=BTreeMap::<String,u64>::new();
    for a in associations {
        if !seeds.contains(a.symbol.as_str()){*excluded.entry("outside_seed_scope".into()).or_default()+=1;continue;}
        let Some(g)=hgnc.get(&a.symbol) else{return Err("HGNC missing".into());};
        if a.hgnc.as_deref()!=Some(g.hgnc_id.as_str()){*excluded.entry("unverified_gene_identifier_crosswalk".into()).or_default()+=1;continue;}
        let Some(source)=disorders.get(&a.orpha) else{*excluded.entry("missing_disorder".into()).or_default()+=1;continue;};
        if source.name.starts_with("OBSOLETE:")||source.name.starts_with("MOVED TO") { *excluded.entry("retired_disorder".into()).or_default()+=1;continue; }
        let key=withhold.salt().key(atlas_core::withhold::KeyKind::Node,&a.orpha).ok_or("invalid disease key")?;
        if withhold.keys(&[key]).is_some() || withhold.record("raw/en_product6.xml",&a.record.locator).is_some()
            || withhold.record("raw/en_product1.xml",&source.record.locator).is_some() {
            *excluded.entry("withheld_source_lineage".into()).or_default()+=1;continue;
        }
        let d=diseases.entry(a.orpha.clone()).or_insert_with(||{
            let mut d=Disease::new(a.orpha.clone(),act);d.name=source.name.clone();
            d.source_ids.insert(a.orpha.clone());d.derived_from.push(source.record.clone());d
        });
        d.genes.push(GeneLink{symbol:a.symbol,association:a.association,source:"Orphanet".into(),
            source_disease:a.orpha,pmids:a.pmids,assessed:Some(a.status=="Assessed"),
            hgnc:a.hgnc,ncbi_gene:g.entrez.clone(),record:a.record});
    }
    if diseases.is_empty(){return Err("no sample assertions".into());}
    let links=diseases.values().map(|d|d.genes.len()).sum::<usize>();
    provenance.activity_mut(act).count("kept:gene_associations",links as u64);
    for (k,n) in &excluded {provenance.activity_mut(act).count(&format!("excluded:{k}"),*n);}
    let identity=DiseaseIdentity::new_gated(&[],std::iter::empty::<(&str,&[atlas_core::identity::Mapping])>(),&Default::default());
    let atlas=Atlas::new(vec![],identity,provenance.clone(),diseases.into_values().collect());
    let mut builder=atlas_ingest::graph::builder::Builder::new(&atlas,agent);
    let hgnc_entity=builder.entity(provenance.entities[entities["hgnc_complete_set.txt"].0 as usize].clone());
    builder.data.licences.push(atlas_core::graph::EntityLicence{entity:hgnc_entity,
        licence:"https://creativecommons.org/publicdomain/zero/1.0/".into(),class:atlas_core::graph::LicenceClass::Open});
    let _alias_activity=builder.start("activity:public-sample-hgnc-aliases","Approved HGNC symbols and source aliases",&[hgnc_entity]);
    let file=std::fs::read(data.join("raw/hgnc_complete_set.txt"))?;
    for g in &hgnc.genes {
        let atlas_core::provenance::Locator::Line(line)=g.record.locator else{return Err("expected HGNC line locator".into());};
        let bytes=file.split_inclusive(|b|*b==b'\n').nth(line as usize-1).ok_or("missing HGNC source line")?;
        let bytes=bytes.strip_suffix(b"\n").unwrap_or(bytes);let bytes=bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let record=builder.record(atlas_core::graph::SourceRecord{entity:hgnc_entity,
            locator:atlas_core::provenance::Locator::Line(line),id:g.hgnc_id.clone(),url:None,fetched_at:None,
            hash:atlas_core::graph::RecordHash::TsvLine,sha256:atlas_ingest::graph::cache::sha256(bytes)});
        builder.data.gene_aliases.push(atlas_core::graph::GeneAlias{hgnc:g.hgnc_id.clone(),symbol:g.symbol.clone(),name:g.name.clone(),aliases:g.aliases.clone(),previous:g.previous.clone(),record});
    }
    let association_hash=manifest["sources"].as_array().unwrap().iter().find(|s|s["file"]=="en_product6.xml").unwrap()["sha256"].as_str().unwrap();
    let rows:Vec<_>=atlas.diseases().iter().flat_map(|d|d.genes.iter().map(move|g|serde_json::json!({
        "id":atlas_core::node::edge_id(g.hgnc.as_deref().unwrap(),"gene_associated_with_condition",&d.id),
        "from":g.hgnc,"to":d.id,"association":g.association,"assessed":g.assessed,
        "source_locator":g.record.locator.to_string(),"source_sha256":association_hash,
        "source_url":"https://www.orphadata.com/data/xml/en_product6.xml"}))).collect();
    let bytes=serde_json::to_vec(&serde_json::json!({"records":rows}))?;
    let derived_hash=atlas_core::graph::hex(&atlas_ingest::graph::cache::sha256(&bytes));
    let original=builder.entity(atlas.provenance.entities[entities["en_product6.xml"].0 as usize].clone());
    let projected=builder.entity(SourceEntity{id:"source:public-associations.json".into(),file:"cache/public-associations.json".into(),
        url:"https://www.orphadata.com/data/xml/en_product6.xml".into(),version:Some(format!("derived-sha256:{derived_hash}")),
        retrieved_at:atlas.provenance.entities[entities["en_product6.xml"].0 as usize].retrieved_at.clone(),
        sha256:Some(derived_hash),bytes:bytes.len() as u64,licence:Some("https://creativecommons.org/licenses/by/4.0/".into())});
    for entity in [original,projected] {builder.data.licences.push(atlas_core::graph::EntityLicence{entity,
        licence:"https://creativecommons.org/licenses/by/4.0/".into(),class:atlas_core::graph::LicenceClass::Open});}
    let edge_activity=builder.start("activity:public-sample-association-projection","Selected Orphadata assertions projected with original source locators",&[original,projected]);
    for (index,row) in rows.iter().enumerate(){
        let record=builder.record(atlas_core::graph::SourceRecord{entity:projected,
            locator:atlas_core::provenance::Locator::Record(format!("records[{index}]")),id:row["id"].as_str().unwrap().into(),
            url:Some("https://www.orphadata.com/data/xml/en_product6.xml".into()),fetched_at:None,
            hash:atlas_core::graph::RecordHash::CanonicalJson,sha256:atlas_ingest::graph::cache::sha256(&serde_json::to_vec(row)?)});
        builder.edge(atlas_ingest::graph::builder::NewEdge{from:row["from"].as_str().unwrap(),to:row["to"].as_str().unwrap(),
            relation:atlas_core::graph::Relation::GeneAssociatedWithCondition,kind:atlas_core::node::EdgeKind::Observed,
            level:atlas_core::graph::LinkLevel::Curated,reason:row["association"].as_str().unwrap().into(),activity:edge_activity},&[record]);
    }
    let graph=Graph::new(builder.data.clone());
    let integrity=atlas_core::integrity::check(&atlas,&graph);
    if !integrity.passed{return Err(format!("sample integrity failed: {:?}",integrity.contracts).into());}
    let mechanism=atlas_core::mechanism::MechanismData::new(provenance,hgnc,Default::default(),Default::default(),vec![],vec![]);
    std::fs::create_dir(output)?;std::fs::create_dir(output.join("cache"))?;
    std::fs::write(output.join("cache/public-associations.json"),bytes)?;
    let signature=format!("public-sample-v1:{}",atlas_ingest::sources::sha256(Path::new(&args[3]))?);
    snapshot::save(&output.join("cache/atlas.snapshot"),&atlas,&signature)?;
    snapshot::save_graph(&output.join("cache/graph.snapshot"),&builder.data,&signature)?;
    atlas_ingest::mechanism::save(&output.join("cache/mechanism.snapshot"),&mechanism,&signature)?;
    let check=snapshot::load(&output.join("cache/atlas.snapshot"))?.0;
    if check.stats()!=atlas.stats(){return Err("reload changed sample stats".into());}
    for source in &atlas.provenance.entities {
        let policy=atlas_release::policy::for_entity(source);
        if !policy.copy_fields || source.licence.as_deref()!=Some(policy.license) {
            return Err("native release policy rejects sample source".into());
        }
    }
    if !graph.data().people.is_empty() || !graph.data().contacts.is_empty() || !graph.data().papers.is_empty()
        || !graph.data().studies.is_empty() || !graph.data().grants.is_empty() || !graph.data().assets.is_empty()
        || !atlas.hpo.is_empty() || atlas.diseases().iter().any(|d|!d.definition.is_empty() || !d.phenotypes.is_empty()
            || !d.excluded.is_empty() || !d.parents.is_empty() || !d.synonyms.is_empty() || !d.related.is_empty()
            || !d.prevalence.is_empty() || !d.inheritance.is_empty() || !d.onset.is_empty() || !d.clinical_course.is_empty()) {
        return Err("sparse public sample boundary failed".into());
    }
    let report=serde_json::json!({"format":"zebratlas-public-sample-v1","public_release":true,"status":"local_candidate",
        "sources":manifest["sources"],"source_commit":manifest["source_commit"],"genes":seeds,"stats":atlas.stats(),
        "gene_associations":links,"excluded":excluded,"identity_merges":0,"integrity":integrity,
        "limits":["selected genes only","no phenotypes, trials, publications, people, contacts or model assets","mechanism layer contains HGNC only","not the complete hosted app dataset"]});
    std::fs::write(output.join("sample-report.json"),serde_json::to_vec_pretty(&report)?)?;
    std::fs::write(output.join("sample-nodes.json"),serde_json::to_vec_pretty(atlas.diseases())?)?;
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"stats":atlas.stats(),"gene_associations":links,"integrity_passed":integrity.passed}))?);
    Ok(())
}
