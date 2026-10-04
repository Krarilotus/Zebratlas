# Data and licences

The bundled public sample contains 15 Orphadata conditions, 4 HGNC genes and 16 source-asserted associations. Source identifiers are retained. Its [manifest](../data/public-sample/sample-manifest.json) pins the generated artifacts; [source inputs](../data/public-sample/sample-inputs.json) and native provenance record source URLs, versions, locators, licences and checksums. See [data attribution](../data/public-sample/LICENCES.md).

This is a scoped research-navigation sample. It contains no person records, clinical documents, HPO/G2P/ClinGen inputs, identity merges or inferred mechanisms. The hosted application uses a broader separately provisioned dataset; the sample's counts describe only its own contents.

The launcher checks every selected artifact before using the API's existing read-only snapshot mode. Data licence and field/privacy checks are separate from successful serialization or an API response. Original source attribution and terms remain in the sample's data manifest and notices.

Research snapshots outside the reviewed sample, account state, private documents, downloaded page snapshots, model caches, credentials and historical operational receipts are excluded from the public repository. A future bulk graph download must have a licensed public projection, field/privacy validation receipt, immutable revision and SHA-256.

The bulk projection is not yet publication-certified. Six optional identifier-crosswalk sets need their original producer input manifests and retrieval metadata; those outputs remain withheld while the full release is checked. No retrieval date is inferred to fill that gap. The older public v0.1 artifact is withdrawn from installation guidance.
