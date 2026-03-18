use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::copy_trading::{
    copy_trade, record_entry, should_copy_trade, CopyTradingConfig, LeaderTrade,
};
use crate::web_state;
use std::collections::HashMap;

const ACTIVITY_WS_URL: &str = "wss://ws-live-data.polymarket.com";
const PING_INTERVAL_SECS: u64 = 5;
const RECONNECT_DELAY_SECS: u64 = 5;
const MAX_SEEN: usize = 10_000;
const PING_MSG: &str = "ping";

pub type NotifyTx = broadcast::Sender<()>;

fn activity_payload_to_leader_trade(p: &serde_json::Value) -> Option<LeaderTrade> {
    let asset = p.get("asset")?.as_str()?.to_string();
    let side = p.get("side")?.as_str()?.to_string();
    let size = p.get("size").and_then(|v| v.as_f64()).or_else(|| p.get("size").and_then(|v| v.as_u64().map(|u| u as f64)))?;
    let price = p.get("price").and_then(|v| v.as_f64()).or_else(|| p.get("price").and_then(|v| v.as_u64().map(|u| u as f64)))?;
    let timestamp = p.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
    let tx_hash = p.get("transactionHash").and_then(|v| v.as_str()).unwrap_or("");
    let id = format!("{}{}", tx_hash, timestamp);
    let condition_id = p.get("conditionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
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
    let (ws_stream, _) = connect_async(ACTIVITY_WS_URL).await?;
    let (mut write, mut read) = ws_stream.split();

    const SUBSCRIBE_MSG: &str = r#"{"action":"subscribe","subscriptions":[{"topic":"activity","type":"trades"}]}"#;
    write.send(Message::Text(SUBSCRIBE_MSG.to_string())).await?;

    let mut seen: HashSet<String> = HashSet::new();
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
        let proxy = payload
            .get("proxyWallet")
            .or_else(|| payload.get("proxy_wallet"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase());
        let proxy = match proxy {
            Some(p) => p,
            None => continue,
        };
        if !targets_lower.contains(&proxy) {
            continue;
        }
        let trade = match activity_payload_to_leader_trade(payload) {
            Some(t) => t,
            None => continue,
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
            web_state::push_trade(
                web_state.clone(),
                "LIVE",
                &trade.side,
                outcome,
                &trade.size,
                &trade.price,
                slug,
                Some(proxy.as_str()),
                Some("filtered"),
            )
            .await;
            let _ = notify_tx.send(());
            continue;
        }

        if simulation {
            let slug = trade.slug.as_deref().unwrap_or("?");
            let outcome = trade.outcome.as_deref().unwrap_or("?");
            info!(
                "SIM | {} {} {} size {} @ {} | {} skipped",
                trade.side, outcome, slug, trade.size, trade.price, proxy
            );
            web_state::push_trade(
                web_state.clone(),
                "SIM",
                &trade.side,
                outcome,
                &trade.size,
                &trade.price,
                slug,
                Some(proxy.as_str()),
                Some("skipped"),
            )
            .await;
            let _ = notify_tx.send(());
            continue;
        }

        match copy_trade(
            &api,
            &trade,
            config.copy.size_multiplier,
            config.filter.buy_amount_limit_in_usd,
        )
        .await
        {
            Ok(Some((size, price))) => {
                {
                    let mut ent = entries.lock().await;
                    record_entry(&mut *ent, &trade.asset_id, size, price);
                }
                let slug = trade.slug.as_deref().unwrap_or("?");
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                info!(
                    "LIVE | {} {} {} size {} @ {} | from {} | ok",
                    trade.side, outcome, slug, trade.size, trade.price, proxy
                );
                web_state::push_trade(
                    web_state.clone(),
                    "LIVE",
                    &trade.side,
                    outcome,
                    &trade.size,
                    &trade.price,
                    slug,
                    Some(proxy.as_str()),
                    Some("ok"),
                )
                .await;
                let _ = notify_tx.send(());
            }
            Ok(None) => {
                let slug = trade.slug.as_deref().unwrap_or("?");
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                info!(
                    "LIVE | {} {} {} size {} @ {} | from {} | ok",
                    trade.side, outcome, slug, trade.size, trade.price, proxy
                );
                web_state::push_trade(
                    web_state.clone(),
                    "LIVE",
                    &trade.side,
                    outcome,
                    &trade.size,
                    &trade.price,
                    slug,
                    Some(proxy.as_str()),
                    Some("ok"),
                )
                .await;
                let _ = notify_tx.send(());
            }
            Err(e) => {
                let slug = trade.slug.as_deref().unwrap_or("?");
                warn!("LIVE | {} {} | from {} | FAILED: {}", trade.side, slug, proxy, e);
                let outcome = trade.outcome.as_deref().unwrap_or("?");
                let copy_status = format!("FAILED: {}", e);
                web_state::push_trade(
                    web_state.clone(),
                    "LIVE",
                    &trade.side,
                    outcome,
                    &trade.size,
                    &trade.price,
                    slug,
                    Some(proxy.as_str()),
                    Some(copy_status.as_str()),
                )
                .await;
                let _ = notify_tx.send(());
            }
        }
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
