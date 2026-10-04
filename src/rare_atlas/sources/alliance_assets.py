"""Stream Alliance disease models and human orthologs; never merge ortholog identities."""

import argparse
import csv
import gzip
import html

from rare_atlas.sources.assets_io import KGX, Acquisition, sha_file

BASE = "https://download.alliancegenome.org/9.1.0/downloads/"


def curie(value):
    prefix, sep, local = value.partition(":")
    return {"FB": "FlyBase", "WB": "WormBase"}.get(prefix, prefix) + sep + local


def tsv(path):
    with gzip.open(path, "rt", encoding="utf-8", newline="") as stream:
        header = None
        for line_number, line in enumerate(stream, 1):
            if line.startswith("#") or not line.strip():
                continue
            values = next(csv.reader([line], delimiter="\t"))
            if header is None:
                header = values
            else:
                if len(values) != len(header):
                    raise ValueError(f"invalid TSV width at L{line_number}")
                yield line_number, dict(zip(header, values, strict=True))


def disease(row, writer, entity, locator):
    model, condition = row.get("Model ID"), row.get("Disease ID")
    relation = row.get("Model Association")
    if not model or not condition or relation not in ("is_model_of", "is_not_model_of"):
        writer.exclude(
            "no explicit experimental disease-model association",
            entity,
            locator,
            row.get("UniqueID"),
            model_type=row.get("Model Type"),
        )
        return
    p = writer.prov(entity, locator)
    if row.get("Annotation Type") != "manually_curated":
        p.update(knowledge_level="prediction", agent_type="computational_model")
    model = curie(model)
    source_url = row.get("Source URL")
    # This is the evidence catalogue, not a claim about material custody.
    # Requesters follow the source record to stocks/author/institutional routes.
    writer.node(
        model,
        html.unescape(row.get("Model Symbol", "")),
        "Genotype",
        p,
        asset_type="disease_model",
        taxon=row.get("Taxon ID"),
        model_type=row.get("Model Type"),
        source_record_url=source_url,
        request_url=source_url,
        request_route_type="model_record_then_stock_or_institution",
        holder=None,
        holder_status="not_reported",
        actionable_access_handoff=False,
        catalog=row.get("Source"),
        availability="unknown",
        scientific_fit="not_assessed",
        permitted_use="not_assessed",
    )
    writer.node(condition, row.get("Disease Name"), "Disease", p)
    writer.edge(
        model,
        "model_of",
        condition,
        p,
        negated=relation == "is_not_model_of" or row.get("Disease Qualifier") in ("NOT", "not"),
        upstream_relation=relation,
        disease_qualifier=row.get("Disease Qualifier"),
        evidence_code=row.get("Evidence Code"),
        publications=row.get("Reference", "").split("|"),
        experimental_conditions=row.get("Experimental Conditions"),
        condition_modifiers=row.get("Condition Modifiers"),
        notes=row.get("Notes"),
        genetic_sex=row.get("Genetic Sex"),
        annotation_type=row.get("Annotation Type"),
        based_on_id=row.get("Based On ID"),
        upstream_record_id=row.get("UniqueID"),
        allele_ids=[curie(x) for x in row.get("Allele IDs", "").split("|") if x],
        strain_background_id=row.get("Strain Background ID"),
    )
    genes = row.get("Gene IDs", "").split("|")
    symbols = row.get("Gene Symbols", "").split("|")
    for i, gene in enumerate(genes):
        if not gene:
            continue
        gene = curie(gene)
        writer.node(gene, symbols[i] if i < len(symbols) else gene, "Gene", p)
        writer.edge(
            model,
            "related_to",
            gene,
            p,
            upstream_relation=row.get("Gene Association"),
            association_basis="source model-associated gene; no human-variant effect inferred",
        )
    writer.counts["included_model_annotations"] += 1


def orthology(row, writer, entity, locator, mapping=None):
    ids = [row.get("Gene1ID"), row.get("Gene2ID")]
    taxa = [row.get("Gene1SpeciesTaxonID"), row.get("Gene2SpeciesTaxonID")]
    if "NCBITaxon:9606" not in taxa or taxa[0] == taxa[1]:
        writer.exclude("orthology outside human-to-nonhuman scope", entity, locator, "|".join(str(x) for x in ids))
        writer.counts["orthology_outside_human_scope"] += 1
        return
    p = {**writer.prov(entity, locator), "knowledge_level": "prediction", "agent_type": "computational_model"}
    for i in (0, 1):
        writer.node(curie(ids[i]), row.get(f"Gene{i + 1}Symbol"), "Gene", p, taxon=taxa[i])
    writer.edge(
        curie(ids[0]),
        "orthologous_to",
        curie(ids[1]),
        p,
        algorithms=row.get("Algorithms", "").split("|"),
        algorithms_match=row.get("AlgorithmsMatch"),
        out_of_algorithms=row.get("OutOfAlgorithms"),
        best_score=row.get("IsBestScore"),
        best_reverse_score=row.get("IsBestRevScore"),
        identity_merge=False,
    )
    if mapping:
        values = [
            curie(ids[0]),
            "RO:HOM0000017",
            curie(ids[1]),
            "semapv:UnspecifiedMatching",
            "rare-atlas-assets/1.0.0",
            entity["url"],
            entity["version"],
            entity["retrieved_at"],
            entity["sha256"],
            locator,
            "CC-BY-4.0",
            "open",
        ]
        mapping.write("\t".join(values) + "\n")
    writer.counts["human_orthology_annotations"] += 1


def run(offline=False):
    from rare_atlas.paths import CACHE

    acquisition = Acquisition(offline)
    writer = KGX(
        "alliance",
        {
            "disease_scope": "all explicit disease models; live scope applied by graph owner",
            "orthology_scope": "human to nonhuman; never exactMatch",
            "identifier_normalization": {"FB": "FlyBase", "WB": "WormBase"},
            "model_label_transformation": "HTML entities decoded; original retained in source record",
        },
    )
    writer.license_verification = acquisition.verify_license("alliance")
    path, entity = acquisition.get("alliance", BASE + "DISEASE-ALLIANCE_TSV_COMBINED.tsv.gz", "9.1.0")
    for line, row in tsv(path):
        disease(row, writer, entity, f"L{line};UniqueID={row['UniqueID']}")
    mapping_root = CACHE / "mappings"
    mapping_root.mkdir(parents=True, exist_ok=True)
    mapping_path = writer.root / "alliance-human-orthologs.sssom.tsv"
    path, entity = acquisition.get("alliance", BASE + "ORTHOLOGY-ALLIANCE_TSV_COMBINED.tsv.gz", "9.1.0")
    with mapping_path.open("w", encoding="utf-8", newline="\n") as out:
        out.write(
            "# mapping_set_id: https://w3id.org/rare-atlas/mappings/alliance-human-orthologs\n"
            "# license: https://creativecommons.org/licenses/by/4.0/\n"
            "# mapping_set_description: Human/nonhuman orthology predictions; NEVER identity mappings.\n"
            "# curie_map:\n#   HGNC: https://identifiers.org/hgnc:\n"
            "#   MGI: https://identifiers.org/mgi:\n#   RGD: https://identifiers.org/rgd:\n"
            "#   ZFIN: https://identifiers.org/zfin:\n#   FlyBase: https://identifiers.org/flybase:\n"
            "#   WormBase: https://identifiers.org/wormbase:\n#   SGD: https://identifiers.org/sgd:\n"
            "#   Xenbase: https://identifiers.org/xenbase:\n#   RO: http://purl.obolibrary.org/obo/RO_\n"
            "#   semapv: https://w3id.org/semapv/vocab/\n"
        )
        out.write(
            "subject_id\tpredicate_id\tobject_id\tmapping_justification\tmapping_tool\t"
            "prov_source_url\tprov_version\tprov_retrieved_at\tprov_sha256\tprov_locator\tlicense\tlicense_class\n"
        )
        for line, row in tsv(path):
            orthology(row, writer, entity, f"L{line}", out)
    import shutil

    shutil.copyfile(mapping_path, mapping_root / (mapping_path.name + ".part"))
    (mapping_root / (mapping_path.name + ".part")).replace(mapping_root / mapping_path.name)
    acquisition.close()
    return writer.finish(
        mapping_set={
            "path": str(mapping_root / mapping_path.name),
            "sha256": sha_file(mapping_path),
            "records": writer.counts["human_orthology_annotations"],
            "identity_mapping": False,
        }
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    run(parser.parse_args().offline)
