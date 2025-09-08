
use anyhow::Result;
use sqlx::{sqlite::{SqliteConnectOptions, SqliteJournalMode}, SqlitePool};
use std::str::FromStr;

pub type Db = SqlitePool;

pub async fn init_db(db_path: &str) -> Result<Db> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path))?
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;

    // Pragmas
    sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await?;
    sqlx::query("PRAGMA synchronous=NORMAL;").execute(&pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON;").execute(&pool).await?;

    // Schema
    sqlx::query(r#"
    CREATE TABLE IF NOT EXISTS blocks (
        number INTEGER PRIMARY KEY,
        hash   TEXT,
        ts     INTEGER
    );
    "#).execute(&pool).await?;

    sqlx::query(r#"
    CREATE TABLE IF NOT EXISTS erc20_transfers (
        tx_hash      TEXT NOT NULL,
        log_index    INTEGER NOT NULL,
        block_number INTEGER NOT NULL,
        contract     TEXT NOT NULL,
        "from"       TEXT NOT NULL,
        "to"         TEXT NOT NULL,
        amount_wei   TEXT NOT NULL,
        PRIMARY KEY (tx_hash, log_index)
    );
    "#).execute(&pool).await?;

    sqlx::query(r#"
    CREATE TABLE IF NOT EXISTS exchange_addresses (
        address  TEXT PRIMARY KEY,
        exchange TEXT NOT NULL
    );
    "#).execute(&pool).await?;

    sqlx::query(r#"
    CREATE TABLE IF NOT EXISTS netflow_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        cumulative_in_wei  TEXT NOT NULL DEFAULT '0',
        cumulative_out_wei TEXT NOT NULL DEFAULT '0',
        last_block         INTEGER
    );
    "#).execute(&pool).await?;

    // Seed single-row netflow_state if empty
    sqlx::query("INSERT OR IGNORE INTO netflow_state(id) VALUES (1);")
        .execute(&pool).await?;

    Ok(pool)
}

pub async fn upsert_exchange_addresses(db: &Db, addrs: &[(&str, &str)]) -> Result<()> {
    for (addr, ex) in addrs {
        sqlx::query(r#"INSERT OR IGNORE INTO exchange_addresses(address, exchange) VALUES(?, ?);"#)
            .bind(addr.to_lowercase())
            .bind(*ex)
            .execute(db).await?;
    }
    Ok(())
}
