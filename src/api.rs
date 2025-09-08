
use axum::{routing::get, Router, response::IntoResponse};
use serde::Serialize;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use tracing::info;

#[derive(Serialize)]
struct NetflowOut {
    symbol: &'static str,
    decimals: u8,
    cumulative_in: String,
    cumulative_out: String,
    cumulative_net: String,
    last_block: Option<i64>,
}

async fn netflow_handler(db: SqlitePool) -> impl IntoResponse {
    let (in_wei, out_wei, last_block) = sqlx::query_as::<_, (String, String, Option<i64>)>(r#"
        SELECT cumulative_in_wei, cumulative_out_wei, last_block FROM netflow_state WHERE id=1;
    "#).fetch_one(&db).await.unwrap_or(("0".into(), "0".into(), None));

    let in_int = rug::Integer::from_str_radix(&in_wei, 10).unwrap_or_default();
    let out_int = rug::Integer::from_str_radix(&out_wei, 10).unwrap_or_default();
    let net = &in_int - &out_int;

    // Present as decimal POL with 18 decimals (configurable if desired)
    let decimals: u32 = 18;
    let fmt = |x: &rug::Integer| -> String {
        let ten = rug::Integer::from(10);
        let scale = ten.pow(decimals);
        let (q, r) = x.clone().div_rem(scale);
        if r == 0 {
            format!("{}", q)
        } else {
            let mut frac = r.to_string_radix(10);
            // pad leading zeros in fractional part
            if frac.len() < decimals as usize {
                let pad = (decimals as usize) - frac.len();
                frac = "0".repeat(pad) + &frac;
            }
            // trim trailing zeros
            while frac.ends_with('0') { frac.pop(); }
            format!("{}.{}", q, frac)
        }
    };

    let out = NetflowOut {
        symbol: "POL",
        decimals: 18,
        cumulative_in: fmt(&in_int),
        cumulative_out: fmt(&out_int),
        cumulative_net: fmt(&net),
        last_block,
    };
    axum::Json(out)
}

pub async fn serve(db: SqlitePool) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/netflow", get({
            let db = db.clone();
            move || netflow_handler(db.clone())
        }));

    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    info!("HTTP API listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
