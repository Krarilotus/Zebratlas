# Source licences and release permissions (D35)

Audit date: **2026-10-04 (Europe/Berlin)**. Scope: every source family named in
`docs/design/SOURCES.md`, its linked source contracts, and the shared `data/raw`
inventory. This is a conservative release policy, not a claim that all publicly
accessible data is open. `link only` means identifiers, public source URLs,
checksums, versions, retrieval metadata and record locators; no labels, record
fields, quoted prose or asserted source relationships. Unknown permissions default
to this mode. No page snapshots, abstracts, account data, contribution data,
personal contact fields or clinical case records are released, even when a source
would permit them. The licence on **our contribution** does not relicense inputs.

Each terms link below was read or attempted on the audit date. **Verified** means
the primary page supplied the stated terms. **Unverified** means unavailable,
JavaScript-only, or insufficient to establish record-level copying rights. Earlier
fetcher observations are identified as such rather than presented as this audit's
verification. Policy is compiled in `crates/atlas-release/src/policy.rs`: only
verified CC0/CC BY sources can carry whitelisted fields. The provenance registry's
self-reported licence strings are not permission grants.

Permissions notation: **F** = facts/structured fields; **I** = identifiers + links;
**Q** = short quotes; **R** = full records. “Allowed” describes verified source
permissions; the exporter still uses a smaller whitelist and never copies Q/R.
SA = ShareAlike, NC = non-commercial, ND = no derivatives/integrity restrictions.

| Source / local scope | Primary licence/terms (checked 2026-10-04) | Allowed / release policy | SA / NC / ND | Attribution text / caveat |
|---|---|---|---|---|
| MONDO, `mondo.obo` | [Download/licence](https://mondo.monarchinitiative.org/pages/download/), **verified CC BY 4.0** | F/I/Q/R permitted with attribution; selected fields only | no/no/no | Mondo Disease Ontology, Monarch Initiative; source version; CC BY 4.0; identifier-based derived projection; changes indicated |
| Orphadata Science, `en_product1.xml`, `en_product6.xml`, `en_product9_prev.xml`, `en_product9_ages.xml` | [FAQ](https://sciences.orphadata.com/faq/), **verified CC BY 4.0** for Science datasets | F/I/Q/R permitted; selected fields only | no/no/no | Orphadata Science: Free access data from Orphanet. © INSERM 1999. https://sciences.orphadata.com/. Data version: manifest. Derived projection; no endorsement. Orphanet/Orphadata data was used in development; outputs may not reflect it faithfully and Orphanet is not responsible for their quality |
| Orphanet website/professional directories | [About resources](https://www.orphadata.com/about-us/), verified distinction between Science and Products | **link only**; Science permission does not cover every web page or Product | unknown | Orphanet, exact page URL; no website prose copied |
| HPO ontology and annotations: `hp.obo`, `phenotype.hpoa`, `genes_to_disease.txt`, `genes_to_phenotype.txt`; historical `phenopackets/phenotype_2024-01-16.hpoa` | [HPO licence](https://human-phenotype-ontology.github.io/license.html), **verified custom integrity terms**; current applicability of historical page not established | I; **link only** under D35; vocabulary/annotation contents withheld | no/no/integrity | This service uses Human Phenotype Ontology Consortium resources; versions retained; no vocabulary or annotation text copied |
| HGNC, `hgnc_complete_set.txt`, HGNC REST in models | [HGNC licence](https://www.genenames.org/about/license/), **verified CC0** | F/I/Q/R permitted; symbol/identifier projection | no/no/no | HGNC (RRID:SCR_002827), Genenames.org; source version; attribution recommended |
| Gene Ontology, `go-basic.obo` | [GO policy](https://geneontology.org/docs/go-citation-policy/), **verified CC BY 4.0** | F/I/Q/R permitted; not loaded by this exporter | no/no/no | Gene Ontology Consortium; exact release/version; CC BY 4.0; modifications indicated; as-is/no warranties |
| GOA human annotations, `goa_human.gaf.gz` | [GO policy](https://geneontology.org/docs/go-citation-policy/) + [EMBL-EBI terms](https://www.ebi.ac.uk/about/terms-of-use/); contributor-specific scope **unverified** | **link only** pending annotation-provider review | unknown | GO Consortium and EMBL-EBI GOA; contributing source must be attributed |
| Reactome, `NCBI2Reactome.txt`, `NCBI2Reactome_All_Levels.txt`, `ReactomePathways.txt`, `ReactomePathwaysRelation.txt` | [Reactome agreement](https://reactome.org/license), **verified CC0 for database data** (art CC BY; code separate) | F/I/Q/R data permitted; not loaded by this exporter | no/no/no | Reactome pathway database; version; no logos/art/code included |
| Gene2Phenotype, `allG2P_2026-09-28.csv.gz` and `.md5` | [Service](https://www.ebi.ac.uk/gene2phenotype/) JS-only; [EBI terms](https://www.ebi.ac.uk/about/terms-of-use/) verified pass-through, resource-specific licence **unverified** | **link only**; `.md5` is a checksum sidecar, not another content grant | unknown | Gene2Phenotype, EMBL-EBI; version; “open data” is not treated as CC0 |
| ClinGen dosage, `ClinGen_gene_curation_list_GRCh38.tsv` | [Terms](https://www.clinicalgenome.org/docs/terms-of-use/), record-level scope **unverified** | **link only** | unknown | ClinGen Clinical Genome Resource; exact record URL |
| Phenopacket Store, `phenopackets/all_phenopackets.zip` | [Source licence](https://github.com/monarch-initiative/phenopacket-store/blob/main/LICENSE), **verified BSD-3-Clause repository licence**; publication-derived case rights are not assumed | **not exported**: case/patient records excluded by scope regardless of licence | no/no/no for repository | Phenopacket Store and original case publications; retain BSD notice for any separately permitted reuse; benchmark remains local |
| ClinicalTrials.gov, `trials/`, `contacts/ctgov.json` | [Terms](https://clinicaltrials.gov/about-site/terms-conditions), JS-only; [NCBI policy](https://www.ncbi.nlm.nih.gov/home/about/policies/) warns about third-party rights | **link only** until current registry copying terms verified; contacts excluded | unknown | ClinicalTrials.gov, US National Library of Medicine; access date and source URL |
| NIH RePORTER, `reporter/` | [FAQ](https://reporter.nih.gov/faq), unavailable; record-level prose rights **unverified** | **link only**; no abstracts or PI contact data | unknown | NIH RePORTER; exact project page and version/retrieval metadata |
| PubMed, `pubmed/` | [NCBI policy](https://www.ncbi.nlm.nih.gov/home/about/policies/), **verified third-party copyright caveat** | I; **link only**, no paper titles, abstracts or authors | varies | PubMed, US National Library of Medicine; PMID and publication URL; publisher rights unchanged |
| Europe PMC, `research_intl/europepmc*`, OA records | [Copyright](https://europepmc.org/Copyright), unavailable; [EBI terms](https://www.ebi.ac.uk/about/terms-of-use/) verified pass-through | **link only**; free-to-read status grants no blanket copying right | varies | Europe PMC; article ID and legal public link; publisher/article licence governs text |
| WHO ICTRP, `registries/ictrp.json` | [Downloading terms](https://www.who.int/tools/clinical-trials-registry-platform/network/who-data-set/downloading-records-from-the-ictrp-database), **verified no commercial/promotional use** | **link only** under D35 | no/yes/unknown | WHO ICTRP **and original registry**; no WHO emblem/name implying endorsement |
| EU CTR, `registries/euctr.json` | [Legal notice](https://www.clinicaltrialsregister.eu/disclaimer.html), verified attribution/access-date requirement; sponsor-content rights **unverified** | **link only** | unknown | EU Clinical Trials Register; access date; sponsor-submitted data caveat |
| CTIS, `registries/ctis.json` | [Public portal](https://euclinicaltrials.eu/), record-level reuse terms **unverified** | **link only** | unknown | European Medicines Agency CTIS; exact trial link |
| Wikidata, `labels/wikidata.json`, xref mappings | [Licensing](https://www.wikidata.org/wiki/Wikidata:Licensing), **verified CC0 structured data**, other text distinct | F/I/Q/R structured data permitted; ID projection | no/no/no | Wikidata; source revision links recommended; aliases are candidates, never identity merges |
| Google/Bright Data SERP and unlocker, `orgs/serp/`, requests | [Bright Data terms](https://brightdata.com/terms-of-service), **unverified**; transport does not grant target-content rights | **not exported**; search snippets/snapshots/requests excluded | varies | Original destination URLs only; no API secrets or search-result text |
| Official patient-group pages, `orgs/candidates`, `organisations`, `pages` | Each record's official evidence page, individual site terms **unverified** | **link only**, no cover quotes, descriptions, scraped labels or contact fields | varies | Official organisation source URL; our curation does not remove site restrictions |
| NORD directories/pages | [Terms](https://rarediseases.org/terms-conditions/), **verified permission restrictions** | **link only** | restrictive | National Organization for Rare Disorders; prior permission required for copied material |
| Global Genes directories/pages | [Terms](https://globalgenes.org/terms-of-service/), unavailable/**unverified** | **link only** | unknown | Global Genes; public official page URL |
| EURORDIS, `directories/alliances` | [Terms](https://www.eurordis.org/terms-conditions/), 429 in this audit; fetcher previously recorded non-commercial restriction | **link only**; earlier restriction not relaxed | unknown/previously observed/unknown | EURORDIS; exact member page URL; permissions required for commercial republication |
| ACHSE | [Members](https://www.achse-online.de/ueber-uns/mitgliedsorganisationen), terms **unverified** | **link only** | unknown | ACHSE and member's official page |
| UNIAMO | [Members](https://uniamo.org/associazioni-federate/), terms **unverified** | **link only** | unknown | UNIAMO and member's official page |
| FEDER | [Members](https://www.enfermedades-raras.org/movimiento-asociativo/entidades-asociadas), terms **unverified** | **link only** | unknown | FEDER and member's official page |
| CORD | [Members](https://www.raredisorders.ca/membership/affiliate-member-list), terms **unverified** | **link only** | unknown | Canadian Organization for Rare Disorders and official member page |
| Genetic Alliance UK, Alliance Maladies Rares, Rare Voices Australia | [GA UK](https://geneticalliance.org.uk/membership/a-z-members-directory/) reuse terms unverified; [AMR](https://alliance-maladies-rares.org/mentions-legales/) **verified prior-permission requirements** for copying and hyperlinks; [RVA](https://rarevoices.org.au/terms-of-use/) **verified permission/commercial-use restrictions** | **link only** under D35; these auxiliary inputs remain outside the exporter; AMR hyperlink permission needs review before any deposit containing its links | restrictive/restrictive/restrictive | Respective official organisation; no content copied |
| ERN EpiCARE, `directories/ern_epicare` | [Official site](https://epi-care.eu/), terms **unverified** | **link only** | unknown | ERN EpiCARE; exact centre/member URL; heuristic links not authoritative identity |
| ERDRI, `directories/erdri` | [JRC platform](https://eu-rd-platform.jrc.ec.europa.eu/erdri), unavailable/**unverified** | **link only** | unknown | European Commission JRC, ERDRI; registry source URL |
| CORDIS, `research_intl/cordis` | [Legal notice](https://cordis.europa.eu/about/legal), **verified CC BY 4.0 EU editorial content**, beneficiary/third-party carve-outs | **link only** for cached project records until ownership established | no/no/no for EU-owned content | CORDIS, European Union; access date; modifications; beneficiary rights retained |
| KAKEN, `research_intl/kaken` | [Help](https://kaken.nii.ac.jp/en/help/), unavailable/**unverified** | **link only** | unknown | KAKEN, National Institute of Informatics; grant/researcher IDs and source page |
| UKRI GtR, `research_intl/gtr` | [Terms](https://gtr.ukri.org/resources/terms), unavailable/**unverified** | **link only** | unknown | UKRI Gateway to Research; grant reference and project URL |
| DFG GEPRIS, `research_intl/gepris` | [Official site](https://gepris.dfg.de/), permissions **unverified**, fetcher records robots exclusion | **link only**; no downloaded project records | unknown | Deutsche Forschungsgemeinschaft GEPRIS; unfetched project links marked as such |
| ClinVar, `clinvar/` | [NCBI policy](https://www.ncbi.nlm.nih.gov/home/about/policies/), verified no NCBI molecular-data distribution restriction; submitter text scope **unverified** | **link only** in this conservative release; not in core snapshot | varies | ClinVar, NCBI and submitters; VCV accession/version |
| Alliance/MGI/JAX, `models/` orthology/alleles | [Alliance terms](https://www.alliancegenome.org/terms-of-use), unavailable/**unverified**; upstream MGI/JAX terms not established | **link only** | unknown | Alliance of Genome Resources and contributing model organism database; source IDs |
| IMPC, `models/` phenotype measurements | [Help](https://www.mousephenotype.org/help/), **verified CC BY 4.0 dataset** | F/I/Q/R permitted with attribution; exporter does not load auxiliary models cache | no/no/no | International Mouse Phenotyping Consortium; dataset release/version; measured phenotypes are not validated biomarkers |
| Cellosaurus, `models/` iPSC assets | [FAQ Q22](https://www.cellosaurus.org/faq), **verified CC BY 4.0** | F/I/Q/R permitted with attribution; exporter does not load auxiliary cache | no/no/no | Cellosaurus, SIB CALIPHO; release/version; modifications |
| hPSCreg links | [Official site](https://hpscreg.eu/), direct record terms **unverified** | **link only** | unknown | hPSCreg; source identifier/link, no donor data |
| Every Cure disease-list, drug-list, indications-list | [Publication terms](https://docs.dev.everycure.org/releases/public_data_releases/); immutable [disease card](https://huggingface.co/datasets/everycure/disease-list/blob/c302a048c4018c94c855635a14849ca4a848e95f/README.md), [drug card](https://huggingface.co/datasets/everycure/drug-list/blob/25640b38181d9953566f21aa7d363b13ab4640be/README.md): upstream agent audit states CC BY 4.0; record cards **not independently verified here** | **link only** in this exporter; not loaded | no/no/no claimed for lists | Every Cure and upstream sources, record-level version; do not imply therapeutic proof |
| Every Cure matrix-scores, kg-nodes, kg-edges | [Publication terms](https://docs.dev.everycure.org/releases/public_data_releases/) verified upstream licence pass-through; [node licence](https://huggingface.co/datasets/everycure/kg-nodes/blob/c9c60b2c55e72cedb757b0422f4ea160a9bffc5c/LICENSE.md) upstream audit notes SA/NC/ND | **link only**; scores without a verified licence excluded | varies/varies/varies | Every Cure plus all contributing databases; no blanket CC BY override |
| Open Targets, `repurposing.opentargets` | [Licence](https://platform-docs.opentargets.org/licence), **verified CC0 Platform with upstream licence listing** | **link only** until per-record upstream restrictions reviewed; auxiliary cache not loaded | varies upstream | Open Targets and originating resources; data release and record URL |
| `people.overlap`, `people.persons`, SSSOM person-links | Derived from PubMed/RePORTER/Europe PMC/KAKEN/GtR; above terms apply | **excluded under D36 section 2**, including personal identifiers/profile links and incident edges | inherited | No person records in the bulk release |
| `analogues.models` and source case studies | Each cited official registry/protocol/group/publication URL, terms **unverified** | **link only**; no claims, excerpts or snapshots copied; auxiliary cache not loaded | inherited | Each cited original source; analogues are research leads, not clinical proof |
| Our alignment/curation/transformation contribution | D35, **CC BY 4.0** | Derived IDs/edges and attribution with input restrictions preserved | no/no/no | Rare Disease Atlas contributors, release version; changes indicated; no warranties |

## Inventory and boundaries

### v0.1 additive scope (release-final)

Current master graph readers include KGX and research assets, programmes,
outcomes, models, international research and funding calls. Their appearance in
the graph is not permission to copy. The compiled policy recognizes each source
family for separate size/count reporting; unresolved research inputs stay
identifiers and links. Direct HPO/G2P/ClinGen/GARD remain link only.

Reviewed KGX artifact families permit whitelisted structured labels/assertions
only for Cellosaurus (CC BY), IMPC covered portal output (CC BY), Open Targets
Platform output (CC0), OpenAlex nonpersonal metadata (CC0), ROR registry output
(CC0), and Crossref Funder Registry (CC0), with originating-provider notices.
The primary-page evidence is the artifact-scope correction in
`docs/research/LICENCE-CROSSCHECK.md` and each producer's source manifest.
An individual KGX entity with a stricter/unknown licence still cannot copy fields.
Monarch's heterogeneous union has no blanket licence, Alliance's export notice
remains unresolved, and ChEMBL is SA: these remain pointer-only.

Mapping permission is separate from graph labels: MONDO/Orphadata disease xrefs,
HGNC/NCBI Gene xrefs, UniChem ID pairs, Cellosaurus direct identifier links and
ROR-origin funder pairs are projected without labels or original prose. Only
ROR-backed funder-ror rows are released; cache RePORTER-derived rows are held.
`org-ror`, `trial-xrefs`, `work-ids` and Alliance orthology are held conservatively
for unresolved/mixed input scopes. `release: false` or personal mappings never
enter, regardless of a set's licence string. No direct GARD mapping content is
released. `release-report.json` records each set's decision and exact input hash.

The historical source matrix above describes the initial exporter audit; its
"not loaded" notes describe that earlier scope, superseded by this explicit
v0.1 adapter policy. No unlisted source obtains copying permission.

All 19 top-level raw files seen in this audit are covered above, including the G2P
checksum sidecar. The raw `phenopackets/` directory contains the benchmark archive
and historical HPOA file; neither is copied. `raw` is input only. The current core
snapshot loads MONDO, Orphadata, HPO/HPOA, G2P, ClinicalTrials.gov, RePORTER, PubMed,
HGNC, Wikidata and curated organisation caches. People-resolution records and
all edges incident to a person are excluded under D36 section 2. Other source
families in the matrix are covered for permission decisions but are not ingested
by this release application. Their absence is explicit in the datasheet rather
than represented as graph coverage. Shared cache expansion by other agents must
not silently expand the licence allowlist.

Structured fields allowed into the bundle are only safe node labels and licensed
edge endpoints/relation/kind. Definitions, full records, quotes, abstracts, page
snapshots and contact fields are withheld universally. Restricted edge IDs remain
as opaque identifiers with source/provenance, **without an RDF assertion**. Counts
report these separately. Mixed-source records use the strictest input policy.
Publisher licences, legal free-to-read URLs and full-text reuse permissions are
separate questions; the openaccess agent owns OA enrichment.

The RDF namespace `https://w3id.org/rare-disease-atlas/` is the existing project's
vocabulary namespace, not a claim that a resolver or public deposit exists.
Publishing, DOI assignment and author/operator details are founder-owned steps.
