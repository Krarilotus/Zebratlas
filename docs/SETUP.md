# Local setup

Install Rust 1.91 or newer, Node.js 24 and Python 3.11 or newer. The repository includes a small real public sample; its launcher verifies the manifest and source checksums before starting the API.

In the repository root:

```bash
cargo build --locked --release -j 2 -p atlas-server
python tools/run_public_sample.py --binary target/release/atlas-server --sample data/public-sample --port 8000
```

Leave that terminal running. In a second terminal:

```bash
cd web
npm ci
ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run dev
```

Open `http://localhost:3000`. Search `STXBP1`, open a returned condition or gene, then inspect its graph and source evidence. The bundled sample covers the associations documented in its data manifest. Model-assisted interpretation needs a configured provider or your connector account; plain indexed search can use the sample without a model.

In PowerShell, set the API address with `$env:ZEBRA_BACKEND_URL = 'http://127.0.0.1:8000'`, then run `npm run dev`. The launcher detects the Windows `.exe` extension automatically. Account and contribution databases are in memory; other mutable state uses a temporary directory. The sample files remain unchanged.

For a web production build, run `npm run build`, then `ZEBRA_BACKEND_URL=http://127.0.0.1:8000 npm run start`. `ZEBRA_BACKEND_URL` is the web server's internal API address. Accounts and contributions use the same API by default; configure `ACCOUNTS_API_URL` and `CONTRIB_API_URL` explicitly when they run separately.

Provider credentials belong in the operator's secret store. The public sample has no clinical uploads, accounts, identity merges or inferred mechanism claims. Its launcher refuses inherited production operational/identity-gate configuration, so run it in a separate local environment.

Verify changes with `python tools/audit_submission.py`, the relevant `cargo test --locked -j 2 -p CRATE` checks, and the commands in [CONTRIBUTING](../CONTRIBUTING.md).

To regenerate this sample, provide the three original source files with the hashes in `data/public-sample/sample-inputs.json` under an external `DATA/raw` folder. Run `cargo run --locked -j 2 -p atlas-release --example public_sample -- DATA NEW_OUTPUT data/public-sample/sample-inputs.json`. It refuses changed source bytes or an existing output folder. The original downloads are excluded from Git; regeneration depends on obtaining those exact versions.
