use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Semaphore};
use tokio_tungstenite::{connect_async_with_config, tungstenite::Message};

use crate::copy_trading::{
    copy_trade, record_entry, should_copy_trade, CopyTradingConfig, LeaderTrade,
};
use crate::web_state;
use std::collections::HashMap;

// Polymarket's public activity/trades feed can lag on-chain fills. Set LOG_MATCH_LAG=1
// to log (wall_clock − payload time) for matched targets and separate feed vs bot delay.
const ACTIVITY_WS_URL: &str = "wss://ws-live-data.polymarket.com";
const PING_INTERVAL_SECS: u64 = 5;
const RECONNECT_DELAY_SECS: u64 = 5;
const MAX_SEEN: usize = 10_000;
const PING_MSG: &str = "ping";

pub type NotifyTx = broadcast::Sender<()>;

fn is_eth_address(s: &str) -> bool {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn payload_proxy_or_owner(payload: &serde_json::Value) -> Option<String> {
    let proxy = payload
        .get("proxyWallet")
        .or_else(|| payload.get("proxy_wallet"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    if proxy.is_some() {
        return proxy;
    }
    for key in ["owner", "maker", "makerAddress", "user"] {
        if let Some(v) = payload.get(key).and_then(|v| v.as_str()) {
            let lower = v.trim().to_lowercase();
            if is_eth_address(&lower) {
                return Some(lower);
            }
        }
    }
    None
}

fn parse_payload_timestamp_ms(trade: &LeaderTrade) -> Option<i64> {
    let match_ts = trade.match_time.parse::<i64>().ok()?;
    // The feed sometimes sends seconds, sometimes milliseconds.
    let match_ms = if match_ts >= 1_000_000_000_000 { match_ts } else { match_ts * 1000 };
    Some(match_ms)
}

fn activity_payload_to_leader_trade(p: &serde_json::Value) -> Option<LeaderTrade> {
    let asset = p.get("asset")
        .or_else(|| p.get("assetId"))
        .or_else(|| p.get("token_id"))
        .and_then(|v| v.as_str())?.to_string();
    let side_raw = p.get("side")
        .or_else(|| p.get("orderSide"))
        .or_else(|| p.get("type"))
        .and_then(|v| v.as_str())?;
    let side = side_raw.to_uppercase();
    let size = p.get("size").and_then(|v| v.as_f64())
        .or_else(|| p.get("size").and_then(|v| v.as_u64().map(|u| u as f64)))
        .or_else(|| p.get("size").and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok())))?;
    let price = p.get("price").and_then(|v| v.as_f64())
        .or_else(|| p.get("price").and_then(|v| v.as_u64().map(|u| u as f64)))
        .or_else(|| p.get("price").and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok())))?;
    let timestamp = p.get("timestamp").and_then(|v| v.as_i64())
        .or_else(|| p.get("timestamp").and_then(|v| v.as_str().and_then(|s| s.parse::<i64>().ok())))
        .unwrap_or(0);
    let tx_hash = p.get("transactionHash").or_else(|| p.get("transaction_hash")).and_then(|v| v.as_str()).unwrap_or("");
    let id = format!("{}{}", tx_hash, timestamp);
    let condition_id = p.get("conditionId").or_else(|| p.get("condition_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let slug = p.get("slug").and_then(|v| v.as_str()).map(String::from);
    let outcome = p.get("outcome").and_then(|v| v.as_str()).map(String::from);
    Some(LeaderTrade {
        id,
        asset_id: asset,
        market: condition_id,
        side,
        size: format!("{}", size),
        price: format!("{}", price),
        match_time: timestamp.to_string(),
        slug,
        outcome,
        end_date: None,
    })
}

async fn run_activity_stream_loop(
    targets_lower: HashSet<String>,
    api: Arc<crate::api::PolymarketApi>,
    config: CopyTradingConfig,
    web_state: web_state::SharedState,
    notify_tx: NotifyTx,
    entries: Arc<Mutex<HashMap<String, crate::copy_trading::Entry>>>,
    simulation: bool,
) -> Result<()> {
    // Optional debug helper: log which proxyWallet values are arriving from the
    // activity feed but don't match our configured targets.
    // Default is off to avoid log spam.
    let log_unmatched_proxies = std::env::var("LOG_UNMATCHED_PROXIES")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let log_unmatched_proxies_limit: usize = std::env::var("LOG_UNMATCHED_PROXIES_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(20);
    let mut unmatched_logged_count: usize = 0;
    let log_match_lag = std::env::var("LOG_MATCH_LAG")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    // Prevent the websocket loop from being blocked by slow order placement.
    let copy_trade_timeout_sec: u64 = std::env::var("COPY_TRADE_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10);

    let copy_trade_concurrency: usize = std::env::var("COPY_TRADE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4)
        .max(1);
    let copy_trade_semaphore = Arc::new(Semaphore::new(copy_trade_concurrency));

    info!("Activity stream | connecting to {}", ACTIVITY_WS_URL);
    // disable_nagle=true: default connect_async leaves Nagle on, which can add tens of ms
    // of delay on small WS frames (trade notifications).
    let connect_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(10),
        connect_async_with_config(ACTIVITY_WS_URL, None, true),
    )
    .await;

    let (ws_stream, _) = match connect_result {
        Ok(r) => r?,
        Err(_) => return Err(anyhow!("Activity stream | connect_async timeout after 10s")),
    };
    let (mut write, mut read) = ws_stream.split();

    const SUBSCRIBE_MSG: &str = r#"{"action":"subscribe","subscriptions":[{"topic":"activity","type":"trades"}]}"#;
    write.send(Message::Text(SUBSCRIBE_MSG.to_string())).await?;
    info!("Activity stream | subscribed to activity/trades");

    let mut seen: HashSet<String> = HashSet::new();
    let mut logged_unknown_proxy: HashSet<String> = HashSet::new();
    let ping_handle = tokio::spawn({
        let mut write = write;
        async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(PING_INTERVAL_SECS)).await;
                if write.send(Message::Text(PING_MSG.to_string())).await.is_err() {
                    break;
                }
            }
        }
    });

    while let Some(msg) = read.next().await {
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => match String::from_utf8(b) {
                Ok(s) => s,
                Err(_) => continue,
            },
            _ => continue,
        };
        if text == "pong" || !text.contains("payload") {
            continue;
        }
        let root: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let topic = root.get("topic").and_then(|v| v.as_str()).unwrap_or("");
        let typ = root.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if topic != "activity" || typ != "trades" {
            continue;
        }
        let payload = match root.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let proxy = match payload_proxy_or_owner(payload) {
            Some(p) => p,
            None => {
                if logged_unknown_proxy.insert("__no_proxy__".to_string()) {
                    let keys: Vec<_> = payload.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                    info!(
                        "Activity payload missing wallet field (proxyWallet/owner). Keys: {:?}. If your target never matches, the feed format may have changed.",
                        keys
                    );
                }
                continue;
            }
        };
        if !targets_lower.contains(&proxy) {
            if log_unmatched_proxies
                && logged_unknown_proxy.insert(proxy.clone())
                && unmatched_logged_count < log_unmatched_proxies_limit
            {
                info!(
                    "Activity from proxy {} is not in your target list (set copy.target_address to the leader's proxyWallet).",
                    proxy
                );
                unmatched_logged_count += 1;
            }
            continue;
        }
        let trade = match activity_payload_to_leader_trade(payload) {
            Some(t) => t,
            None => {
                if logged_unknown_proxy.insert(format!("__parse_fail_{}", proxy)) {
                    warn!(
                        "Matched target {} but could not parse trade (missing asset/side/size/price?). Check payload format.",
                        proxy
                    );
                }
                continue;
            }
        };
        if seen.contains(&trade.id) {
            continue;
        }
        if seen.len() >= MAX_SEEN {
            let mut arr: Vec<_> = seen.drain().collect();
            let start = arr.len().saturating_sub(MAX_SEEN / 2);
            for id in arr.drain(start..) {
                seen.insert(id);
            }
        }
        seen.insert(trade.id.clone());

        if !should_copy_trade(&config, &trade) {
            let slug = trade.slug.as_deref().unwrap_or("?");
            let outcome = trade.outcome.as_deref().unwrap_or("?");
            info!(
                "Filtered | {} {} {} size {} @ {} | {} (entry_trade_sec / revert_trade / trade_sec_from_resolve)",
                trade.side, outcome, slug, trade.size, trade.price, proxy
            );
            // Do not await UI updates on the WS read path — they can stall processing
            // of the global activity feed and make copy-trading feel "late".
            let ws = web_state.clone();
            let nt = notify_tx.clone();
            let side = trade.side.clone();
            let size = trade.size.clone();
            let price = trade.price.clone();
            let slug_s = slug.to_string();
            let outcome_s = outcome.to_string();
            let proxy_s = proxy.clone();
            tokio::spawn(async move {
                web_state::push_trade(
                    ws,
                    "LIVE",
                    &side,
                    &outcome_s,
                    &size,
                    &price,
                    &slug_s,
                    Some(proxy_s.as_str()),
                    Some("filtered"),
                )
                .await;
                let _ = nt.send(());
            });
            continue;
        }

        if log_match_lag {
            if let Some(match_ms) = parse_payload_timestamp_ms(&trade) {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let lag_ms = now_ms.saturating_sub(match_ms);
                let lag_sec = (lag_ms as f64) / 1000.0;
                info!(
                    "Target match lag: {:.1}s (payload_ts_ms={}, now_ts_ms={}) for proxy {}",
                    lag_sec, match_ms, now_ms, proxy
                );
            }
        }

        if simulation {
            let slug = trade.slug.as_deref().unwrap_or("?");
            let outcome = trade.outcome.as_deref().unwrap_or("?");
            info!(
                "SIM | {} {} {} size {} @ {} | {} skipped",
                trade.side, outcome, slug, trade.size, trade.price, proxy
            );
            let ws = web_state.clone();
            let nt = notify_tx.clone();
            let side = trade.side.clone();
            let size = trade.size.clone();
            let price = trade.price.clone();
            let slug_s = slug.to_string();
            let outcome_s = outcome.to_string();
            let proxy_s = proxy.clone();
            tokio::spawn(async move {
                web_state::push_trade(
                    ws,
                    "SIM",
                    &side,
                    &outcome_s,
                    &size,
                    &price,
                    &slug_s,
                    Some(proxy_s.as_str()),
                    Some("skipped"),
                )
                .await;
                let _ = nt.send(());
            });
            continue;
        }

        let slug_pre = trade.slug.as_deref().unwrap_or("?");
        let outcome_pre = trade.outcome.as_deref().unwrap_or("?");
        info!(
            "Copy | {} {} {} size {} @ {} | target {}",
            trade.side, outcome_pre, slug_pre, trade.size, trade.price, proxy
        );

        // IMPORTANT: do not await order placement inside the websocket read loop.
        // Otherwise, slow/failed HTTP calls can make target detection appear delayed.
        let multiplier = config.copy.size_multiplier;
        let buy_amount_limit_usd = config.filter.buy_amount_limit_in_usd;

        let api_cl = api.clone();
        let web_state_cl = web_state.clone();
        let notify_tx_cl = notify_tx.clone();
        let entries_cl = entries.clone();
        let semaphore_cl = copy_trade_semaphore.clone();

        let trade_task = trade;
        let proxy_task = proxy;

        tokio::spawn(async move {
            // Limit in-flight copy executions so we don't overwhelm the CLOB API.
            let permit = semaphore_cl.acquire_owned().await;
            if permit.is_err() {
                return;
            }
            let _permit = permit.ok();

            let slug = trade_task.slug.as_deref().unwrap_or("?");
            let outcome = trade_task.outcome.as_deref().unwrap_or("?");

            let copy_fut = copy_trade(
                &api_cl,
                &trade_task,
                multiplier,
                buy_amount_limit_usd,
            );

            match tokio::time::timeout(
                tokio::time::Duration::from_secs(copy_trade_timeout_sec),
                copy_fut,
            )
            .await
            {
                Err(_) => {
                    warn!(
                        "Copy timed out after {}s | {} {} {} @ {} | target {}",
                        copy_trade_timeout_sec,
                        trade_task.side,
                        slug,
                        outcome,
                        trade_task.size,
                        proxy_task
                    );
                    let _ = web_state::push_trade(
                        web_state_cl,
                        "LIVE",
                        &trade_task.side,
                        outcome,
                        &trade_task.size,
                        &trade_task.price,
                        slug,
                        Some(proxy_task.as_str()),
                        Some("timeout"),
                    )
                    .await;
                    let _ = notify_tx_cl.send(());
                }
                Ok(Ok(Some((size, price)))) => {
                    info!(
                        "Copy done | {} {} | filled ~{} @ {}",
                        trade_task.side, slug, size, price
                    );
                    // Track entries only for BUY fills; SELL fills should not add/average entries.
                    if trade_task.side == "BUY" {
                        let mut ent = entries_cl.lock().await;
                        record_entry(&mut *ent, &trade_task.asset_id, size, price);
                    }
                    info!(
                        "LIVE | {} {} {} size {} @ {} | from {} | ok",
                        trade_task.side, outcome, slug, trade_task.size, trade_task.price, proxy_task
                    );
                    let _ = web_state::push_trade(
                        web_state_cl,
                        "LIVE",
                        &trade_task.side,
                        outcome,
                        &trade_task.size,
                        &trade_task.price,
                        slug,
                        Some(proxy_task.as_str()),
                        Some("ok"),
                    )
                    .await;
                    let _ = notify_tx_cl.send(());
                }
                Ok(Ok(None)) => {
                    warn!(
                        "Copy skipped | {} {} size {} @ {} | target {} (size/price zero or below buy_amount_limit?)",
                        trade_task.side,
                        slug,
                        trade_task.size,
                        trade_task.price,
                        proxy_task
                    );
                    let _ = web_state::push_trade(
                        web_state_cl,
                        "LIVE",
                        &trade_task.side,
                        outcome,
                        &trade_task.size,
                        &trade_task.price,
                        slug,
                        Some(proxy_task.as_str()),
                        Some("skipped (size/limit)"),
                    )
                    .await;
                    let _ = notify_tx_cl.send(());
                }
                Ok(Err(e)) => {
                    warn!(
                        "LIVE | {} {} | from {} | FAILED: {}",
                        trade_task.side, slug, proxy_task, e
                    );
                    let copy_status = format!("FAILED: {}", e);
                    let _ = web_state::push_trade(
                        web_state_cl,
                        "LIVE",
                        &trade_task.side,
                        outcome,
                        &trade_task.size,
                        &trade_task.price,
                        slug,
                        Some(proxy_task.as_str()),
                        Some(copy_status.as_str()),
                    )
                    .await;
                    let _ = notify_tx_cl.send(());
                }
            }
        });
    }
    ping_handle.abort();
    Err(anyhow!("WebSocket stream ended"))
}

pub fn spawn_activity_stream(
    targets: Vec<String>,
    api: Arc<crate::api::PolymarketApi>,
    config: CopyTradingConfig,
    web_state: web_state::SharedState,
    notify_tx: NotifyTx,
    entries: Arc<Mutex<HashMap<String, crate::copy_trading::Entry>>>,
    simulation: bool,
) {
    let targets_lower: HashSet<String> = targets.iter().map(|s| s.to_lowercase()).collect();
    let n = targets_lower.len();
    info!(
        "Activity stream | {} target(s) (instant trades via WebSocket)",
        n
    );
    tokio::spawn(async move {
        loop {
            match run_activity_stream_loop(
                targets_lower.clone(),
                api.clone(),
                config.clone(),
                web_state.clone(),
                notify_tx.clone(),
                entries.clone(),
                simulation,
            )
            .await
            {
                Ok(()) => {}
                Err(e) => {
                    warn!(
                        "Activity stream error: {} - reconnecting in {}s",
                        e, RECONNECT_DELAY_SECS
                    );
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(RECONNECT_DELAY_SECS)).await;
        }
    });
}
