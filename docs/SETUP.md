# Local setup

Install Rust 1.91 or newer, Node.js 24 and Python 3.11 or newer. The repository includes a small real public sample; its launcher verifies the manifest and source checksums before starting the API.

Clone the repository and start the sample API:

```bash
git clone https://github.com/Krarilotus/Zebratlas.git
cd Zebratlas
cargo build --locked --release -j 2 -p atlas-server
python tools/run_public_sample.py --binary target/release/atlas-server --sample data/public-sample --port 8000
```

Leave that terminal running. In a second terminal:

```bash
cd web
npm ci
ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run dev
```

Open `http://localhost:3000/zebra`. Search `STXBP1`, open a returned condition or gene, then inspect its graph and source evidence. The bundled sample covers the associations documented in its data manifest. Plain indexed search works without a model. This launcher strips inherited provider/email secrets, so use the direct startup below when deliberately enabling those services.

On Windows, use Git Bash for these commands. The launcher detects the `.exe` extension automatically. Account and contribution databases are in memory; other mutable state uses a temporary directory. The sample files remain unchanged.

## Persistent API and production web

Use a fresh shell without another deployment's data, identity or operational settings. From the repository root, copy the public sample to a separate working data directory and keep databases outside the immutable sample:

```bash
mkdir -p var
cp -R data/public-sample var/sample
export RARE_ATLAS_DATA="$PWD/var/sample"
export RARE_ATLAS_SNAPSHOTS="$PWD/var/sample/cache"
export RARE_ATLAS_ACCOUNTS_DB="$PWD/var/accounts.sqlite"
export RARE_ATLAS_CONTRIB_DB="$PWD/var/contributions.sqlite"
export ATLAS_CONNECTOR_DB="$PWD/var/connectors.sqlite"
export ATLAS_LLM_CACHE_DIR="$PWD/var/llm"
export ATLAS_FREE_SPEND_FILE="$PWD/var/shared-spend.json"
export ATLAS_PUBLIC_ORIGIN="http://localhost:3000"
# Only for local plain-HTTP sessions; omit this on an HTTPS deployment.
export RARE_ATLAS_INSECURE_COOKIES=1
./target/release/atlas-server --data "$RARE_ATLAS_DATA" serve --read-only-snapshots --addr 127.0.0.1:8000
```

Run the copy command only once; choose a new destination if `var/sample` already exists. On Windows, invoke `./target/release/atlas-server.exe` instead. Protect and back up the SQLite files and spend ledger. An existing verified dataset can replace the working sample; the public repository does not include the hosted private snapshots or a populated researcher database.

In the web terminal, run `npm run build`, then `ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run start` from `web/` (run `npm ci` first if needed). `ZEBRA_BACKEND_URL` is the web server's internal API address. Accounts and contributions use the same API by default; configure `ACCOUNTS_API_URL` and `CONTRIB_API_URL` when they run separately. The API does not automatically load an arbitrary `.env` file: supply its environment through your shell or service manager. Next.js can read a private `web/.env.local`; never commit secrets.

For a public deployment, place the services behind HTTPS, keep databases persistent, and set `ATLAS_PUBLIC_ORIGIN` to your public root origin. Remove `RARE_ATLAS_INSECURE_COOKIES`. Set `RARE_ATLAS_TRUST_FORWARDED_FOR=1` only behind a trusted proxy that overwrites forwarded client headers.

## Shared models and personal access

An API key authenticates access; its presence is not permission to spend it for every visitor. Paid personal presets such as `openai` and `anthropic` have environment fallback disabled. A shared server connection must explicitly enable `env_fallback = true` **and** `free_tier = true`; the latter applies the spending/request guard, not a claim that the provider is free.

For an operator-authorized shared assistant, save this credential-free TOML as `llm.local.toml` outside your public configuration. `replace_defaults` makes the authorized connection list explicit:

```toml
replace_defaults = true

[[connections]]
name = "hosted-free"
preset = "hosted-free"
env_fallback = true
free_tier = true

[[connections]]
name = "hosted-anthropic"
preset = "hosted-anthropic"
env_fallback = true
free_tier = true
shared_budget = "hosted-free"

[[connections]]
name = "anthropic"
preset = "anthropic"
env_fallback = false
```

Before direct API startup, set `ATLAS_LLM_CONFIG` to the TOML's absolute path, `ATLAS_LLM_CLI=0`, and `ATLAS_FREE_FALLBACKS=hosted-anthropic`. Supply `OPENROUTER_API_KEY` for the primary and `ANTHROPIC_API_KEY` for the shared fallback through the API server's secret store. Configure only providers you have authorized. Optional guarded aliases `hosted-gemini` and `hosted-kisski` use `GEMINI_API_KEY` and `KISSKI_API_KEY`; they must be present in the TOML, share `hosted-free`'s budget, and be named explicitly in `ATLAS_FREE_FALLBACKS` if wanted. These internal aliases are not personal model-picker entries. Missing credentials do not become successful connections.

Set `ATLAS_FREE_DAILY_USD` to the intended daily spending ceiling and retain `ATLAS_FREE_SPEND_FILE` across restarts. `ATLAS_FREE_PER_HOUR` and `ATLAS_FREE_PER_DAY` limit each visitor; `ATLAS_FREE_PER_MINUTE` and `ATLAS_FREE_CONCURRENCY` limit the shared service. `ATLAS_FREE_MAX_TOKENS` and `ATLAS_FREE_MAX_INPUT_CHARS` bound requests; `ATLAS_FREE_DISABLED=1` stops shared calls. A monetary cap can leave a configured zero-cost route usable; visitor/global limits and the kill switch cannot be bypassed by a fallback. Do not use zero price overrides for a paid provider. The [guard implementation](../crates/atlas-llm/src/free_tier.rs) documents the limits and accounting policy.

`ATLAS_LLM_DEFAULT` selects the default connection. `ATLAS_FREE_FALLBACKS` is a comma-separated list after the primary; set it explicitly rather than relying on built-in defaults. Leave web-side `ZEBRA_LLM_KEY` and `ZEBRA_LLM_CONNECTION` unset for the normal shared chain: they are anonymous operator overrides and an explicit choice locks the route. A supplied personal key, explicitly named connection/model, or saved personal account choice must not silently switch to the shared assistant. Direct API callers can supply `X-LLM-Connection` and `X-LLM-Key`; the key is request-scoped, not persisted as an account preference.

To keep personal credentials on a user's computer, configure accounts first, install their chosen model CLI locally and pair the connector:

```bash
cargo build --locked --release -j 2 -p atlas-connector
./target/release/atlas-connector --server https://YOUR_HOST pair --label "My computer"
./target/release/atlas-connector --server https://YOUR_HOST run --allow claude-code
```

Replace the origin with your actual HTTPS origin and approve the pairing code in that account. Use `.exe` on Windows. `--allow` restricts which configured local routes may execute; a key alone does not create a connected device or authorize model use. Local tool availability, authentication and advertised model IDs are checked separately.

## Accounts and email

New accounts require email verification. Without mail configuration, verification/reset actions are unavailable; do not expect working signup from the temporary demo. Configure the API with `RESEND_API_KEY`, `ATLAS_ACCOUNT_EMAIL_FROM` (or `RESEND_FROM`) containing your authorized sender, and `ATLAS_ACCOUNT_PUBLIC_ORIGIN` containing your trusted **HTTPS root origin**, for example `https://YOUR_HOST`. An origin with a path, query or fragment is rejected. Configure and verify the sender through your mail provider. `ATLAS_PUBLIC_ORIGIN` is the connector origin and is a separate setting.

## Optional RDF query service

Indexed sample search does not require an RDF service. Executed SPARQL, guarded reruns and nrese inference need a separately running service with the matching RDF projection; setting an endpoint does not load its dataset. `ATLAS_QUERY_SCHEMA` points to its version-1 schema card with the matching graph hash; `ATLAS_QUERY_ENDPOINT` is the planning query endpoint; `ATLAS_SPARQL_URL` is the manual workspace endpoint. Configure all three consistently. The public sample contains application snapshots, not a ready-to-run nrese deployment. Operational manifests and proof receipts must describe the dataset actually served; see [data boundaries](DATA.md) before replacing the sample with another dataset.

Provider credentials belong in the operator's secret store. The public sample has no clinical uploads, accounts, identity merges or inferred mechanism claims. Its launcher refuses inherited production operational/identity-gate configuration, so run it in a separate local environment.

Verify changes with `python tools/audit_submission.py`, the relevant `cargo test --locked -j 2 -p CRATE` checks, and the commands in [CONTRIBUTING](../CONTRIBUTING.md).

To regenerate this sample, provide the three original source files with the hashes in `data/public-sample/sample-inputs.json` under an external `DATA/raw` folder. Run `cargo run --locked -j 2 -p atlas-release --example public_sample -- DATA NEW_OUTPUT data/public-sample/sample-inputs.json`. It refuses changed source bytes or an existing output folder. The original downloads are excluded from Git; regeneration depends on obtaining those exact versions.
