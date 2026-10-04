[**Use Zebratlas — zebratlas.org**](https://zebratlas.org)

# Zebratlas

Find people, communities and research working on a rare disease. Describe what you need, search a diagnosis or gene, or attach a document. Zebratlas connects the results to their original sources and official contact routes.

Open a result to see why it matched, inspect its connected graph, and check the evidence behind a relationship. The query workspace exposes the executed SPARQL and supports guarded reruns. Accounts let you save useful results and connect your own model access.

Source assertions and inferred relationships remain distinguishable. For supported ontology-hierarchy queries, nrese explains the rules, premises and sources behind an inference. HermiT checks the authored schema separately; clinical, mechanism and treatment validation remain separate. Uncertain identity matches stay candidates.

## Run it yourself

The public sample contains **15 conditions, 4 genes and 16 condition–gene associations** from Orphadata and HGNC. Each association retains its source evidence. Search, graph inspection and all 16 association hashes passed **22 checks on the Linux API**. These numbers describe this sample; the hosted app has broader coverage. The sample contains no researcher/person records, identity merges or inferred mechanisms; these sample checks do not validate end-to-end collaborator matching.

Requires Git, Node.js 24, Python 3.11+ and Rust 1.91+ with a native C/C++ linker. Commands below use Bash (Git Bash on Windows). The sample launcher needs only Python's standard library. Start the sample API in one terminal:

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

Open `http://localhost:3000/zebra` and search `STXBP1`. The API listens on port 8000; the web interface listens on port 3000. On Windows the launcher finds `atlas-server.exe` automatically. Stop both processes with Ctrl+C.

This is a temporary, source-backed demo: it verifies the bundled snapshots, keeps mutable state outside the sample, uses in-memory account/contribution databases, and removes inherited provider/email secrets. It does **not** configure working signup mail, persistent accounts, personal connectors or an external SPARQL engine. Missing optional services leave their features unavailable; the sample does not become the full hosted research dataset.

For the complete self-hosted configuration, follow [Setup](docs/SETUP.md): persistent API/web startup, shared model budgets, personal BYOK/connectors, account email and optional SPARQL. [Data and licences](docs/DATA.md) describes the sample and hosted data boundaries.

| Optional feature | Configuration |
| --- | --- |
| Shared assistant paid for by the operator | `ATLAS_LLM_CONFIG` with explicit `free_tier = true` and `env_fallback = true`; `ATLAS_FREE_DAILY_USD`, `ATLAS_FREE_SPEND_FILE`, request limits and kill switch |
| Personal model access | A personal connection with `env_fallback = false`, or an authenticated connector account; never silently use the operator's key |
| Signup verification and password reset | `RESEND_API_KEY`, `ATLAS_ACCOUNT_EMAIL_FROM` and an HTTPS `ATLAS_ACCOUNT_PUBLIC_ORIGIN` |
| Persistent account/contribution state | `RARE_ATLAS_ACCOUNTS_DB`, `RARE_ATLAS_CONTRIB_DB`; personal connectors also use `ATLAS_CONNECTOR_DB` |
| External query workspace execution | Matching `ATLAS_QUERY_SCHEMA`, `ATLAS_QUERY_ENDPOINT` and `ATLAS_SPARQL_URL`; separately run and populate the RDF service |

An API key authenticates a provider; it is **not permission to share that key with app users**. The shared connection configuration authorizes guarded operator spending. Personal connections remain separate. Set `ATLAS_FREE_FALLBACKS` explicitly for the shared routes you intend to use; an explicit provider/model/key selection never authorizes a silent switch to another provider. Keep credentials on the API server or the user's connector machine, never in `NEXT_PUBLIC_*` variables.

For bulk research, the [verified public graph](https://huggingface.co/datasets/Krarilotus/zebratlas-kg/tree/dd8851755ab95fea08df63b9a1a35975edc73a16) contains **292,391 nodes and 835,141 edge records**: **45,556 asserted relationships** and **789,585 link-only records**. Fourteen optional crosswalk sets remain withheld. The bundled sample provides the local app setup; the bulk projection supports RDF and JSON analysis.

## Stack

| Layer | Implementation |
| --- | --- |
| Interface | Next.js 16, React 19, TypeScript |
| Search and application API | Rust, Axum, source-backed graph indexes |
| Accounts and contributions | SQLite |
| RDF queries and bounded reasoning | [nrese / OWL-RS](https://github.com/Krarilotus/OWL-RS), SPARQL |
| Model access | Configured provider or a user's connector account |
| Query diagram | Query-by-Graph and Traqula parser/generator |

The interface supports twelve languages. Results include a short source-backed explanation where available; eligible connected models can simplify a condition definition in the selected language. Newer untranslated labels retain explicit English fallbacks; native review is separate from code validation.

## Contribute

Use the site's correction form for a source or relationship that needs review. For code changes, follow [CONTRIBUTING](CONTRIBUTING.md). [Official network routes](docs/NETWORKS.md) connect to patient alliances, research coordination and governed data programmes.

Created by [Johannes Mitschunas](https://www.fmi.uni-jena.de/18557/johannes-mitschunas).

## Licences

Original application code is `MIT OR Apache-2.0`. Query-by-Graph is by [Daniel Motz](https://www.daniel-motz.de), redistributed in Zebratlas with project-specific permission. Its upstream terms and dependency notices are retained in [NOTICE](NOTICE). Dataset and third-party licences remain separate.
