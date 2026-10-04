# Contributing

Report a reproducible problem with the affected page, expected behaviour and browser or API version. Use synthetic examples in issues and tests; send personal-data concerns through the site's private removal request.

Keep changes focused. Preserve source URLs, record locators, retrieval dates, hashes and identity decisions. An inferred link needs its derivation; a source correction needs the original evidence and a reason. Put user-facing copy in the locale catalogues.

Before a pull request, run the checks relevant to the changed code:

```bash
python tools/audit_submission.py
cargo test --locked -j 2 -p atlas-core -p atlas-ingest -p atlas-ask -p atlas-server
cd web
npm ci
npm run test:unit
npm run check:account
npm run check:query
npm run build
```

The data boundary audit checks source manifests, dependency notices and forbidden artifacts. New data requires separate licence and privacy review. Credentials, downloaded records, account state and private documents stay outside the repository.

For ontology and release-validation changes, install `requirements-dev.txt` in a virtual environment and build `cargo build --locked -j 2 -p atlas-release`. Run `python -m unittest discover -s crates/atlas-release/scripts -p 'test_*.py'` and `node --no-warnings web/scripts/check-zebra-tbox.mjs`. These checks use synthetic fixtures and the bundled authored ontology, not private snapshots or historical benchmark data.
