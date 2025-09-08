
# Real-time Polygon POL Net-Flow Indexer (Rust + SQLite)

A concise, real-time indexer that listens to **POL** ERC‑20 `Transfer` events on **Polygon**, filters transfers
to/from **Binance**-labeled addresses, and maintains a **cumulative net-flow** counter (to - from) since startup.
No backfill is performed.

> **Net-flow definition**: cumulative sum of POL transferred **to** Binance addresses minus POL transferred **from**
> those addresses, aggregated over time from the moment the indexer starts.

---

## Quick Start

### 1) Prerequisites
- Rust stable (>= 1.76)
- SQLite3 (CLI optional; the app creates tables automatically)
- A Polygon **WebSocket** RPC URL (e.g., from your provider)
- The **POL token contract address on Polygon** (configure via `.env`)
- Linux/Mac/WSL recommended (Windows works too)

### 2) Configure
Create a `.env` file at project root (or set these as environment variables):

```env
# --- Networking ---
RPC_URL=wss://your-polygon-ws-endpoint
# POL token address on Polygon (ERC-20). Keep configurable to avoid hardcoding.
POL_TOKEN_ADDRESS=0x0000000000000000000000000000000000000000

# Optional: number of confirmations to treat a block as final (basic reorg safety)
CONFIRMATIONS=20

# --- Database ---
DB_PATH=./netflow.sqlite

# --- Binance exchange addresses (EVM checksum or lowercase) ---
# Comma-separated list. Provided by the task.
BINANCE_ADDRESSES=0xF977814e90dA44bFA03b6295A0616a897441aceC,0xe7804c37c13166fF0b37F5aE0BB07A3aEbb6e245,0x505e71695E9bc45943c58adEC1650577BcA68fD9,0x290275e3db66394C52272398959845170E4DCb88,0xD5C08681719445A5Fdce2Bda98b341A49050d821,0x082489A616aB4D46d1947eE3F912e080815b08DA
```

> ⚠️ **POL address**: Keep it configurable; POL is the successor to MATIC and addresses may evolve per network/bridge. The app only listens to the given token address on Polygon.

### 3) Run
```bash
cargo run --release
```

The indexer will:
- connect to Polygon via WebSocket,
- subscribe to `Transfer(address,address,uint256)` logs for the configured POL token,
- stream new blocks, decode transfers,
- persist raw transfers, and
- update a **cumulative** net-flow.

### 4) Query the current cumulative net-flow
A tiny HTTP server is exposed on `127.0.0.1:8080`:

```bash
curl http://127.0.0.1:8080/netflow
```

Example JSON:
```json
{"symbol":"POL","decimals":18,"cumulative_in":"123.45","cumulative_out":"67.89","cumulative_net":"55.56","last_block":53876543}
```

---

## Project Structure

```
polygon-netflow-indexer/
├─ src/
│  ├─ main.rs          # bootstrap: env, DB, indexer, API
│  ├─ db.rs            # SQLite helpers & schema init
│  ├─ indexer.rs       # real-time log subscription & processing
│  └─ api.rs           # basic Axum HTTP API
├─ Cargo.toml
├─ .gitignore
├─ .env.example
└─ README.md
```

---

## Schema (SQLite)

- `blocks(number INTEGER PRIMARY KEY, hash TEXT, ts INTEGER)`
- `erc20_transfers(tx_hash TEXT, log_index INTEGER, block_number INTEGER, contract TEXT, "from" TEXT, "to" TEXT, amount_wei TEXT, PRIMARY KEY(tx_hash, log_index))`
- `exchange_addresses(address TEXT PRIMARY KEY, exchange TEXT NOT NULL)`
- `netflow_state(id INTEGER PRIMARY KEY CHECK(id=1), cumulative_in_wei TEXT NOT NULL DEFAULT '0', cumulative_out_wei TEXT NOT NULL DEFAULT '0', last_block INTEGER)`

**Notes**
- Big integers stored as **decimal strings** (`TEXT`) to avoid precision loss.
- `netflow_state` maintains a single row (id=1) of cumulative totals.
- WAL mode enabled for better write concurrency.

---

## How the Indexing Works

1. Subscribe to POL `Transfer` logs via a **topic filter** and **token contract address**.
2. For each log:
   - Decode `from`, `to`, `value` (uint256).
   - If either side is in `exchange_addresses` (Binance set), count as **in** or **out**:
     - **in**: `to` ∈ Binance list
     - **out**: `from` ∈ Binance list
   - Insert raw transfer into `erc20_transfers` (idempotent).
   - Update `netflow_state` cumulative totals and `last_block` atomically.
3. Basic reorg safety: only **apply** logs from blocks that are at least `CONFIRMATIONS` behind the current head (or keep a ring buffer and finalize later). This template implements *simple lag* finalization for clarity.

---

## Extend to Multiple Exchanges

- Insert additional labeled addresses into `exchange_addresses` (`exchange` column distinct names like `binance`, `okx`, etc.).
- Keep separate `netflow_state` rows per exchange (add `exchange TEXT` to the PK or create a new table `netflow_by_exchange`).
- Run the same log stream—classification happens by address membership set.

---

## No Backfill Policy

- At startup, the indexer grabs the **latest** block number and begins streaming new logs from there.
- You can optionally persist `start_block` in a table if needed for audit.

---

## Commands & Operations

- **Run indexer + API**: `cargo run --release`
- **Query**: `curl http://127.0.0.1:8080/netflow`
- **DB file**: `./netflow.sqlite` by default (configurable by `DB_PATH`).

---

## Presentation

- This README explains the schema, logic, API, and scalability plan.
- The source includes inline comments describing the transformation steps.

---

## License

MIT
