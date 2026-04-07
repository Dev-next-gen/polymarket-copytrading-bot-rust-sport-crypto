use anyhow::{anyhow, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{broadcast, Mutex, Semaphore};
use tokio_tungstenite::{connect_async_with_config, tungstenite::Message};

use crate::copy_trading::{
    copy_trade, record_entry, should_copy_trade, trade_market_dedupe_key, CopyTradingConfig,
    LeaderTrade,
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

/// Normalize to `0x` + 40 hex (lowercase) for set lookup.
fn normalize_wallet_str(s: &str) -> Option<String> {
    let s = s.trim();
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if hex.len() != 40 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}", hex.to_lowercase()))
}

/// First wallet field present (for logs when nothing matches targets).
fn payload_proxy_or_owner(payload: &serde_json::Value) -> Option<String> {
    for key in [
        "proxyWallet",
        "proxy_wallet",
        "userAddress",
        "user_address",
        "maker",
        "makerAddress",
        "maker_address",
        "taker",
        "takerAddress",
        "owner",
        "user",
        "trader",
        "from",
        "wallet",
        "address",
    ] {
        if let Some(v) = payload.get(key).and_then(|v| v.as_str()) {
            if let Some(w) = normalize_wallet_str(v) {
                return Some(w);
            }
        }
    }
    None
}

/// Match activity payload to configured targets: any known wallet field may be the leader (EOA vs proxy).
fn activity_wallet_matching_target(
    payload: &serde_json::Value,
    targets: &HashSet<String>,
) -> Option<String> {
    for key in [
        "proxyWallet",
        "proxy_wallet",
        "userAddress",
        "user_address",
        "maker",
        "makerAddress",
        "maker_address",
        "taker",
        "takerAddress",
        "owner",
        "user",
        "trader",
        "from",
        "wallet",
        "address",
    ] {
        if let Some(v) = payload.get(key).and_then(|v| v.as_str()) {
            if let Some(w) = normalize_wallet_str(v) {
                if targets.contains(&w) {
                    return Some(w);
                }
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
    // Include asset+side+size+price to avoid id="0" collisions when tx_hash and timestamp are both absent.
    let id = if tx_hash.len() >= 8 {
        format!("tx:{}", tx_hash.to_lowercase())
    } else {
        format!("{}|{}|{}|{}|{}", asset, side_raw.to_uppercase(), size, price, timestamp)
    };
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
    once_per_slug_addrs: HashSet<String>,
    once_per_slug_seen: Arc<StdMutex<HashSet<(String, String)>>>,
    copy_trade_concurrency: usize,
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

    // Subscribe per-user so we receive ALL activity for each target reliably.
    // A global subscription (no "user" field) delivers a sampled firehose that
    // can miss individual users' fills entirely.
    let mut subscriptions = Vec::new();
    for addr in &targets_lower {
        subscriptions.push(serde_json::json!({"topic": "activity", "type": "trades", "user": addr}));
        subscriptions.push(serde_json::json!({"topic": "activity", "type": "orders_matched", "user": addr}));
    }
    // Also keep a global fallback subscription in case the user-specific ones don't trigger
    // for wallet-format mismatches (e.g. EOA vs proxy wallet).
    subscriptions.push(serde_json::json!({"topic": "activity", "type": "trades"}));
    subscriptions.push(serde_json::json!({"topic": "activity", "type": "orders_matched"}));

    let subscribe = serde_json::json!({
        "action": "subscribe",
        "subscriptions": subscriptions
    });
    write
        .send(Message::Text(subscribe.to_string()))
        .await?;
    info!(
        "Activity stream | subscribed to activity/trades+orders_matched for {} target(s) (+ global fallback)",
        targets_lower.len()
    );

    // Local seen-set: lock-free, lives only for this WS connection lifetime.
    let mut seen: HashSet<String> = HashSet::new();
    let mut seen_order: VecDeque<String> = VecDeque::new();
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
        if topic != "activity" || (typ != "trades" && typ != "orders_matched") {
            continue;
        }
        let raw_payload = match root.get("payload") {
            Some(p) => p.clone(),
            None => continue,
        };

        // Polymarket sometimes sends payload as an array of trade objects.
        // Normalise to a Vec so the same handling covers both shapes.
        let items: Vec<&serde_json::Value> = if raw_payload.is_array() {
            raw_payload.as_array().unwrap().iter().collect()
        } else {
            vec![&raw_payload]
        };

        'item: for payload in items {
        let proxy = match activity_wallet_matching_target(payload, &targets_lower) {
            Some(w) => w,
            None => {
                if let Some(primary) = payload_proxy_or_owner(payload) {
                    if log_unmatched_proxies
                        && logged_unknown_proxy.insert(primary.clone())
                        && unmatched_logged_count < log_unmatched_proxies_limit
                    {
                        info!(
                            "Activity from wallet {} (type={}) is not in your target list. \
                            Use the same address Polymarket shows for that trader (often proxyWallet on profile / activity). \
                            Set LOG_UNMATCHED_PROXIES=1 to sample more.",
                            primary,
                            typ
                        );
                        unmatched_logged_count += 1;
                    }
                } else if logged_unknown_proxy.insert("__no_wallet__".to_string()) {
                    let keys: Vec<_> = payload
                        .as_object()
                        .map(|o| o.keys().cloned().collect())
                        .unwrap_or_default();
                    info!(
                        "Activity payload missing wallet fields (type={}). Keys: {:?}. Feed format may have changed.",
                        typ, keys
                    );
                }
                continue 'item;
            }
        };
        let trade = match activity_payload_to_leader_trade(payload) {
            Some(t) => t,
            None => {
                // Always warn (not once-per-proxy) so parse failures stay visible.
                warn!(
                    "Matched target {} but could not parse trade (missing asset/side/size/price). \
                    Payload keys: {:?}",
                    proxy,
                    payload.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()
                );
                continue 'item;
            }
        };
        // Local lock-free dedupe (no mutex — keeps the WS read loop non-blocking).
        let feed_dedupe = format!("{}|{}", proxy, trade.id);
        if seen.contains(&trade.id) || seen.contains(&feed_dedupe) {
            continue 'item;
        }
        if seen.len() >= MAX_SEEN {
            let evict = seen_order.len() / 2;
            for _ in 0..evict {
                if let Some(old) = seen_order.pop_front() {
                    seen.remove(&old);
                }
            }
        }
        seen.insert(trade.id.clone());
        seen_order.push_back(trade.id.clone());
        seen.insert(feed_dedupe.clone());
        seen_order.push_back(feed_dedupe);

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

        // One BUY per market (slug / condition / token) for selected leaders only.
        if trade.side == "BUY" && once_per_slug_addrs.contains(&proxy) {
            let mkey = trade_market_dedupe_key(&trade);
            let mut slug_dedupe = once_per_slug_seen.lock().unwrap();
            if !slug_dedupe.insert((proxy.clone(), mkey.clone())) {
                let slug = trade.slug.as_deref().unwrap_or("?");
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                info!(
                    "Filtered | {} {} {} size {} @ {} | {} (once_per_slug; market_key={})",
                    trade.side, outcome, slug, trade.size, trade.price, proxy, mkey
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
                        "LIVE",
                        &side,
                        &outcome_s,
                        &size,
                        &price,
                        &slug_s,
                        Some(proxy_s.as_str()),
                        Some("once_per_slug"),
                    )
                    .await;
                    let _ = nt.send(());
                });
                continue;
            }
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
        let copy_fixed_usd = config.copy.copy_fixed_usd;

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
                copy_fixed_usd,
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
                    // Print full error chain so the root cause is visible in logs.
                    let root_cause = e.chain()
                        .skip(1)
                        .next()
                        .map(|c| format!(": {}", c))
                        .unwrap_or_default();
                    warn!(
                        "LIVE | {} {} | from {} | FAILED: {}{}",
                        trade_task.side, slug, proxy_task, e, root_cause
                    );
                    let copy_status = format!("FAILED: {}{}", e, root_cause);
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
        } // end 'item loop
    }
    ping_handle.abort();
    Err(anyhow!("WebSocket stream ended"))
}

/// How often the REST polling fallback checks for missed fills (seconds).
/// Override with env `POLL_FALLBACK_INTERVAL_SEC` (min 5, default 15).
const POLL_FALLBACK_INTERVAL_SEC: u64 = 15;
/// How many recent trades to fetch per target per poll cycle.
const POLL_FALLBACK_LIMIT: u32 = 20;

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
    let once_per_slug_addrs = config.once_per_slug_addresses();
    let once_per_slug_seen: Arc<StdMutex<HashSet<(String, String)>>> =
        Arc::new(StdMutex::new(HashSet::new()));

    let n = targets_lower.len();
    let n_once = once_per_slug_addrs.len();
    let target_n = n.max(1);
    let default_conc = (target_n.saturating_mul(8)).clamp(16, 128);
    let copy_trade_concurrency = std::env::var("COPY_TRADE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .or_else(|| config.copy.copy_trade_concurrency.map(|u| u as usize))
        .unwrap_or(default_conc)
        .max(1);
    info!(
        "Activity stream | {} target(s) (instant trades via WebSocket + REST fallback poll every {}s); once_per_slug for {} address(es); copy concurrency {}",
        n, POLL_FALLBACK_INTERVAL_SEC, n_once, copy_trade_concurrency
    );

    // ── WebSocket loop ───────────────────────────────────────────────────────
    tokio::spawn({
        let targets_lower = targets_lower.clone();
        let once_per_slug_addrs = once_per_slug_addrs.clone();
        let once_per_slug_seen = once_per_slug_seen.clone();
        let api = api.clone();
        let config = config.clone();
        let web_state = web_state.clone();
        let notify_tx = notify_tx.clone();
        let entries = entries.clone();
        async move {
            loop {
                match run_activity_stream_loop(
                    targets_lower.clone(),
                    once_per_slug_addrs.clone(),
                    once_per_slug_seen.clone(),
                    copy_trade_concurrency,
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
        }
    });

    // ── REST polling fallback ────────────────────────────────────────────────
    // Catches any trades the WebSocket missed (sampled feed, brief disconnects, etc.)
    // Uses its OWN local seen-set — no mutex shared with the WS loop, so the WS
    // hot path is never blocked.  once_per_slug_seen (already shared) prevents
    // double-copying for BUY orders.
    let poll_interval_sec = std::env::var("POLL_FALLBACK_INTERVAL_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(POLL_FALLBACK_INTERVAL_SEC)
        .max(5);
    // Only copy trades that happened after the bot started.  Trades from
    // older resolved markets have no orderbook and cannot be filled.
    let bot_start_ms = chrono::Utc::now().timestamp_millis();
    tokio::spawn(async move {
        let copy_trade_semaphore = Arc::new(Semaphore::new(copy_trade_concurrency));
        let mut poll_seen: HashSet<String> = HashSet::new();
        let mut poll_seen_order: VecDeque<String> = VecDeque::new();
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval_sec)).await;
            for proxy in &targets_lower {
                // Any error in a single poll cycle is logged and skipped; the
                // loop always continues to the next target / next interval.
                let trades_raw = match api.get_user_recent_trades(proxy, POLL_FALLBACK_LIMIT).await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("REST fallback poll for {} failed: {}", proxy, e);
                        continue;
                    }
                };
                for item in &trades_raw {
                    // Silently skip unparseable items — never stop the loop.
                    let trade = match activity_payload_to_leader_trade(item) {
                        Some(t) => t,
                        None => continue,
                    };

                    // Skip trades that pre-date bot startup; their markets are
                    // likely already resolved / have no orderbook.
                    let trade_ms = trade.match_time.parse::<i64>().unwrap_or(0);
                    let trade_ms_norm = if trade_ms >= 1_000_000_000_000 { trade_ms } else { trade_ms * 1000 };
                    if trade_ms_norm > 0 && trade_ms_norm < bot_start_ms {
                        continue;
                    }

                    let feed_dedupe = format!("{}|{}", proxy, trade.id);
                    if poll_seen.contains(&trade.id) || poll_seen.contains(&feed_dedupe) {
                        continue;
                    }
                    if poll_seen.len() >= MAX_SEEN {
                        let evict = poll_seen_order.len() / 2;
                        for _ in 0..evict {
                            if let Some(old) = poll_seen_order.pop_front() {
                                poll_seen.remove(&old);
                            }
                        }
                    }
                    poll_seen.insert(trade.id.clone());
                    poll_seen_order.push_back(trade.id.clone());
                    poll_seen.insert(feed_dedupe.clone());
                    poll_seen_order.push_back(feed_dedupe);

                    if !should_copy_trade(&config, &trade) {
                        continue;
                    }
                    if trade.side == "BUY" && once_per_slug_addrs.contains(proxy) {
                        let mkey = trade_market_dedupe_key(&trade);
                        if !once_per_slug_seen.lock().unwrap().insert((proxy.clone(), mkey)) {
                            continue;
                        }
                    }
                    let slug = trade.slug.as_deref().unwrap_or("?");
                    let outcome = trade.outcome.as_deref().unwrap_or("?");
                    info!(
                        "REST fallback | {} {} {} size {} @ {} | target {}",
                        trade.side, outcome, slug, trade.size, trade.price, proxy
                    );
                    if simulation {
                        continue;
                    }
                    let multiplier = config.copy.size_multiplier;
                    let buy_limit = config.filter.buy_amount_limit_in_usd;
                    let copy_fixed = config.copy.copy_fixed_usd;
                    let api_cl = api.clone();
                    let web_state_cl = web_state.clone();
                    let notify_tx_cl = notify_tx.clone();
                    let semaphore_cl = copy_trade_semaphore.clone();
                    let trade_task = trade;
                    let proxy_task = proxy.clone();
                    tokio::spawn(async move {
                        let permit = semaphore_cl.acquire_owned().await;
                        if permit.is_err() { return; }
                        let _permit = permit.ok();
                        let slug = trade_task.slug.as_deref().unwrap_or("?");
                        let outcome = trade_task.outcome.as_deref().unwrap_or("?");
                        match copy_trade(&api_cl, &trade_task, multiplier, buy_limit, copy_fixed).await {
                            Ok(Some(_)) => {
                                info!("REST fallback copy done | {} {} | from {}", trade_task.side, slug, proxy_task);
                                let _ = web_state::push_trade(web_state_cl, "LIVE", &trade_task.side, outcome,
                                    &trade_task.size, &trade_task.price, slug, Some(proxy_task.as_str()), Some("ok (poll)")).await;
                                let _ = notify_tx_cl.send(());
                            }
                            Ok(None) => {}
                            Err(e) => {
                                warn!("REST fallback copy failed | {} {} | from {} | {}", trade_task.side, slug, proxy_task, e);
                            }
                        }
                    });
                }
            }
        }
    });
}
