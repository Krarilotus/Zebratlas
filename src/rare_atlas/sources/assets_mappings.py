"""Source-asserted cell-line IDs and canonical RRIDs, without label matching."""

import argparse
import json

from rare_atlas.paths import CACHE
from rare_atlas.sources.assets_io import Acquisition, canonical, sha_file, software_manifest


def run(offline=False):
    acquisition = Acquisition(offline)
    # The official Cellosaurus convention uses RRID:CVCL_xxxx for these accessions.
    path, entity = acquisition.get("cellosaurus-terms", "https://www.cellosaurus.org/overview_rii.html")
    text = path.read_text(encoding="utf-8")
    if "RRID:CVCL_" not in text:
        raise ValueError("Cellosaurus RRID convention cannot be verified")
    destination = CACHE / "mappings" / "assets-cell-identifiers.sssom.tsv"
    destination.parent.mkdir(parents=True, exist_ok=True)
    output = destination.with_suffix(".part")
    columns = [
        "subject_id",
        "predicate_id",
        "object_id",
        "mapping_justification",
        "mapping_tool",
        "prov_source_url",
        "prov_version",
        "prov_retrieved_at",
        "prov_sha256",
        "prov_locator",
        "license",
        "license_class",
        "comment",
    ]
    seen = set()
    entities = {}
    rows = 0
    with output.open("w", encoding="utf-8", newline="\n") as out:
        out.write(
            "# mapping_set_id: https://w3id.org/rare-atlas/mappings/assets-cell-identifiers\n"
            "# mapping_set_description: Same-line direct hPSCreg xrefs and official Cellosaurus RRID convention.\n"
            "# license: https://creativecommons.org/licenses/by/4.0/\n"
            "# curie_map:\n#   CVCL: https://www.cellosaurus.org/CVCL_\n"
            "#   RRID: https://scicrunch.org/resolver/\n#   hpscreg: https://hpscreg.eu/cell-line/\n"
            "#   skos: http://www.w3.org/2004/02/skos/core#\n"
            "#   semapv: https://w3id.org/semapv/vocab/\n"
        )
        out.write("\t".join(columns) + "\n")
        for index in sorted(acquisition.index.glob("*.json")):
            source = json.loads(index.read_text(encoding="utf-8"))
            if not source["url"].startswith("https://api.cellosaurus.org/search/cell-line?"):
                continue
            from rare_atlas.paths import RAW

            raw = RAW / source["path"]
            if sha_file(raw) != source["sha256"]:
                raise ValueError("cell identifier source checksum mismatch")
            entities[source["id"]] = source
            for i, cell in enumerate(json.loads(raw.read_text(encoding="utf-8"))["Cellosaurus"]["cell-line-list"]):
                ac = next(x["value"] for x in cell["accession-list"] if x["type"] == "primary")
                subject = "CVCL:" + ac.removeprefix("CVCL_")
                targets = [("RRID:" + ac, "official Cellosaurus RRID naming convention")]
                targets += [
                    ("hpscreg:" + x["accession"], "direct same-line hPSCreg cross-reference")
                    for x in cell.get("xref-list", [])
                    if x.get("database") == "hPSCreg"
                ]
                for target, comment in targets:
                    if (subject, target) in seen:
                        continue
                    seen.add((subject, target))
                    values = [
                        subject,
                        "skos:exactMatch",
                        target,
                        "semapv:ManualMappingCuration",
                        "rare-atlas-assets/1.0.0",
                        source["url"],
                        source["version"],
                        source["retrieved_at"],
                        source["sha256"],
                        f"/Cellosaurus/cell-line-list/{i}",
                        "CC-BY-4.0",
                        "open",
                        comment,
                    ]
                    out.write("\t".join(values) + "\n")
                    rows += 1
    output.replace(destination)
    manifest = {
        "mapping_set": destination.name,
        "sha256": sha_file(destination),
        "rows": rows,
        "source_entities": list(entities.values()),
        "naming_convention_entity": entity,
        "identity_policy": "source asserted IDs only; no label matching",
        "transformation": {
            "type": "prov:Activity",
            "prov:used": [*entities, entity["id"]],
            "prov:wasAssociatedWith": {
                "type": "prov:SoftwareAgent",
                "name": "rare-atlas-assets/1.0.0",
                "software_sha256": software_manifest(),
            },
            "parameters": {
                "cell_rrid_convention": "prefix upstream accession with RRID:",
                "hpscreg": "direct same-line source xref only",
            },
        },
    }
    destination.with_suffix(".manifest.json").write_text(canonical(manifest) + "\n", encoding="utf-8")
    acquisition.close()
    print(canonical({"mapping_set": str(destination), "rows": rows}), flush=True)
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    run(parser.parse_args().offline)
