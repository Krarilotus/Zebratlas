# Data and licences

The bundled public sample contains 15 Orphadata conditions, 4 HGNC genes and 16 source-asserted associations. Source identifiers are retained. Its [manifest](../data/public-sample/sample-manifest.json) pins the generated artifacts; [source inputs](../data/public-sample/sample-inputs.json) and native provenance record source URLs, versions, locators, licences and checksums. See [data attribution](../data/public-sample/LICENCES.md).

This is a scoped research-navigation sample. It contains no person records, clinical documents, HPO/G2P/ClinGen inputs, identity merges or inferred mechanisms. The hosted application uses a broader separately provisioned dataset; the sample's counts describe only its own contents.

The launcher checks every selected artifact before using the API's existing read-only snapshot mode. Data licence and field/privacy checks are separate from successful serialization or an API response. Original source attribution and terms remain in the sample's data manifest and notices.

Research snapshots outside the reviewed sample, account state, private documents, downloaded page snapshots, model caches, credentials and historical operational receipts are excluded from the public repository.

The [certified public research projection](https://huggingface.co/datasets/Krarilotus/zebratlas-kg/tree/dd8851755ab95fea08df63b9a1a35975edc73a16) is pinned to revision `dd8851755ab95fea08df63b9a1a35975edc73a16`, tag `public-20261004-v1`. It contains 292,391 nodes, 835,141 edge records (45,556 asserted; 789,585 link-only), 54,344,194 RDF triples and 3,079 accepted identity mappings. The release metadata records its licence/privacy/field/hash/RDF checks and identity-dependency replay.

The [ZIP](https://huggingface.co/datasets/Krarilotus/zebratlas-kg/resolve/dd8851755ab95fea08df63b9a1a35975edc73a16/zebratlas-kg-20261004-public-candidate2.zip) is 441,221,179 bytes; SHA-256 is `b822c0e6eca4bdd73d8b24d2fa13c2c408aa302f9b40371d4edbef697dac9485`. Anonymous download and all 17 decompressed payload hashes were verified. This RDF/JSON/SSSOM projection does not reconstruct the private serving snapshots.

All fourteen optional crosswalk sets remain withheld: six lack original producer input/retrieval evidence; eight have existing licence or scope exclusions. Required accepted-identity provenance remains present. No retrieval dates were invented. The older v0.1 artifact remains excluded from installation guidance.
