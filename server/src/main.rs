//! Gootif: notification inbox API + APNs push worker.
//! Replaced the original PHP app on 2026-10-21 (php2shell) with the same HTTP contract and DB.
//!
//!   gootif serve [--listen 127.0.0.1:18090]
//!   gootif push-worker [--once]
//!
//! Config: KEY=VALUE file at $GOOTIF_CONFIG (default /etc/gootif/gootif.env) with
//! SERVICE_KEY, APP_KEY, DB_PATH and the APNS_* settings — see deploy/gootif.env.example.

mod api;
mod config;
mod db;
mod push;

use std::sync::{Arc, Mutex};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("serve") => {
            let listen = args
                .iter()
                .position(|a| a == "--listen")
                .and_then(|i| args.get(i + 1).cloned())
                .unwrap_or_else(|| "127.0.0.1:18090".into());
            serve(&listen).await
        }
        Some("push-worker") => match config::Config::load() {
            Ok(cfg) => push::run_worker(&cfg, args.iter().any(|a| a == "--once")).await,
            Err(e) => Err(e.into()),
        },
        _ => {
            eprintln!("usage: gootif serve [--listen ADDR] | gootif push-worker [--once]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("[gootif] fatal: {e}");
        std::process::exit(1);
    }
}

async fn serve(listen: &str) -> Result<(), BoxError> {
    let cfg = config::Config::load()?;
    let service_key = cfg.require("SERVICE_KEY")?;
    let app_key = cfg.require("APP_KEY")?;
    let conn = db::open(&cfg.require("DB_PATH")?)?;
    let app = Arc::new(api::App { cfg, service_key, app_key, db: Mutex::new(conn) });

    let router = axum::Router::new().fallback(api::dispatch).with_state(app);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    eprintln!("[gootif] listening on {listen}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
