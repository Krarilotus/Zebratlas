"""Project the three Atlas-authored TBox modules into a deterministic, public UI artifact.

No import resolution, foundation bytes, store loading, publication or inferred axioms.
Requires rdflib. Paths are explicit so sealed release candidates are never edited.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import shutil
from rdflib import Graph, URIRef, BNode, Literal
from rdflib.namespace import RDF, RDFS, OWL, SKOS

ROOT = Path(__file__).resolve().parents[2]
NS = "https://w3id.org/rare-disease-atlas/vocab#"
MODULES = {"atlas": "tbox.ttl", "bfo": "bridge-bfo.ttl", "dul": "bridge-dul.ttl"}
PREFIXES = {NS: "ra:", str(OWL): "owl:", str(RDFS): "rdfs:", "http://www.w3.org/ns/prov#": "prov:", "http://www.w3.org/2001/XMLSchema#": "xsd:", "http://purl.obolibrary.org/obo/": "obo:", "http://www.ontologydesignpatterns.org/ont/dul/DUL.owl#": "dul:"}
def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()
def compact(iri: str) -> str:
    for prefix, name in PREFIXES.items():
        if iri.startswith(prefix):
            return name + iri[len(prefix):]
    return iri
def expression(graph: Graph, value, depth=0):
    if depth > 16:
        raise ValueError("Unbounded class expression")
    if isinstance(value, URIRef):
        return {"type": "named", "id": str(value)}
    if isinstance(value, Literal):
        return {"type": "literal", "value": str(value), "datatype": str(value.datatype) if value.datatype else None, "language": value.language}
    for predicate, kind in [(OWL.intersectionOf, "intersection"), (OWL.unionOf, "union")]:
        head = graph.value(value, predicate)
        if head is not None:
            return {"type": kind, "members": [expression(graph, item, depth + 1) for item in graph.items(head)]}
    on_property = graph.value(value, OWL.onProperty)
    if on_property is not None:
        for predicate, quantifier in [(OWL.someValuesFrom, "some"), (OWL.allValuesFrom, "only"), (OWL.hasValue, "value")]:
            target = graph.value(value, predicate)
            if target is not None:
                return {"type": "restriction", "property": str(on_property), "quantifier": quantifier, "target": expression(graph, target, depth + 1)}
    raise ValueError("An unsupported blank-node expression must not be simplified into a named edge")
def terms_in(value):
    if value["type"] == "named":
        yield value["id"]
    elif value["type"] in ("union", "intersection"):
        for child in value["members"]:
            yield from terms_in(child)
    elif value["type"] == "restriction":
        yield from terms_in(value["target"])
def build(source: Path, output: Path, pin: Path):
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    validation = json.loads((source / "validation.json").read_text(encoding="utf-8"))
    graphs, modules = {}, []
    for module, filename in MODULES.items():
        path = source / filename
        expected = manifest["outputs"][filename]["sha256"]
        if digest(path) != expected:
            raise ValueError(f"Authored module checksum mismatch: {filename}")
        graphs[module] = Graph().parse(path, format="turtle")
        modules.append({"id": module, "file": filename, "sha256": expected, "bytes": path.stat().st_size})
    classes, properties, subclasses, equivalents, references, axioms = {}, {}, [], [], set(), []
    for module, graph in graphs.items():
        for subject in graph.subjects(RDF.type, OWL.Class):
            if not isinstance(subject, URIRef) or not str(subject).startswith(NS):
                continue
            iri = str(subject)
            classes.setdefault(iri, {"id": iri, "label": str(graph.value(subject, RDFS.label) or compact(iri)), "definition": str(graph.value(subject, SKOS.definition) or ""), "modules": []})["modules"].append(module)
        for subject, parent in graph.subject_objects(RDFS.subClassOf):
            if not isinstance(subject, URIRef):
                raise ValueError("Anonymous subclass subjects need an explicit rendering contract")
            value = expression(graph, parent)
            subclasses.append({"source": str(subject), "target": value, "module": module})
            references.update(terms_in(value))
        for subject, target in graph.subject_objects(OWL.equivalentClass):
            if not isinstance(subject, URIRef):
                raise ValueError("Anonymous equivalent subjects need an explicit rendering contract")
            value = expression(graph, target)
            equivalents.append({"source": str(subject), "expression": value, "module": module})
            references.update(terms_in(value))
        for predicate, kind in [(OWL.inverseOf, "inverse"), (OWL.disjointWith, "disjoint"), (RDFS.subPropertyOf, "subproperty")]:
            for subject, target in graph.subject_objects(predicate):
                if not isinstance(subject, URIRef):
                    raise ValueError("Anonymous axiom subject requires a faithful expression contract")
                value = expression(graph, target)
                axioms.append({"source": str(subject), "target": value, "kind": kind, "predicate": str(predicate), "module": module})
                references.add(str(subject))
                references.update(terms_in(value))
        subjects = set(graph.subjects(RDF.type, OWL.ObjectProperty)) | set(graph.subjects(RDF.type, OWL.DatatypeProperty))
        for subject in sorted(subjects, key=str):
            if not str(subject).startswith(NS):
                continue
            item = {"id": str(subject), "label": str(graph.value(subject, RDFS.label) or compact(str(subject))), "definition": str(graph.value(subject, SKOS.definition) or ""), "kind": "datatype" if (subject, RDF.type, OWL.DatatypeProperty) in graph else "object", "module": module,
                    "domains": [expression(graph, item) for item in sorted(graph.objects(subject, RDFS.domain), key=str)], "ranges": [expression(graph, item) for item in sorted(graph.objects(subject, RDFS.range), key=str)]}
            if not item["domains"] or not item["ranges"]:
                raise ValueError(f"Missing declared domain/range: {subject}")
            properties[str(subject)] = item
            for value in item["domains"] + item["ranges"]:
                references.update(terms_in(value))
    for item in subclasses + equivalents:
        references.add(item["source"])
    data = {"schema": "zebra.authored-tbox.v1", "version": manifest["ontology_version"], "namespace": NS, "defaultBridge": "bfo", "scope": "authored_tbox_only", "activeStoreInference": False,
            "counts": {"classes": sum("atlas" in item["modules"] for item in classes.values()), "bridgeClasses": sum("atlas" not in item["modules"] for item in classes.values()), "allNamedClasses": len(classes), "properties": len(properties), "equivalences": len(equivalents)}, "modules": modules,
            "classes": sorted(classes.values(), key=lambda item: item["id"]), "properties": sorted(properties.values(), key=lambda item: item["id"]),
            "subclasses": sorted(subclasses, key=lambda item: (item["module"], item["source"], json.dumps(item["target"], sort_keys=True))), "equivalences": sorted(equivalents, key=lambda item: (item["module"], item["source"])),
            "axioms": sorted(axioms, key=lambda item: (item["module"], item["source"], item["kind"], json.dumps(item["target"], sort_keys=True))),
            "references": [{"id": iri, "label": compact(iri)} for iri in sorted(references - set(classes))],
            "sourcePins": [{key: item.get(key) for key in ("file", "source_url", "sha256", "version", "license")} for item in manifest["sources"] if item["file"] in ("bfo-core.owl", "DUL-canonical.owl")],
            "manifestSha256": digest(source / "manifest.json"), "governanceSha256": digest(source / "GOVERNANCE.md"),
            "validation": {"sha256": digest(source / "validation.json"), "pinChecks": validation["pin_checks"], "regressions": len(validation["regressions"]), "profiles": validation["reasoner"], "limitations": validation["limitations"]}}
    if data["counts"] != {"classes": 63, "bridgeClasses": 6, "allNamedClasses": 69, "properties": 112, "equivalences": 20}:
        raise ValueError(f"The reviewed 1.0.0 authored module counts changed: {data['counts']}")
    output.mkdir(parents=True, exist_ok=True)
    payload = (json.dumps(data, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    (output / "view.json").write_bytes(payload)
    for filename in [*MODULES.values(), "GOVERNANCE.md", "validation.json", "manifest.json"]:
        shutil.copyfile(source / filename, output / filename)
    pin_data = {"schema": data["schema"], "version": data["version"], "url": f"/zebra/tbox/{data['version']}/view.json", "sha256": hashlib.sha256(payload).hexdigest(), "bytes": len(payload), "counts": data["counts"]}
    pin.parent.mkdir(parents=True, exist_ok=True)
    pin.write_text(json.dumps(pin_data, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(pin_data, indent=2))
if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--pin", type=Path, required=True)
    args = parser.parse_args()
    build(args.source_dir.resolve(), args.output_dir.resolve(), args.pin.resolve())
