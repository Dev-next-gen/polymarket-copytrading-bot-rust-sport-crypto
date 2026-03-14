//! Copy-trading backend: follow leader addresses from trade.toml.
//! Config: config.json (polymarket credentials + API keys), trade.toml (targets, filters, exit).
//! Serves UI at port (trade.toml) and /api/state for the Leptos frontend.

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::sse::{Event, KeepAlive, Sse},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use clap::Parser;
use async_stream::stream;
use log::info;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
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

/// Notify clients that state changed (e.g. new trade); used for SSE.
pub type NotifyTx = broadcast::Sender<()>;

#[derive(Clone)]
struct AppState {
    web: SharedState,
    ui_dir: PathBuf,
    notify: NotifyTx,
}

async fn sse_state_updates(State(app): State<AppState>) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let mut rx = app.notify.subscribe();
    let stream = stream! {
        loop {
            match rx.recv().await {
                Ok(_) => yield Ok(Event::default().data("update")),
                Err(broadcast::error::RecvError::Lagged(_)) => yield Ok(Event::default().data("update")),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn api_state(State(app): State<AppState>) -> axum::response::Response {
    let state = web_state::get_state(app.web).await;
    let mut res = Json(state).into_response();
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "no-store, no-cache, must-revalidate, max-age=0"
            .parse()
            .unwrap(),
    );
    res.headers_mut().insert(
        axum::http::header::PRAGMA,
        "no-cache".parse().unwrap(),
    );
    res
}

/// Serve index.html for GET / so the SPA always loads the same entry point.
async fn serve_index(State(app): State<AppState>) -> Result<Response, (StatusCode, &'static str)> {
    use axum::response::IntoResponse;
    let path = app.ui_dir.join("index.html");
    let contents = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "index.html not found"))?;
    let mut res = Html(contents).into_response();
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "no-cache, no-store, must-revalidate".parse().unwrap(),
    );
    Ok(res)
}

/// Set correct Content-Type for .wasm and .js so the browser runs the Leptos app.
async fn static_asset_mime(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    if res.status().is_success() {
        let headers = res.headers_mut();
        if path.ends_with(".wasm") {
            let _ = headers.insert("content-type", "application/wasm".parse().unwrap());
        } else if path.ends_with(".js") {
            let _ = headers.insert(
                "content-type",
                "application/javascript; charset=utf-8".parse().unwrap(),
            );
        }
    }
    res
}

fn spawn_web_server(state: SharedState, notify: NotifyTx, port: u16, ui_dir: PathBuf) {
    tokio::spawn(async move {
        let app_state = AppState {
            web: state,
            ui_dir: ui_dir.clone(),
            notify,
        };
        let serve_dir = ServeDir::new(&ui_dir);
        let app = Router::new()
            .route("/api/state", get(api_state))
            .route("/api/state/stream", get(sse_state_updates))
            .route("/", get(serve_index))
            .with_state(app_state)
            .fallback_service(serve_dir)
            .layer(middleware::from_fn(static_asset_mime));
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
    let ui_dir = if args.ui_dir.is_absolute() {
        args.ui_dir.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&args.ui_dir)
    };
    let (notify_tx, _) = broadcast::channel(64);
    info!("Serving UI from: {}", ui_dir.display());
    spawn_web_server(web_state.clone(), notify_tx.clone(), port, ui_dir);
    info!("UI: http://localhost:{} (or http://<this-host-ip>:{} from another device)", port, port);

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
                for p in &positions {
                    let slug = p.slug.as_deref().unwrap_or("?");
                    let outcome = p.outcome.as_deref().unwrap_or("?");
                    web_state::push_trade(
                        web_state.clone(),
                        "POS".to_string(),
                        "—".to_string(),
                        outcome.to_string(),
                        format!("{:.2}", p.size),
                        format!("{:.3}", p.cur_price),
                        slug.to_string(),
                        Some(user_lower.clone()),
                        Some("loaded".to_string()),
                    )
                    .await;
                    let _ = notify_tx.send(());
                }
                prev.insert(user_lower.clone(), curr);
                continue;
            }
            let trades = diff_to_trades(&user_lower, &curr, prev_map.unwrap());
            for trade in trades {
                let slug = trade.slug.as_deref().unwrap_or("?");
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                // Push every detected trade (BUY and SELL) to the UI; then decide whether to copy.
                if !should_copy_trade(&copy_config, &trade) {
                    web_state::push_trade(
                        web_state.clone(),
                        "LIVE".to_string(),
                        trade.side.clone(),
                        outcome.to_string(),
                        trade.size.clone(),
                        trade.price.clone(),
                        slug.to_string(),
                        Some(user_lower.clone()),
                        Some("filtered".to_string()),
                    )
                    .await;
                    let _ = notify_tx.send(());
                    continue;
                }
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
                    let _ = notify_tx.send(());
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
                        let _ = notify_tx.send(());
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
                        let _ = notify_tx.send(());
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
                        let _ = notify_tx.send(());
                    }
                }
            }
            prev.insert(user_lower, curr);
        }
        tokio::time::sleep(poll_interval).await;
    }
}
