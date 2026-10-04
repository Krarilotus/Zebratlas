# Datasheet: Rare Disease Atlas licence-aware knowledge graph

## Motivation

Connect rare-disease communities with sourced research, studies, organisations and
shared assets. This local release candidate supports reproducibility and source
inspection (D10/D15/D21/D35/D36). It is research/navigation infrastructure; it does not
diagnose, recommend treatment or establish clinical efficacy. No public deposit or
DOI is claimed. The founder approves publishing separately.

## Composition

The bundle projects the atlas disease/gene/phenotype store and the connected graph
of studies, papers, grants, organisations and research assets (models, samples,
programmes, outcomes and funding). Person nodes, identifiers and every
edge incident to a person are excluded from the bulk release under D36 section 2. `release-report.json` contains
measured node/edge counts, source memberships, bytes, withheld counts and mapping
counts. `integrity.json` uses the exact core check behind `/api/integrity`.
Retired and newly-described disease identifiers remain; they are classified rather
than dropped. Explicitly absent phenotype edge identifiers are kept separate from
present phenotype identifiers. Restricted edge identifiers are retained as
`link_only` records and are not asserted in RDF. Gene edges and phenotypes outside
the API's active established-disease scope are reconciled explicitly.

## Collection process

Source files and offline cache envelopes were collected by project fetchers; this
application makes no HTTP requests and never changes shared sources or caches.
It reuses compatible, fresh bincode snapshots or builds in memory with atlas-ingest.
Each exported record has source URL, retrieval metadata, version or an explicit
snapshot hash identity, sha256 and exact record locator. Raw-file retrieval dates
may be filesystem modification-time proxies; this limitation is preserved in
`retrieval_basis`, never represented as a verified download event. Source versions
are not invented when absent. Graph record hashes use canonical JSON, JSON-line or
TSV algorithms; raw evidence uses source-file byte hashes plus exact locators.

## Preprocessing and provenance

Identity resolution and graph generation use the current master's atlas-core/atlas-ingest
rules. SSSOM includes authoritative disease-identity and reviewed cache projections;
candidates, granularity, typed gene-product links and conflict flags never become
exact identity assertions. Mapping labels, author columns and free-form comments
are withheld; comments become exact provenance. Original mapping hashes and row
locators preserve the projection chain. RDF 1.2 reifiers identify each licensed
asserted edge. PROV-O
source entities, record entities, generating activities, qualified usage/derivation
and software agents retain transformation lineage. The release applies an explicit
field whitelist and conservative licence filter. Arbitrary activity parameter
values are withheld and hashed to prevent secret or restricted-text disclosure;
activities retain eligible input lineage, software version, parameter hash and a
counts hash. Person-cache sources and activities are omitted.
Full upstream struct dumps are never part of the release.

## Licences and attribution

Our derived contribution is CC BY 4.0. `LICENSE`, `LICENCES.md` and
`source-policies.json` carry third-party notices. Verified CC0/CC BY inputs allow
selected fields. Custom, SA, NC, ND, mixed or unknown terms permit only identifiers
and links under D35. No licence is inferred from an upstream host or an “open data”
label. A free-to-read paper URL does not grant a licence to redistribute its text.

## Recommended uses

Reproduce graph counts, inspect lineage, follow public source links, compare
identifier alignments and build non-clinical research navigation. Cite this
dataset's version and attributed inputs. Do not treat link-only edge identifiers
as asserted triples or decode them to bypass the release's permission policy.

## Limitations and exclusions

The connected layer is fetched for a ten-gene DEE slice and reflects source search
and curation coverage, not worldwide completeness. Registry status can be stale.
Inferred text/identity links are not proof of association. HPO annotations and
paper prose are withheld here. No accounts, contributions, private or personal
contact fields, abstracts, page snapshots, clinical phenopackets or secrets are
included. No researcher names, personal identifiers or profile links are released. Trial
contacts, grant investigators, organisation staff and work authors are omitted by
the field whitelist; the collective dataset attribution is retained in CITATION.cff.
The exporter uses current graph readers, including models, programmes, outcomes,
funding/international research and KGX. Research caches with unresolved permissions
are pointers only. KGX retains the reader's two-hop neighbourhood scope; this is
not the full union of every external KG. Separate auxiliary ClinVar, repurposing,
analogue and OA enrichments absent from the graph readers are not silently added.
Lack of an exported edge is not evidence
that no association exists. The data supports no clinical accuracy or speedup claim.

## Acquisition quarantine

The exporter reads `data/cache/quarantine.json` before loading snapshots. A missing
file means no quarantine; unreadable or malformed manifests fail the release.
Every listed cache-file/record-locator pair excludes the whole derived node, edge
or mapping, including mixed-source records. JSON Pointer `/records/N` and ingest
`records[N]` locators match; nested record locators exclude their containing record.
Listed upstream page URLs also exclude derived references conservatively when old
snapshots omit intermediate page lineage. Edges to excluded nodes are removed even
in link-only form. No excluded record IDs, URLs or tombstones are copied into the
bundle; originals remain untouched in the restricted shared caches. Aggregate
exclusions and the quarantine manifest's byte hash are in `release-report.json`.
Exported counts plus explicit exclusion counts reconcile with upstream integrity.

The current manifest's hash, counts and exclusions are measured in
`release-report.json`; `docs/release/REPORT.md` records the validated snapshot.
Shared manifests may change during a build; changed hashes stop finalization
and publication. Missing retrieval metadata is a counted exclusion, never an
invented acquisition timestamp. Original records remain in restricted inputs.
Quarantine does not clear licences or certify that every acquisition was reviewed.

## Suppression (D43)

`data/suppression.json` is read before snapshots and again at projection. Missing
means no entries; unreadable, malformed or unsupported entries fail closed.
The shared `atlas_core::withhold` / `atlas_ingest::withhold` mechanism owns the
schema and matching for both lists. Suppression is versioned `schema: suppression`
with salted person/node keys and the matching salt ID; quarantine supports record
and whole-file selectors. Unknown versions, malformed data and foreign salts stop
publication. The Python gates consume an index parsed by that same Rust filter.
Every person profile/contact field and incident edge is excluded independently;
this includes researcher mapping sets with `release: false`. No suppression key,
reviewer identity or raw manifest is included in the public payload.
Only manifest hashes and aggregate exclusion counts enter the release.

## Mapping policy

Reviewed sets: MONDO and Orphadata xrefs (CC BY), HGNC/NCBI gene and UniChem drug
IDs (CC0), Cellosaurus identifier links (CC BY), and only ROR-origin funder-ror
rows (CC0). GARD, HPO, G2P and ClinGen mapping content remains held. Trial,
mixed-directory, mixed-work and Alliance sets remain held by the crosscheck even
when cache headers claim open terms. `release: false` and person mappings never
enter. Per-set policy, counts, exclusions and bytes are in `release-report.json`.

## Distribution and maintenance

The versioned directory is immutable: exporting to an existing directory fails.
`SHA256SUMS` covers every payload file, including documentation and reports; it
excludes itself. Run the Rust guardrail tests and Python RDF/SSSOM/count/checksum
validator before deposit. For live count reconciliation, supply a same-snapshot
`/api/integrity` URL. Re-audit licence terms and source versions for each release.
Maintain backwards-compatible IDs where the upstream identifier permits it.
Contact/operator details, real author metadata, persistent public repository URL
and DOI are deliberately not fabricated; the founder supplies them for deposit.
