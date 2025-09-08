
use anyhow::{Context, Result};
use ethers::abi::{AbiDecode};
use ethers::core::types::{Filter, H160, H256, Log, BlockNumber, Address, U64};
use ethers::providers::{Middleware, Provider, Ws};
use sqlx::SqlitePool;
use tracing::{info, warn, error};
use std::sync::Arc;

const TRANSFER_TOPIC: &str = "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"; // keccak("Transfer(address,address,uint256)")

#[derive(Clone)]
pub struct IndexerCfg {
    pub rpc_url: String,
    pub token: Address,
    pub confirmations: u64,
}

#[derive(Clone)]
pub struct Indexer {
    pub db: SqlitePool,
    pub cfg: IndexerCfg,
}

fn topic_h256(hex: &str) -> H256 {
    H256::from_slice(&hex::decode(hex).expect("valid hex")[..])
}

#[inline]
fn to_checksum_lower(a: Address) -> String {
    format!("{:#x}", a)
}

pub async fn run_indexer(ix: Indexer) -> Result<()> {
    let ws = Ws::connect(ix.cfg.rpc_url.clone()).await
        .context("failed to connect WS")?;
    let provider = Provider::new(ws);

    // Start from latest block (no backfill)
    let head = provider.get_block_number().await?.as_u64();
    info!("Starting from head block {}", head);

    // Subscribe to logs for Transfer events for the token
    let filter = Filter::new()
        .address(ix.cfg.token)
        .topic0(topic_h256(TRANSFER_TOPIC));

    let mut sub = provider.subscribe_logs(&filter).await?;
    info!("Subscribed to Transfer logs for token {}", to_checksum_lower(ix.cfg.token));

    while let Some(log) = sub.next().await {
        if let Err(e) = handle_log(&ix, &provider, log).await {
            error!("handle_log error: {e:#}");
        }
    }
    Ok(())
}

async fn handle_log(ix: &Indexer, provider: &Provider<Ws>, lg: Log) -> Result<()> {
    // Basic finality lag
    let head = provider.get_block_number().await?.as_u64();
    let Some(bn) = lg.block_number.map(|b| b.as_u64()) else {
        return Ok(());
    };
    if head.saturating_sub(bn) < ix.cfg.confirmations {
        // not final enough
        return Ok(());
    }

    // Decode topics:
    // topic0 = Transfer(...)
    // topic1 = from, topic2 = to, data = value
    if lg.topics.len() < 3 {
        return Ok(());
    }
    let from = H160::from_slice(&lg.topics[1].as_bytes()[12..]);
    let to   = H160::from_slice(&lg.topics[2].as_bytes()[12..]);
    let amount = ethers::abi::Uint::decode(lg.data.as_ref())?; // value
    let amount_str = amount.to_string();

    let tx_hash = lg.transaction_hash.unwrap_or_default();
    let log_index = lg.log_index.unwrap_or_default().as_u64() as i64;
    let block_number = bn as i64;
    let contract = ix.cfg.token;

    // Persist raw transfer (idempotent)
    sqlx::query(r#"
        INSERT OR IGNORE INTO erc20_transfers
            (tx_hash, log_index, block_number, contract, "from", "to", amount_wei)
        VALUES (?, ?, ?, ?, ?, ?, ?);
    "#)
        .bind(format!("{:#x}", tx_hash))
        .bind(log_index)
        .bind(block_number)
        .bind(format!("{:#x}", contract))
        .bind(to_checksum_lower(from))
        .bind(to_checksum_lower(to))
        .bind(amount_str.clone())
        .execute(&ix.db).await?;

    // Classify in/out relative to exchange set
    let from_is_ex = is_exchange(&ix.db, &from).await?;
    let to_is_ex   = is_exchange(&ix.db, &to).await?;

    if from_is_ex || to_is_ex {
        // Single-row state update
        if to_is_ex {
            sqlx::query(r#"
                UPDATE netflow_state
                SET cumulative_in_wei = CAST((CAST(cumulative_in_wei AS INTEGER) + CAST(? AS INTEGER)) AS TEXT),
                    last_block = MAX(COALESCE(last_block, 0), ?)
                WHERE id = 1;
            "#)
            .bind(amount_str.clone())
            .bind(block_number)
            .execute(&ix.db).await?;
        }
        if from_is_ex {
            sqlx::query(r#"
                UPDATE netflow_state
                SET cumulative_out_wei = CAST((CAST(cumulative_out_wei AS INTEGER) + CAST(? AS INTEGER)) AS TEXT),
                    last_block = MAX(COALESCE(last_block, 0), ?)
                WHERE id = 1;
            "#)
            .bind(amount_str.clone())
            .bind(block_number)
            .execute(&ix.db).await?;
        }
    }

    Ok(())
}

async fn is_exchange(db: &SqlitePool, addr: &Address) -> Result<bool> {
    let a = format!("{:#x}", addr);
    let rec = sqlx::query_scalar::<_, Option<i64>>(
        r#"SELECT 1 FROM exchange_addresses WHERE lower(address)=lower(?) LIMIT 1;"#)
        .bind(a)
        .fetch_optional(db).await?;
    Ok(rec.is_some())
}
