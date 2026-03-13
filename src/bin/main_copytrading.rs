//! Copy-trading backend: follow leader addresses from trade.toml.
//! Config: config.json (polymarket credentials + API keys), trade.toml (targets, filters, exit).
//! Serves UI at port (trade.toml) and /api/state for the Leptos frontend.

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use clap::Parser;
use log::info;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::services::ServeDir;

use polymarket_trading_bot::api::PolymarketApi;
use polymarket_trading_bot::config::Config;
use polymarket_trading_bot::copy_trading::{
    build_snapshot_map, copy_trade, diff_to_trades, record_entry, should_copy_trade, spawn_exit_loop,
    CopyTradingConfig, SnapshotMap,
};
use polymarket_trading_bot::web_state::{self, BotState, SharedState};

#[derive(Parser, Debug)]
#[command(name = "main_copytrading")]
#[command(about = "Copy-trade from leader addresses (trade.toml). Uses config.json for Polymarket credentials.")]
pub struct CopyArgs {
    /// Config file (polymarket credentials, API key)
    #[arg(short, long, default_value = "config.json")]
    pub config: PathBuf,

    /// Copy-trading config (targets, filters, exit)
    #[arg(short, long, default_value = "trade.toml")]
    pub trade_config: PathBuf,

    /// Run in simulation (no real orders)
    #[arg(long)]
    pub simulation: bool,

    /// Directory to serve UI from (default: frontend/dist). Build with: cd frontend && trunk build
    #[arg(long, default_value = "frontend/dist")]
    pub ui_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    web: SharedState,
    ui_dir: PathBuf,
}

async fn api_state(State(app): State<AppState>) -> Json<BotState> {
    Json(web_state::get_state(app.web).await)
}

fn spawn_web_server(state: SharedState, port: u16, ui_dir: PathBuf) {
    tokio::spawn(async move {
        let app_state = AppState { web: state, ui_dir: ui_dir.clone() };
        let serve_dir = ServeDir::new(&ui_dir);
        let app = Router::new()
            .route("/api/state", get(api_state))
            .with_state(app_state)
            .fallback_service(serve_dir);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
        axum::serve(listener, app).await.expect("serve");
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = CopyArgs::parse();
    let config = Config::load(&args.config)?;

    let trade_path = if args.trade_config.is_absolute() {
        args.trade_config.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&args.trade_config)
    };
    let copy_config = CopyTradingConfig::load(&trade_path).with_context(|| {
        format!(
            "Load trade.toml (copy targets, filters, exit). Tried: {} (cwd: {})",
            trade_path.display(),
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).display()
        )
    })?;

    let targets = copy_config.target_addresses();
    if targets.is_empty() {
        anyhow::bail!(
            "No copy targets. Set copy.target_address or copy.target_addresses in {}",
            trade_path.display()
        );
    }

    let simulation = args.simulation || copy_config.simulation;
    let api = Arc::new(PolymarketApi::new(
        config.polymarket.gamma_api_url.clone(),
        config.polymarket.clob_api_url.clone(),
        config.polymarket.api_key.clone(),
        config.polymarket.api_secret.clone(),
        config.polymarket.api_passphrase.clone(),
        config.polymarket.private_key.clone(),
        config.polymarket.proxy_wallet_address.clone(),
        config.polymarket.signature_type,
    ));

    if !simulation {
        api.authenticate().await.context("Polymarket authenticate")?;
    }

    let wallet = if simulation {
        "simulation".to_string()
    } else {
        api.get_wallet_address().context("Get wallet address")?
    };

    info!(
        "Copy-trading | {} | {} target(s) | wallet: {}",
        if simulation { "SIMULATION" } else { "LIVE" },
        targets.len(),
        if wallet.len() > 20 {
            format!("{}...{}", &wallet[..10], &wallet[wallet.len() - 8..])
        } else {
            wallet.clone()
        }
    );

    let web_state = web_state::new_shared_state();
    web_state::set_status(
        web_state.clone(),
        if simulation { "Sim".to_string() } else { "Live".to_string() },
        targets.len() as u32,
        Some(wallet.clone()),
        Some(targets.clone()),
    )
    .await;
    web_state::set_ui_config(
        web_state.clone(),
        copy_config.ui.delta_highlight_sec,
        copy_config.ui.delta_animation_sec,
    )
    .await;
    let port = copy_config.port;
    spawn_web_server(web_state.clone(), port, args.ui_dir);
    info!("UI http://localhost:{}", port);

    let entries: Arc<Mutex<HashMap<String, polymarket_trading_bot::copy_trading::Entry>>> =
        Arc::new(Mutex::new(HashMap::new()));
    if !simulation
        && (copy_config.exit.take_profit > 0.0
            || copy_config.exit.stop_loss > 0.0
            || copy_config.exit.trailing_stop > 0.0)
    {
        spawn_exit_loop(
            api.clone(),
            copy_config.clone(),
            wallet.clone(),
            entries.clone(),
        );
        info!("Exit loop started (take_profit/stop_loss/trailing_stop)");
    }

    let poll_interval = std::time::Duration::from_secs(
        copy_config.copy.poll_interval_sec.max(5),
    );
    let mut prev: HashMap<String, SnapshotMap> = HashMap::new();

    loop {
        for user in &targets {
            let user_lower = user.to_lowercase();
            let positions = match api.get_positions(&user_lower).await {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("get_positions {}: {}", user_lower, e);
                    continue;
                }
            };
            let curr = build_snapshot_map(&positions);
            let pos_list: Vec<_> = positions
                .iter()
                .map(|p| {
                    (
                        p.slug.clone().unwrap_or_else(|| "?".to_string()),
                        p.outcome.clone().unwrap_or_else(|| "?".to_string()),
                        p.size,
                        p.cur_price,
                    )
                })
                .collect();
            web_state::set_positions(web_state.clone(), user_lower.clone(), pos_list).await;

            let prev_map = prev.get(&user_lower);
            if prev_map.is_none() {
                info!("INIT | {} | {} position(s)", user_lower, curr.len());
                prev.insert(user_lower.clone(), curr);
                continue;
            }
            let trades = diff_to_trades(&user_lower, &curr, prev_map.unwrap());
            for trade in trades {
                if !should_copy_trade(&copy_config, &trade) {
                    continue;
                }
                let slug = trade.slug.as_deref().unwrap_or("?");
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                if simulation {
                    info!(
                        "SIM | {} {} {} size {} @ {} | {} {}",
                        trade.side,
                        outcome,
                        slug,
                        trade.size,
                        trade.price,
                        user_lower,
                        "skipped"
                    );
                    web_state::push_trade(
                        web_state.clone(),
                        "SIM".to_string(),
                        trade.side.clone(),
                        outcome.to_string(),
                        trade.size.clone(),
                        trade.price.clone(),
                        slug.to_string(),
                        Some(user_lower.clone()),
                        Some("skipped".to_string()),
                    )
                    .await;
                    continue;
                }
                match copy_trade(
                    &api,
                    &trade,
                    copy_config.copy.size_multiplier,
                    copy_config.filter.buy_amount_limit_in_usd,
                )
                .await
                {
                    Ok(Some((size, price))) => {
                        {
                            let mut ent = entries.lock().await;
                            record_entry(&mut *ent, &trade.asset_id, size, price);
                        }
                        info!(
                            "LIVE | {} {} {} size {} @ {} | from {} | ok",
                            trade.side,
                            outcome,
                            slug,
                            trade.size,
                            trade.price,
                            user_lower
                        );
                        web_state::push_trade(
                            web_state.clone(),
                            "LIVE".to_string(),
                            trade.side.clone(),
                            outcome.to_string(),
                            trade.size.clone(),
                            trade.price.clone(),
                            slug.to_string(),
                            Some(user_lower.clone()),
                            Some("ok".to_string()),
                        )
                        .await;
                    }
                    Ok(None) => {
                        info!(
                            "LIVE | {} {} {} size {} @ {} | from {} | ok",
                            trade.side,
                            outcome,
                            slug,
                            trade.size,
                            trade.price,
                            user_lower
                        );
                        web_state::push_trade(
                            web_state.clone(),
                            "LIVE".to_string(),
                            trade.side.clone(),
                            outcome.to_string(),
                            trade.size.clone(),
                            trade.price.clone(),
                            slug.to_string(),
                            Some(user_lower.clone()),
                            Some("ok".to_string()),
                        )
                        .await;
                    }
                    Err(e) => {
                        log::warn!(
                            "LIVE | {} {} | from {} | FAILED: {}",
                            trade.side,
                            slug,
                            user_lower,
                            e
                        );
                        web_state::push_trade(
                            web_state.clone(),
                            "LIVE".to_string(),
                            trade.side.clone(),
                            outcome.to_string(),
                            trade.size.clone(),
                            trade.price.clone(),
                            slug.to_string(),
                            Some(user_lower.clone()),
                            Some(format!("FAILED: {}", e)),
                        )
                        .await;
                    }
                }
            }
            prev.insert(user_lower, curr);
        }
        tokio::time::sleep(poll_interval).await;
    }
}
