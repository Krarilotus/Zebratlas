use clap::{Parser, Subcommand};
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(name = "atlas-connector")]
struct Args {
    #[arg(long, default_value = "https://zebratlas.org")]
    server: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Pair {
        #[arg(long, default_value = "My computer")]
        label: String,
    },
    Run {
        #[arg(long, value_delimiter = ',')]
        allow: Vec<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        mcp: bool,
    },
    Forget,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let origin = atlas_connector::client::origin(&args.server)?;
    match args.command {
        Command::Pair { label } => atlas_connector::client::pair(&origin, &label).await,
        Command::Forget => atlas_connector::client::forget(&origin),
        Command::Run { allow, config, mcp } => {
            let registry = match config {
                Some(p) => atlas_llm::Registry::from_file(&p, true)?,
                None => atlas_llm::Registry::presets(true)?,
            };
            let llm = atlas_llm::Llm::new(registry, atlas_llm::Cache::new("unused", atlas_llm::CacheMode::Off));
            atlas_connector::client::run(&origin, Arc::new(llm), allow, mcp).await
        }
    }
}
