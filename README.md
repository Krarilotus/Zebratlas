[**Use Zebratlas — zebratlas.org**](https://zebratlas.org)

# Zebratlas

Find people, communities and research working on a rare disease. Describe what you need, search a diagnosis or gene, or attach a document. Zebratlas connects the results to their original sources and official contact routes.

Open a result to see why it matched, inspect its connected graph, and check the evidence behind a relationship. The query workspace exposes the executed SPARQL and supports guarded reruns. Accounts let you save useful results and connect your own model access.

Source assertions and inferred relationships remain distinguishable. For supported ontology-hierarchy queries, nrese explains the rules, premises and sources behind an inference. HermiT checks the authored schema separately; clinical, mechanism and treatment validation remain separate. Uncertain identity matches stay candidates.

## Run it yourself

The public sample contains **15 conditions, 4 genes and 16 condition–gene associations** from Orphadata and HGNC. Each association retains its source evidence. Search, graph inspection and all 16 association hashes passed **22 checks on the Linux API**. These numbers describe this sample; the hosted app has broader coverage. The sample contains no researcher/person records, identity merges or inferred mechanisms; these sample checks do not validate end-to-end collaborator matching.

Requires Node.js 24, Python 3.11+ and Rust 1.91+. Start the sample API in one terminal:

```bash
git clone https://github.com/Krarilotus/Zebratlas.git
cd Zebratlas
cargo build --locked --release -j 2 -p atlas-server
python tools/run_public_sample.py --binary target/release/atlas-server --sample data/public-sample --port 8000
```

Start the web interface in another terminal:

```bash
cd web
npm ci
ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run dev
```

Open `http://localhost:3000`. [Setup](docs/SETUP.md) covers production builds and optional model access; [data and licences](docs/DATA.md) describes the sample and hosted data boundaries.

For bulk research, the [verified public graph](https://huggingface.co/datasets/Krarilotus/zebratlas-kg/tree/dd8851755ab95fea08df63b9a1a35975edc73a16) contains **292,391 nodes and 835,141 edge records**: **45,556 asserted relationships** and **789,585 link-only records**. Fourteen optional crosswalk sets remain withheld. The bundled sample provides the local app setup; the bulk projection supports RDF and JSON analysis.

## Stack

| Layer | Implementation |
| --- | --- |
| Interface | Next.js 16, React 19, TypeScript |
| Search and application API | Rust, Axum, source-backed graph indexes |
| Accounts and contributions | SQLite |
| RDF queries and bounded reasoning | nrese / OWL-RS, SPARQL |
| Model access | Configured provider or a user's connector account |
| Query diagram | Query-by-Graph and Traqula parser/generator |

The interface has English and German catalogues. Its 12-language selector uses explicit English fallbacks where translations are incomplete; native review is separate from code validation.

## Contribute

Use the site's correction form for a source or relationship that needs review. For code changes, follow [CONTRIBUTING](CONTRIBUTING.md). [Official network routes](docs/NETWORKS.md) connect to patient alliances, research coordination and governed data programmes.

Created by [Johannes Mitschunas](https://www.fmi.uni-jena.de/18557/johannes-mitschunas).

## Licences

Original application code is `MIT OR Apache-2.0`. Query-by-Graph is by [Daniel Motz](https://www.daniel-motz.de), redistributed in Zebratlas with project-specific permission. Its upstream terms and dependency notices are retained in [NOTICE](NOTICE). Dataset and third-party licences remain separate.
