use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value_t = true)]
    pub simulation: bool,

    #[arg(long)]
    pub no_simulation: bool,

    #[arg(long)]
    pub backtest: bool,

    #[arg(long)]
    pub history_file: Option<PathBuf>,

    #[arg(short, long, default_value = "config.json")]
    pub config: PathBuf,
}

impl Args {
    pub fn is_simulation(&self) -> bool {
        if self.backtest {
            false // Backtest is not simulation (it's a separate mode)
        } else if self.no_simulation {
            false
        } else {
            self.simulation
        }
    }

    pub fn is_backtest(&self) -> bool {
        self.backtest
    }

    pub fn is_history_replay(&self) -> bool {
        self.history_file.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub polymarket: PolymarketConfig,
    #[serde(default)]
    pub trading: TradingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PolymarketConfig {
    pub gamma_api_url: String,
    pub clob_api_url: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
    pub private_key: Option<String>,
    pub proxy_wallet_address: Option<String>,
    pub signature_type: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TradingConfig {
    pub eth_condition_id: Option<String>,
    pub btc_condition_id: Option<String>,
    pub solana_condition_id: Option<String>,
    pub xrp_condition_id: Option<String>,
    pub check_interval_ms: u64,
    pub fixed_trade_amount: f64,
    pub trigger_price: f64,
    pub min_elapsed_minutes: u64,
    pub sell_price: f64,
    pub max_buy_price: Option<f64>,
    pub stop_loss_price: Option<f64>,
    pub hedge_price: Option<f64>,
    pub market_closure_check_interval_seconds: u64,
    pub min_time_remaining_seconds: Option<u64>,
    pub enable_btc_trading: bool,
    pub enable_eth_trading: bool,
    pub enable_solana_trading: bool,
    pub enable_xrp_trading: bool,
    pub dual_limit_price: Option<f64>,
    pub dual_limit_shares: Option<f64>,
    pub dual_limit_hedge_after_minutes: Option<u64>,
    pub dual_limit_hedge_price: Option<f64>,
    pub dual_limit_early_hedge_minutes: Option<u64>,
    pub dual_limit_trend_strength_threshold: Option<f64>,
    pub dual_limit_trend_price_buffer: Option<f64>,
    pub dual_limit_trend_history_size: Option<usize>,
    pub dual_filled_limit_sell_enabled: Option<bool>,
    pub trailing_stop_point: Option<f64>,
    pub dual_limit_hedge_trailing_stop: Option<f64>,
    pub slug: Option<String>,
    pub continuous: bool,
    pub trailing_shares: Option<f64>,
    pub dual_limit_trailing_buy_mode: Option<bool>,
}

impl Default for PolymarketConfig {
    fn default() -> Self {
        Self {
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
            clob_api_url: "https://clob.polymarket.com".to_string(),
            api_key: None,
            api_secret: None,
            api_passphrase: None,
            private_key: None,
            proxy_wallet_address: None,
            signature_type: None,
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            eth_condition_id: None,
            btc_condition_id: None,
            solana_condition_id: None,
            xrp_condition_id: None,
            check_interval_ms: 1000,
            fixed_trade_amount: 1.0,
            trigger_price: 0.9,
            min_elapsed_minutes: 10,
            sell_price: 0.99,
            max_buy_price: Some(0.95),
            stop_loss_price: Some(0.85),
            hedge_price: Some(0.5),
            market_closure_check_interval_seconds: 10,
            min_time_remaining_seconds: Some(30),
            enable_btc_trading: true,
            enable_eth_trading: true,
            enable_solana_trading: false,
            enable_xrp_trading: false,
            dual_limit_price: None,
            dual_limit_shares: None,
            dual_limit_hedge_after_minutes: Some(10),
            dual_limit_hedge_price: Some(0.85),
            dual_limit_early_hedge_minutes: Some(5),
            dual_limit_trend_strength_threshold: Some(0.3),
            dual_limit_trend_price_buffer: Some(0.05),
            dual_limit_trend_history_size: Some(60),
            dual_filled_limit_sell_enabled: None,
            slug: None,
            continuous: false,
            trailing_stop_point: Some(0.03),
            dual_limit_hedge_trailing_stop: Some(0.03),
            trailing_shares: Some(10.0),
            dual_limit_trailing_buy_mode: Some(false),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            polymarket: PolymarketConfig::default(),
            trading: TradingConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            let config = Config::default();
            let content = serde_json::to_string_pretty(&config)?;
            std::fs::write(path, content)?;
            Ok(config)
        }
    }
}

