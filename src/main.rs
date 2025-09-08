
mod db;
mod indexer;
mod api;

use crate::db::{init_db, upsert_exchange_addresses};
use crate::indexer::{Indexer, IndexerCfg, run_indexer};
use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use ethers::types::Address;
use once_cell::sync::Lazy;
use std::env;
use tokio::try_join;
use tracing_subscriber::{EnvFilter, fmt::Subscriber};

#[derive(Parser, Debug)]
#[command(name="polygon-netflow-indexer")]
struct Args {
    /// Run only the API server (skip the indexer)
    #[arg(long, default_value_t=false)]
    api_only: bool,

    /// Run only the indexer (skip the API)
    #[arg(long, default_value_t=false)]
    indexer_only: bool,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = Subscriber::builder().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber).ok();
}

static DEFAULT_BINANCE: Lazy<Vec<(&'static str, &'static str)>> = Lazy::new(|| vec![
    ("0xF977814e90dA44bFA03b6295A0616a897441aceC", "binance"),
    ("0xe7804c37c13166fF0b37F5aE0BB07A3aEbb6e245", "binance"),
    ("0x505e71695E9bc45943c58adEC1650577BcA68fD9", "binance"),
    ("0x290275e3db66394C52272398959845170E4DCb88", "binance"),
    ("0xD5C08681719445A5Fdce2Bda98b341A49050d821", "binance"),
    ("0x082489A616aB4D46d1947eE3F912e080815b08DA", "binance"),
]);

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    init_tracing();
    let _args = Args::parse();

    let rpc_url = env::var("RPC_URL").expect("RPC_URL required");
    let token_addr = env::var("POL_TOKEN_ADDRESS").expect("POL_TOKEN_ADDRESS required");
    let token = token_addr.parse::<Address>().expect("invalid POL token address");
    let confirmations: u64 = env::var("CONFIRMATIONS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let db_path = env::var("DB_PATH").unwrap_or_else(|_| "./netflow.sqlite".into());

    let db = init_db(&db_path).await?;

    // Seed Binance addresses
    // 1) from .env BINANCE_ADDRESSES (comma-separated), if present
    if let Ok(csv) = env::var("BINANCE_ADDRESSES") {
        let pairs: Vec<(String, String)> = csv.split(',')
            .map(|s| (s.trim().to_string(), "binance".to_string()))
            .collect();
        let refs: Vec<(&str, &str)> = pairs.iter()
            .map(|(a, ex)| (a.as_str(), ex.as_str()))
            .collect();
        upsert_exchange_addresses(&db, &refs).await?;
    } else {
        // 2) fallback to baked-in list
        upsert_exchange_addresses(&db, &DEFAULT_BINANCE).await?;
    }

    let ix = Indexer {
        db: db.clone(),
        cfg: IndexerCfg { rpc_url, token, confirmations },
    };

    // Run both indexer and API
    let indexer_task = tokio::spawn(async move { run_indexer(ix).await });
    let api_task = tokio::spawn(async move { api::serve(db).await });

    // If either fails, bubble up
    let (r1, r2) = try_join!(indexer_task, api_task)?;
    r1?; r2?;
    Ok(())
}
