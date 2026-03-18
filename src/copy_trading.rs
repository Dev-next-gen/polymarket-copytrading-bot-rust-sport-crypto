use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::prelude::{ToPrimitive};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::{DataApiPosition, PolymarketApi};


// ---------- Config (trade.toml) ----------

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CopyTradingConfig {
    #[serde(default = "default_clob_host")]
    pub clob_host: String,
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
    #[serde(default)]
    pub simulation: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub copy: CopySection,
    #[serde(default)]
    pub filter: FilterSection,
    #[serde(default)]
    pub exit: ExitSection,
    #[serde(default)]
    pub ui: UiSection,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct UiSection {
    #[serde(default = "default_delta_highlight")]
    pub delta_highlight_sec: u64,
    #[serde(default = "default_delta_animation")]
    pub delta_animation_sec: u64,
}
fn default_delta_highlight() -> u64 {
    10
}
fn default_delta_animation() -> u64 {
    2
}

fn default_clob_host() -> String {
    "https://clob.polymarket.com".to_string()
}
fn default_chain_id() -> u64 {
    137
}
fn default_port() -> u16 {
    8000
}


#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CopySection {
    #[serde(alias = "target_address", alias = "target_addresses")]
    pub target_addresses: Option<toml::Value>,
    #[serde(default = "default_true")]
    pub revert_trade: bool,
    #[serde(default = "default_multiplier")]
    pub size_multiplier: f64,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: f64,
}

fn default_true() -> bool {
    true
}
fn default_multiplier() -> f64 {
    1.0
}
fn default_poll_interval() -> f64 {
    5.0
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FilterSection {
    #[serde(default)]
    pub buy_amount_limit_in_usd: f64,
    #[serde(default)]
    pub entry_trade_sec: u64,
    #[serde(default)]
    pub trade_sec_from_resolve: u64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExitSection {
    #[serde(default)]
    pub take_profit: f64,
    #[serde(default)]
    pub stop_loss: f64,
    #[serde(default)]
    pub trailing_stop: f64,
}

impl CopyTradingConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path).context("Failed to read trade.toml")?;
        toml::from_str(&s).context("Failed to parse trade.toml")
    }

    pub fn target_addresses(&self) -> Vec<String> {
        let raw = match self.copy.target_addresses.as_ref() {
            Some(v) => v.clone(),
            None => return Vec::new(),
        };
        if let Some(arr) = raw.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = raw.as_str() {
            vec![s.to_string()]
        } else {
            Vec::new()
        }
    }
}

// ---------- Leader trade (from position diff) ----------

#[derive(Debug, Clone)]
pub struct LeaderTrade {
    pub id: String,
    pub asset_id: String,
    pub market: String,
    pub side: String, // "BUY" | "SELL"
    pub size: String,
    pub price: String,
    pub match_time: String,
    pub slug: Option<String>,
    pub outcome: Option<String>,
    pub end_date: Option<String>,
}

// ---------- Filter ----------

pub fn should_copy_trade(config: &CopyTradingConfig, trade: &LeaderTrade) -> bool {
    if trade.side == "SELL" && !config.copy.revert_trade {
        return false;
    }
    if config.filter.entry_trade_sec > 0 {
        let match_ts = trade
            .match_time
            .parse::<i64>()
            .unwrap_or(0);
        let match_ms = if match_ts >= 1_000_000_000_000 {
            match_ts
        } else {
            match_ts * 1000
        };
        let age_sec = (Utc::now().timestamp_millis() - match_ms) / 1000;
        if match_ts > 0 && age_sec > config.filter.entry_trade_sec as i64 {
            return false;
        }
    }
    if config.filter.trade_sec_from_resolve > 0 {
        if let Some(ref end_date) = trade.end_date {
            if let Ok(dt) = DateTime::parse_from_rfc3339(end_date) {
                let sec_to_resolve = (dt.timestamp() - Utc::now().timestamp()) as u64;
                if sec_to_resolve < config.filter.trade_sec_from_resolve {
                    return false;
                }
            }
        }
    }
    true
}

// ---------- Copy trade (place market order) ----------

pub async fn copy_trade(
    api: &PolymarketApi,
    trade: &LeaderTrade,
    multiplier: f64,
    buy_amount_limit_usd: f64,
) -> Result<Option<(f64, f64)>> {
    // `trade.size` / `trade.price` are produced from JSON numeric payloads and
    // can sometimes end up in scientific notation (e.g. `1e-7`) depending on formatting.
    // `Decimal::from_str` may fail in those cases, which would make size/price = 0
    // and cause the bot to skip order placement.
    fn parse_dec(s: &str) -> Decimal {
        if let Ok(d) = Decimal::from_str(s) {
            return d;
        }
        match s.parse::<f64>() {
            Ok(f) => Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO),
            Err(_) => Decimal::ZERO,
        }
    }

    let size = parse_dec(&trade.size);
    let price = parse_dec(&trade.price);
    let mult = Decimal::from_f64_retain(multiplier).unwrap_or(Decimal::ONE);

    // Avoid nonsense orders when parsing failed.
    // SELL market orders don't require price; only BUY does.
    if size <= Decimal::ZERO || mult == Decimal::ZERO {
        return Ok(None);
    }
    if trade.side == "BUY" && price <= Decimal::ZERO {
        return Ok(None);
    }

    let (amount_usd_or_shares, size_out, is_buy) = if trade.side == "BUY" {
        let amount_usd = size * price * mult;
        let capped = if buy_amount_limit_usd > 0.0 {
            let limit = Decimal::from_f64_retain(buy_amount_limit_usd).unwrap_or(Decimal::ZERO);
            if limit <= Decimal::ZERO {
                return Ok(None);
            }
            if amount_usd > limit {
                let size_out = limit / price;
                (limit, size_out, true)
            } else {
                (amount_usd, size * mult, true)
            }
        } else {
            (amount_usd, size * mult, true)
        };
        let amt = capped.0.to_f64().unwrap_or(0.0);
        let sz = capped.1.to_f64().unwrap_or(0.0);
        (amt, sz, true)
    } else {
        let amount_shares = size * mult;
        let amt = amount_shares.to_f64().unwrap_or(0.0);
        (amt, amt, false)
    };

    if amount_usd_or_shares <= 0.0 {
        return Ok(None);
    }

    // Try FOK first (stricter), then fall back to FAK to increase fill probability.
    match api
        .place_market_order(&trade.asset_id, amount_usd_or_shares, &trade.side, Some("FOK"))
        .await
    {
        Ok(_) => {}
        Err(_) => {
            api.place_market_order(&trade.asset_id, amount_usd_or_shares, &trade.side, Some("FAK"))
                .await
                .context("place_market_order failed (FOK and fallback FAK)")?;
        }
    }

    if is_buy {
        let price_f = price.to_f64().unwrap_or(0.0);
        Ok(Some((size_out, price_f)))
    } else {
        Ok(None)
    }
}

// ---------- Entry tracking (for exit loop) ----------

#[derive(Debug, Clone)]
pub struct Entry {
    entry_price: Decimal,
    size: Decimal,
    max_price: Decimal,
}

pub fn record_entry(
    entries: &mut HashMap<String, Entry>,
    asset_id: &str,
    size: f64,
    price: f64,
) {
    let size_b = Decimal::from_str(&size.to_string()).unwrap_or(Decimal::ZERO);
    let price_b = Decimal::from_str(&price.to_string()).unwrap_or(Decimal::ZERO);
    if let Some(e) = entries.get_mut(asset_id) {
        let new_size = e.size + size_b;
        e.entry_price = (e.entry_price * e.size + price_b * size_b) / new_size;
        e.size = new_size;
        if price_b > e.max_price {
            e.max_price = price_b;
        }
    } else {
        entries.insert(
            asset_id.to_string(),
            Entry {
                entry_price: price_b,
                size: size_b,
                max_price: price_b,
            },
        );
    }
}

// ---------- Position snapshot (for diff) ----------

#[derive(Debug, Clone, Default)]
pub struct PositionSnapshot {
    size: f64,
    cur_price: f64,
    condition_id: Option<String>,
    end_date: Option<String>,
    slug: Option<String>,
    outcome: Option<String>,
}

pub fn position_snapshot(p: &DataApiPosition) -> PositionSnapshot {
    PositionSnapshot {
        size: p.size,
        cur_price: p.cur_price,
        condition_id: p.condition_id.clone(),
        end_date: p.end_date.clone(),
        slug: p.slug.clone(),
        outcome: p.outcome.clone(),
    }
}

pub type SnapshotMap = HashMap<String, PositionSnapshot>;

pub fn build_snapshot_map(positions: &[DataApiPosition]) -> SnapshotMap {
    let mut m = HashMap::new();
    for p in positions {
        m.insert(p.asset.clone(), position_snapshot(p));
    }
    m
}

pub fn diff_to_trades(
    user: &str,
    curr: &HashMap<String, PositionSnapshot>,
    prev: &HashMap<String, PositionSnapshot>,
) -> Vec<LeaderTrade> {
    let cap = curr.len() + prev.len();
    let mut out = Vec::with_capacity(cap);
    let now = Utc::now().timestamp_millis().to_string();
    for (asset, c) in curr.iter() {
        let s = prev.get(asset).map(|p| p.size).unwrap_or(0.0);
        let delta = c.size - s;
        if delta > 0.0 {
            out.push(LeaderTrade {
                id: format!("{}-{}-{}", user, asset, now),
                asset_id: asset.clone(),
                market: c.condition_id.clone().unwrap_or_default(),
                side: "BUY".to_string(),
                size: format!("{}", delta),
                price: format!("{}", c.cur_price),
                match_time: now.clone(),
                slug: c.slug.clone(),
                outcome: c.outcome.clone(),
                end_date: c.end_date.clone(),
            });
        } else if delta < 0.0 && s > 0.0 {
            out.push(LeaderTrade {
                id: format!("{}-{}-{}", user, asset, now),
                asset_id: asset.clone(),
                market: c.condition_id.clone().unwrap_or_default(),
                side: "SELL".to_string(),
                size: format!("{}", -delta),
                price: format!("{}", c.cur_price),
                match_time: now.clone(),
                slug: c.slug.clone(),
                outcome: c.outcome.clone(),
                end_date: c.end_date.clone(),
            });
        }
    }
    for asset in prev.keys() {
        if !curr.contains_key(asset) {
            if let Some(s) = prev.get(asset) {
                if s.size > 0.0 {
                    out.push(LeaderTrade {
                        id: format!("{}-{}-{}", user, asset, now),
                        asset_id: asset.clone(),
                        market: s.condition_id.clone().unwrap_or_default(),
                        side: "SELL".to_string(),
                        size: format!("{}", s.size),
                        price: format!("{}", s.cur_price),
                        match_time: now.clone(),
                        slug: s.slug.clone(),
                        outcome: s.outcome.clone(),
                        end_date: s.end_date.clone(),
                    });
                }
            }
        }
    }
    out
}

// ---------- Exit loop (take profit / stop loss / trailing stop) ----------

const EXIT_INTERVAL_MS: u64 = 15_000;

pub fn spawn_exit_loop(
    api: Arc<PolymarketApi>,
    config: CopyTradingConfig,
    wallet: String,
    entries: Arc<Mutex<HashMap<String, Entry>>>,
) {
    if config.exit.take_profit <= 0.0
        && config.exit.stop_loss <= 0.0
        && config.exit.trailing_stop <= 0.0
    {
        return;
    }
    let take_profit = config.exit.take_profit;
    let stop_loss = config.exit.stop_loss;
    let trailing_stop = config.exit.trailing_stop;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(EXIT_INTERVAL_MS));
        loop {
            interval.tick().await;
            if let Err(e) = run_exit_check(
                &api,
                &wallet,
                take_profit,
                stop_loss,
                trailing_stop,
                entries.clone(),
            )
            .await
            {
                log::warn!("exit check error: {}", e);
            }
        }
    });
}

async fn run_exit_check(
    api: &PolymarketApi,
    wallet: &str,
    take_profit: f64,
    stop_loss: f64,
    trailing_stop: f64,
    entries: Arc<Mutex<HashMap<String, Entry>>>,
) -> Result<()> {
    let positions = api.get_positions(wallet).await?;

    let to_sell: Vec<(String, Decimal, Decimal)> = {
        let mut ent = entries.lock().await;
        let mut out = Vec::new();
        for p in &positions {
            let entry = match ent.get_mut(&p.asset) {
                Some(e) if e.size > Decimal::ZERO => e,
                _ => continue,
            };
            let cur_price = Decimal::from_str(&p.cur_price.to_string()).unwrap_or(Decimal::ZERO);
            let pos_size = Decimal::from_str(&p.size.to_string()).unwrap_or(Decimal::ZERO);
            let size_b = if entry.size <= pos_size {
                entry.size
            } else {
                pos_size
            };
            if size_b <= Decimal::ZERO {
                continue;
            }
            let pnl_pct = if entry.entry_price > Decimal::ZERO {
                (cur_price - entry.entry_price) / entry.entry_price * Decimal::from(100)
            } else {
                Decimal::ZERO
            };
            let pnl_f = pnl_pct.to_f64().unwrap_or(0.0);
            if cur_price > entry.max_price {
                entry.max_price = cur_price;
            }
            let trail_pct = if entry.max_price > Decimal::ZERO {
                (entry.max_price - cur_price) / entry.max_price * Decimal::from(100)
            } else {
                Decimal::ZERO
            };
            let trail_f = trail_pct.to_f64().unwrap_or(0.0);

            let should_sell = (take_profit > 0.0 && pnl_f >= take_profit)
                || (stop_loss > 0.0 && pnl_f <= -stop_loss)
                || (trailing_stop > 0.0 && trail_f >= trailing_stop);
            if should_sell {
                out.push((p.asset.clone(), size_b, cur_price));
            }
        }
        out
    };

    for (asset, size_b, cur_price) in &to_sell {
        let amount = (size_b * cur_price).to_f64().unwrap_or(0.0);
        api.place_market_order(asset, amount, "SELL", Some("FOK"))
            .await
            .context("exit sell failed")?;
    }

    if !to_sell.is_empty() {
        let mut ent = entries.lock().await;
        for (asset, size_b, _) in to_sell {
            if let Some(e) = ent.get_mut(&asset) {
                e.size = e.size - size_b;
                if e.size <= Decimal::ZERO {
                    ent.remove(&asset);
                }
            }
        }
    }
    Ok(())
}
