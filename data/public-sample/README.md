# Small public sample

15 source-labelled conditions, four genes and 16 source-asserted associations. This is a runnable sample, not the complete hosted dataset. It contains no people, contacts, patient records, phenotype vocabulary, papers or model assets. The mechanism snapshot contains HGNC identifiers only.

From the repository root:

```bash
cargo build --locked -j 2 -p atlas-server --release
python tools/run_public_sample.py --binary target/release/atlas-server --sample data/public-sample
```

In a second terminal:

```bash
cd web
npm ci
ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run dev
```

Open http://localhost:3000/zebra?lang=en and search for STXBP1. Gene and condition lookup, sourced search, graph neighbourhoods and local association verification are covered by sample-api-smoke.json. Combined nrese queries, phenotype matching and live model calls are outside this sample. The original full source files are not bundled; their hashes, versions and record locators are retained. sample-manifest.json pins every file. See LICENCES.md for attribution and changes.
