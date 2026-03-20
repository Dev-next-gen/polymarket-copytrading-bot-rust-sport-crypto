use crate::models::*;
use anyhow::{Context, Result};
use std::convert::TryFrom;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use base64;
use log::{warn, info, error};
use std::sync::Arc;

use crate::clob_sdk;
use alloy::signers::local::LocalSigner;
use alloy::signers::Signer as _;
use alloy::primitives::Address as AlloyAddress;

// CTF (Conditional Token Framework) imports for redemption
// Based on docs: https://docs.polymarket.com/developers/builders/relayer-client#redeem-positions
use alloy::primitives::{B256, U256, Bytes};
use alloy::primitives::keccak256;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::eth::TransactionRequest;

// Contract interfaces for direct RPC calls (like SDK example)
use alloy::sol;

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IERC1155 {
        function setApprovalForAll(address operator, bool approved) external;
        function isApprovedForAll(address account, address operator) external view returns (bool);
    }

    #[sol(rpc)]
    interface IConditionalTokens {
        function redeemPositions(address collateralToken, bytes32 parentCollectionId, bytes32 conditionId, uint256[] indexSets) external;
    }
}

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct DataApiPosition {
    pub asset: String,
    pub size: f64,
    pub cur_price: f64,
    pub condition_id: Option<String>,
    pub end_date: Option<String>,
    pub slug: Option<String>,
    pub outcome: Option<String>,
}

fn polygon() -> u64 {
    clob_sdk::polygon()
}

pub struct PolymarketApi {
    client: Client,
    gamma_url: String,
    clob_url: String,
    api_key: Option<String>,
    api_secret: Option<String>,
    api_passphrase: Option<String>,
    private_key: Option<String>,
    // Proxy wallet configuration (for Polymarket proxy wallet)
    proxy_wallet_address: Option<String>,
    signature_type: Option<u8>, // 0 = EOA, 1 = Proxy, 2 = GnosisSafe
    // Track if authentication was successful at startup
    authenticated: Arc<tokio::sync::Mutex<bool>>,
    clob_client_state: Arc<tokio::sync::Mutex<ClobClientState>>,
    clob_client_notify: Arc<tokio::sync::Notify>,
}

#[derive(Clone)]
enum ClobClientState {
    Empty,
    Creating,
    Ready(u64),
}

impl PolymarketApi {
    fn normalize_buy_usdc_amount(amount: f64) -> f64 {
        // CLOB market BUY supports max 2 decimals for maker amount.
        // Truncate (not round up) to avoid accidental overspend.
        (amount * 100.0).floor() / 100.0
    }

    fn is_buy_min_size_error(err: &str) -> bool {
        err.contains("invalid amount for a marketable BUY order")
            && err.contains("min size: $1")
    }

    fn polygon_rpc_url() -> String {
        std::env::var("POLYGON_RPC_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://polygon-bor-rpc.publicnode.com".to_string())
    }

    pub fn new(
        gamma_url: String,
        clob_url: String,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        private_key: Option<String>,
        proxy_wallet_address: Option<String>,
        signature_type: Option<u8>,
    ) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            gamma_url,
            clob_url,
            api_key,
            api_secret,
            api_passphrase,
            private_key,
            proxy_wallet_address,
            signature_type,
            authenticated: Arc::new(tokio::sync::Mutex::new(false)),
            clob_client_state: Arc::new(tokio::sync::Mutex::new(ClobClientState::Empty)),
            clob_client_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    async fn ensure_clob_client(&self) -> Result<u64> {
        let notify = Arc::clone(&self.clob_client_notify);

        loop {
            let (is_creator, jh) = {
                let mut guard = self.clob_client_state.lock().await;
                match &*guard {
                    ClobClientState::Ready(h) => return Ok(*h),
                    ClobClientState::Creating => {
                        drop(guard);
                        notify.notified().await;
                        continue;
                    }
                    ClobClientState::Empty => {
                        let clob_url = self.clob_url.clone();
                        let private_key = self.private_key.clone()
                            .ok_or_else(|| anyhow::anyhow!("Private key is required. Please set private_key in config.json"))?;
                        let api_key = self.api_key.clone().unwrap_or_default();
                        let api_secret = self.api_secret.clone().unwrap_or_default();
                        let api_passphrase = self.api_passphrase.clone().unwrap_or_default();
                        let sig_type = match (self.proxy_wallet_address.is_some(), self.signature_type) {
                            (true, Some(0)) => anyhow::bail!("proxy_wallet_address is set but signature_type is 0 (EOA). Use 1 (POLY_PROXY) or 2 (GNOSIS_SAFE)."),
                            (true, None) => {
                                eprintln!("⚠️  proxy_wallet_address set but signature_type not specified; defaulting to 1 (POLY_PROXY)");
                                1u8
                            }
                            (true, Some(1)) | (true, Some(2)) => self.signature_type.unwrap(),
                            (false, Some(0)) | (false, None) => 0u8,
                            (false, Some(1)) | (false, Some(2)) => anyhow::bail!("signature_type {} requires proxy_wallet_address", self.signature_type.unwrap()),
                            (_, Some(n)) => anyhow::bail!("Invalid signature_type: {}. Must be 0, 1, or 2", n),
                        };
                        let funder = self.proxy_wallet_address.as_deref().map(|s| s.trim().to_lowercase());
                        *guard = ClobClientState::Creating;
                        let jh = tokio::task::spawn_blocking(move || {
                            let chain_id = polygon();
                            clob_sdk::client_create(
                                &clob_url,
                                &private_key,
                                chain_id,
                                funder.as_deref(),
                                sig_type,
                                &api_key,
                                &api_secret,
                                &api_passphrase,
                            )
                        });
                        (true, jh)
                    }
                }
            };

            if !is_creator {
                continue;
            }

            let handle = match jh.await {
                Ok(Ok(h)) => h,
                Ok(Err(e)) => {
                    let mut guard = self.clob_client_state.lock().await;
                    *guard = ClobClientState::Empty;
                    return Err(e);
                }
                Err(e) => return Err(anyhow::anyhow!("CLOB client creation task panicked: {}", e)),
            };

            {
                let mut guard = self.clob_client_state.lock().await;
                *guard = ClobClientState::Ready(handle);
            }
            self.clob_client_notify.notify_waiters();
            return Ok(handle);
        }
    }

    pub async fn authenticate(&self) -> Result<()> {
        let _ = self.ensure_clob_client().await?;
        *self.authenticated.lock().await = true;
        eprintln!("✅ Successfully authenticated with Polymarket CLOB API (clob_sdk)");
        eprintln!("   ✓ Private key: Valid");
        eprintln!("   ✓ API credentials: Valid");
        if let Some(proxy_addr) = &self.proxy_wallet_address {
            let normalized = proxy_addr.trim().to_lowercase();
            eprintln!("   ✓ Proxy wallet (funder): {} (normalized for CLOB: {})", proxy_addr, normalized);
            if let Some(st) = self.signature_type {
                eprintln!("   ✓ Signature type: {} (0=EOA, 1=POLY_PROXY, 2=GNOSIS_SAFE)", st);
            }
            // So user can verify which wallet their API key is tied to (must be the proxy, not EOA)
            if let Some(pk) = &self.private_key {
                if let Ok(signer) = LocalSigner::from_str(pk) {
                    eprintln!("   ✓ EOA (from private_key): {}  ← API key must be for the PROXY above, not this EOA", signer.address());
                }
            }
        } else {
            eprintln!("   ✓ Trading account: EOA (private key account)");
        }
        Ok(())
    }

    fn generate_signature(
        &self,
        method: &str,
        path: &str,
        body: &str,
        timestamp: u64,
    ) -> Result<String> {
        let secret = self.api_secret.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API secret is required for authenticated requests"))?;
        
        // Create message: method + path + body + timestamp
        let message = format!("{}{}{}{}", method, path, body, timestamp);
        
        // Try to decode secret from base64url first (Builder API uses base64url encoding)
        // Base64url uses - and _ instead of + and /, making it URL-safe
        // Then try standard base64, then fall back to raw bytes
        let secret_bytes = {
            use base64::engine::general_purpose;
            use base64::Engine;
            
            // First try base64url (URL_SAFE engine)
            if let Ok(bytes) = general_purpose::URL_SAFE.decode(secret) {
                bytes
            }
            // Then try standard base64
            else if let Ok(bytes) = general_purpose::STANDARD.decode(secret) {
                bytes
            }
            // Finally, use raw bytes if both fail
            else {
                secret.as_bytes().to_vec()
            }
        };
        
        // Create HMAC-SHA256 signature
        let mut mac = HmacSha256::new_from_slice(&secret_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to create HMAC: {}", e))?;
        mac.update(message.as_bytes());
        let result = mac.finalize();
        let signature = hex::encode(result.into_bytes());
        
        Ok(signature)
    }

    fn builder_relayer_signature(
        api_secret: &str,
        timestamp_ms: u64,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<String> {
        let message = format!("{}{}{}{}", timestamp_ms, method, path, body);
        let secret_bytes = {
            use base64::engine::general_purpose;
            use base64::Engine;
            if let Ok(b) = general_purpose::URL_SAFE.decode(api_secret) {
                b
            } else if let Ok(b) = general_purpose::STANDARD.decode(api_secret) {
                b
            } else {
                api_secret.as_bytes().to_vec()
            }
        };
        let mut mac = HmacSha256::new_from_slice(&secret_bytes)
            .context("Failed to create HMAC for builder relayer")?;
        mac.update(message.as_bytes());
        let sig = mac.finalize().into_bytes();
        use base64::engine::general_purpose;
        use base64::Engine;
        Ok(general_purpose::URL_SAFE.encode(sig.as_slice()))
    }

    fn add_auth_headers(
        &self,
        request: reqwest::RequestBuilder,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<reqwest::RequestBuilder> {
        // Only add auth headers if we have all required credentials
        if self.api_key.is_none() || self.api_secret.is_none() || self.api_passphrase.is_none() {
            return Ok(request);
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let signature = self.generate_signature(method, path, body, timestamp)?;
        
        let request = request
            .header("POLY_API_KEY", self.api_key.as_ref().unwrap())
            .header("POLY_SIGNATURE", signature)
            .header("POLY_TIMESTAMP", timestamp.to_string())
            .header("POLY_PASSPHRASE", self.api_passphrase.as_ref().unwrap());
        
        Ok(request)
    }

    pub async fn get_all_active_markets(&self, limit: u32) -> Result<Vec<Market>> {
        let url = format!("{}/events", self.gamma_url);
        let limit_str = limit.to_string();
        let mut params = HashMap::new();
        params.insert("active", "true");
        params.insert("closed", "false");
        params.insert("limit", &limit_str);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to fetch all active markets")?;

        let status = response.status();
        let json: Value = response.json().await.context("Failed to parse markets response")?;
        
        if !status.is_success() {
            log::warn!("Get all active markets API returned error status {}: {}", status, serde_json::to_string(&json).unwrap_or_default());
            anyhow::bail!("API returned error status {}: {}", status, serde_json::to_string(&json).unwrap_or_default());
        }
        
        // Extract markets from events - events contain markets
        let mut all_markets = Vec::new();
        
        if let Some(events) = json.as_array() {
            for event in events {
                if let Some(markets) = event.get("markets").and_then(|m| m.as_array()) {
                    for market_json in markets {
                        if let Ok(market) = serde_json::from_value::<Market>(market_json.clone()) {
                            all_markets.push(market);
                        }
                    }
                }
            }
        } else if let Some(data) = json.get("data") {
            if let Some(events) = data.as_array() {
                for event in events {
                    if let Some(markets) = event.get("markets").and_then(|m| m.as_array()) {
                        for market_json in markets {
                            if let Ok(market) = serde_json::from_value::<Market>(market_json.clone()) {
                                all_markets.push(market);
                            }
                        }
                    }
                }
            }
        }
        
        log::debug!("Fetched {} active markets from events endpoint", all_markets.len());
        Ok(all_markets)
    }

    pub async fn get_market_by_slug(&self, slug: &str) -> Result<Market> {
        let url = format!("{}/events/slug/{}", self.gamma_url, slug);
        
        let response = self.client.get(&url).send().await
            .context(format!("Failed to fetch market by slug: {}", slug))?;
        
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to fetch market by slug: {} (status: {})", slug, status);
        }
        
        let json: Value = response.json().await
            .context("Failed to parse market response")?;
        
        // The response is an event object with a "markets" array
        // Extract the first market from the markets array
        if let Some(markets) = json.get("markets").and_then(|m| m.as_array()) {
            if let Some(market_json) = markets.first() {
                // Try to deserialize the market
                if let Ok(market) = serde_json::from_value::<Market>(market_json.clone()) {
                    return Ok(market);
                }
            }
        }
        
        anyhow::bail!("Invalid market response format: no markets array found")
    }

    pub async fn get_orderbook(&self, token_id: &str) -> Result<OrderBook> {
        let url = format!("{}/book", self.clob_url);
        let params = [("token_id", token_id)];

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to fetch orderbook")?;

        let orderbook: OrderBook = response
            .json()
            .await
            .context("Failed to parse orderbook")?;

        Ok(orderbook)
    }

    pub async fn get_market(&self, condition_id: &str) -> Result<MarketDetails> {
        let url = format!("{}/markets/{}", self.clob_url, condition_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context(format!("Failed to fetch market for condition_id: {}", condition_id))?;

        let status = response.status();
        
        if !status.is_success() {
            anyhow::bail!("Failed to fetch market (status: {})", status);
        }

        let json_text = response.text().await
            .context("Failed to read response body")?;

        let market: MarketDetails = serde_json::from_str(&json_text)
            .map_err(|e| {
                log::error!("Failed to parse market response: {}. Response was: {}", e, json_text);
                anyhow::anyhow!("Failed to parse market response: {}", e)
            })?;

        Ok(market)
    }

    pub async fn get_price(&self, token_id: &str, side: &str) -> Result<rust_decimal::Decimal> {
        let url = format!("{}/price", self.clob_url);
        let params = [
            ("side", side),
            ("token_id", token_id),
        ];

        log::debug!("Fetching price from: {}?side={}&token_id={}", url, side, token_id);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to fetch price")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to fetch price (status: {})", status);
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse price response")?;

        let price_str = json.get("price")
            .and_then(|p| p.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid price response format"))?;

        let price = rust_decimal::Decimal::from_str(price_str)
            .context(format!("Failed to parse price: {}", price_str))?;

        log::debug!("Price for token {} (side={}): {}", token_id, side, price);

        Ok(price)
    }

    pub async fn get_best_price(&self, token_id: &str) -> Result<Option<TokenPrice>> {
        let orderbook = self.get_orderbook(token_id).await?;
        
        let best_bid = orderbook.bids.first().map(|b| b.price);
        let best_ask = orderbook.asks.first().map(|a| a.price);

        if best_ask.is_some() {
            Ok(Some(TokenPrice {
                token_id: token_id.to_string(),
                bid: best_bid,
                ask: best_ask,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn place_order(&self, order: &OrderRequest) -> Result<OrderResponse> {
        if order.side != "BUY" && order.side != "SELL" {
            anyhow::bail!("Invalid order side: {}. Must be 'BUY' or 'SELL'", order.side);
        }
        let handle = self.ensure_clob_client().await?;
        eprintln!("📤 Creating and posting order: {} {} {} @ {}", order.side, order.size, order.token_id, order.price);
        let order_id = clob_sdk::post_limit_order(handle, &order.token_id, &order.side, &order.price, &order.size)?;
        eprintln!("✅ Order placed successfully! Order ID: {}", order_id);
        Ok(OrderResponse {
            order_id: Some(order_id.clone()),
            status: "LIVE".to_string(),
            message: Some(format!("Order placed successfully. Order ID: {}", order_id)),
        })
    }

    pub async fn place_limit_orders(&self, orders: &[OrderRequest]) -> Result<Vec<OrderResponse>> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }
        let handle = self.ensure_clob_client().await?;
        eprintln!("📤 Posting {} limit orders...", orders.len());
        let mut out = Vec::with_capacity(orders.len());
        for (i, order) in orders.iter().enumerate() {
            if order.side != "BUY" && order.side != "SELL" {
                out.push(OrderResponse {
                    order_id: None,
                    status: "REJECTED".to_string(),
                    message: Some(format!("Invalid order side: {}", order.side)),
                });
                continue;
            }
            match clob_sdk::post_limit_order(handle, &order.token_id, &order.side, &order.price, &order.size) {
                Ok(order_id) => out.push(OrderResponse {
                    order_id: Some(order_id.clone()),
                    status: "LIVE".to_string(),
                    message: Some(format!("Order ID: {}", order_id)),
                }),
                Err(e) => {
                    let token_id = &order.token_id;
                    eprintln!("   Order {} (token {}...) rejected: {}", i + 1, &token_id[..token_id.len().min(16)], e);
                    out.push(OrderResponse {
                        order_id: None,
                        status: "REJECTED".to_string(),
                        message: Some(e.to_string()),
                    });
                }
            }
        }
        Ok(out)
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let path = "/order";
        let url = format!("{}{}", self.clob_url, path);
        let body = serde_json::json!({ "orderID": order_id });
        let body_str = body.to_string();
        let mut request = self.client.delete(&url).json(&body);
        request = self.add_auth_headers(request, "DELETE", path, &body_str)
            .context("Failed to add auth headers for cancel")?;
        eprintln!("🛑 Cancelling order: {}", order_id);
        let response = request.send().await.context("Failed to send cancel order request")?;
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Cancel order failed (status {}): {}", status, err_text);
        }
        eprintln!("✅ Order cancel confirmed for order: {}", order_id);
        Ok(())
    }

    pub async fn discover_current_market(&self, asset: &str) -> Result<Option<String>> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Calculate current 15-minute period
        let current_period = (current_time / 900) * 900;
        
        // Try to find market for current period and a few previous periods (in case market is slightly delayed)
        for offset in 0..=2 {
            let period_to_check = current_period - (offset * 900);
            let slug = format!("{}-updown-15m-{}", asset.to_lowercase(), period_to_check);
            
            // Try to get market by slug
            if let Ok(market) = self.get_market_by_slug(&slug).await {
                return Ok(Some(market.condition_id));
            }
        }
        
        // If slug-based discovery fails, try searching active markets
        if let Ok(markets) = self.get_all_active_markets(50).await {
            let asset_upper = asset.to_uppercase();
            for market in markets {
                // Check if this is a BTC/ETH 15-minute market
                if market.slug.contains(&format!("{}-updown-15m", asset.to_lowercase())) 
                    || market.question.to_uppercase().contains(&format!("{} 15", asset_upper)) {
                    return Ok(Some(market.condition_id));
                }
            }
        }
        
        Ok(None)
    }

    pub async fn get_portfolio_tokens_all(&self, _btc_condition_id: Option<&str>, _eth_condition_id: Option<&str>) -> Result<Vec<(String, f64, String, String)>> {
        const REDEEM_SCAN_PERIODS: u32 = 24; // 24 × 15 min = 6 hours (fewer requests to avoid 429)
        const DELAY_BETWEEN_PERIODS_MS: u64 = 200; // Delay between each period check to avoid rate limits
        let mut tokens_with_balance = Vec::new();
        
        // Check BTC markets (current + previous periods)
        println!("🔍 Scanning BTC markets (current + last {} periods ≈ 6h)...", REDEEM_SCAN_PERIODS);
        for offset in 0..=REDEEM_SCAN_PERIODS {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let period_to_check = (current_time / 900) * 900 - (offset as u64) * 900;
            let slug = format!("btc-updown-15m-{}", period_to_check);
            
            if let Ok(market) = self.get_market_by_slug(&slug).await {
                let condition_id = market.condition_id.clone();
                println!("   📊 Checking BTC market: {} (period: {})", &condition_id[..16], period_to_check);
                
                if let Ok(market_details) = self.get_market(&condition_id).await {
                    for token in &market_details.tokens {
                        match self.check_balance_only(&token.token_id).await {
                            Ok(balance) => {
                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                                if balance_f64 > 0.0 {
                                    let description = format!("BTC {} (period: {})", token.outcome, period_to_check);
                                    tokens_with_balance.push((token.token_id.clone(), balance_f64, description, condition_id.clone()));
                                    println!("      ✅ Found token with balance: {} shares", balance_f64);
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
            }
            // Delay between period checks to avoid 429 Too Many Requests (Cloudflare/relayer rate limit)
            if offset < REDEEM_SCAN_PERIODS {
                tokio::time::sleep(tokio::time::Duration::from_millis(DELAY_BETWEEN_PERIODS_MS)).await;
            }
        }
        
        // ETH and Solana scanning disabled temporarily (trading BTC only)
        // Uncomment the blocks below to re-enable.
        /*
        // Check ETH markets (current + recent past)
        println!("🔍 Scanning ETH markets (current + recent past)...");
        for offset in 0..=10 { // Check last 10 periods (2.5 hours)
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let period_to_check = (current_time / 900) * 900 - (offset * 900);
            let slug = format!("eth-updown-15m-{}", period_to_check);
            
            if let Ok(market) = self.get_market_by_slug(&slug).await {
                let condition_id = market.condition_id.clone();
                println!("   📊 Checking ETH market: {} (period: {})", &condition_id[..16], period_to_check);
                
                if let Ok(market_details) = self.get_market(&condition_id).await {
                    for token in &market_details.tokens {
                        match self.check_balance_only(&token.token_id).await {
                            Ok(balance) => {
                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                                if balance_f64 > 0.0 {
                                    let description = format!("ETH {} (period: {})", token.outcome, period_to_check);
                                    tokens_with_balance.push((token.token_id.clone(), balance_f64, description, condition_id.clone()));
                                    println!("      ✅ Found token with balance: {} shares", balance_f64);
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
            }
        }
        
        // Check Solana markets (current + recent past)
        println!("🔍 Scanning Solana markets (current + recent past)...");
        for offset in 0..=10 { // Check last 10 periods (2.5 hours)
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let period_to_check = (current_time / 900) * 900 - (offset * 900);
            
            // Try both slug formats
            let slugs = vec![
                format!("solana-updown-15m-{}", period_to_check),
                format!("sol-updown-15m-{}", period_to_check),
            ];
            
            for slug in slugs {
                if let Ok(market) = self.get_market_by_slug(&slug).await {
                    let condition_id = market.condition_id.clone();
                    println!("   📊 Checking Solana market: {} (period: {})", &condition_id[..16], period_to_check);
                    
                    if let Ok(market_details) = self.get_market(&condition_id).await {
                        for token in &market_details.tokens {
                            match self.check_balance_only(&token.token_id).await {
                                Ok(balance) => {
                                    let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                    let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                                    if balance_f64 > 0.0 {
                                        let description = format!("Solana {} (period: {})", token.outcome, period_to_check);
                                        tokens_with_balance.push((token.token_id.clone(), balance_f64, description, condition_id.clone()));
                                        println!("      ✅ Found token with balance: {} shares", balance_f64);
                                    }
                                }
                                Err(_) => continue,
                            }
                        }
                    }
                    break; // Found a valid market, no need to try other slug format
                }
            }
        }
        */
        
        Ok(tokens_with_balance)
    }

    pub async fn get_portfolio_tokens(&self, btc_condition_id: Option<&str>, eth_condition_id: Option<&str>) -> Result<Vec<(String, f64, String)>> {
        let mut tokens_with_balance = Vec::new();
        
        // Discover BTC market if not provided
        let btc_condition_id_owned: Option<String> = if let Some(id) = btc_condition_id {
            Some(id.to_string())
        } else {
            println!("🔍 Discovering current BTC 15-minute market...");
            match self.discover_current_market("BTC").await {
                Ok(Some(id)) => {
                    println!("   ✅ Found BTC market: {}", id);
                    Some(id)
                }
                Ok(None) => {
                    println!("   ⚠️  Could not find current BTC market");
                    None
                }
                Err(e) => {
                    eprintln!("   ❌ Error discovering BTC market: {}", e);
                    None
                }
            }
        };
        
        // Discover ETH market if not provided
        let eth_condition_id_owned: Option<String> = if let Some(id) = eth_condition_id {
            Some(id.to_string())
        } else {
            println!("🔍 Discovering current ETH 15-minute market...");
            match self.discover_current_market("ETH").await {
                Ok(Some(id)) => {
                    println!("   ✅ Found ETH market: {}", id);
                    Some(id)
                }
                Ok(None) => {
                    println!("   ⚠️  Could not find current ETH market");
                    None
                }
                Err(e) => {
                    eprintln!("   ❌ Error discovering ETH market: {}", e);
                    None
                }
            }
        };
        
        // Check BTC market tokens
        if let Some(ref btc_condition_id) = btc_condition_id_owned {
            println!("📊 Checking BTC market tokens for condition: {}", btc_condition_id);
            if let Ok(btc_market) = self.get_market(btc_condition_id).await {
                println!("   ✅ Found {} tokens in BTC market", btc_market.tokens.len());
                for token in &btc_market.tokens {
                    println!("   🔍 Checking balance for token: {} ({})", token.outcome, &token.token_id[..16]);
                    match self.check_balance_allowance(&token.token_id).await {
                        Ok((balance, _)) => {
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            println!("      Balance: {:.6} shares", balance_f64);
                            if balance_f64 > 0.0 {
                                tokens_with_balance.push((token.token_id.clone(), balance_f64, format!("BTC {}", token.outcome)));
                                println!("      ✅ Found token with balance!");
                            }
                        }
                        Err(e) => {
                            println!("      ⚠️  Failed to check balance: {}", e);
                            // Skip tokens that fail balance check (might not exist or network error)
                            continue;
                        }
                    }
                }
            } else {
                eprintln!("   ❌ Failed to fetch BTC market details");
            }
        }
        
        // Check ETH market tokens
        if let Some(ref eth_condition_id) = eth_condition_id_owned {
            println!("📊 Checking ETH market tokens for condition: {}", eth_condition_id);
            if let Ok(eth_market) = self.get_market(eth_condition_id).await {
                println!("   ✅ Found {} tokens in ETH market", eth_market.tokens.len());
                for token in &eth_market.tokens {
                    println!("   🔍 Checking balance for token: {} ({})", token.outcome, &token.token_id[..16]);
                    match self.check_balance_allowance(&token.token_id).await {
                        Ok((balance, _)) => {
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            println!("      Balance: {:.6} shares", balance_f64);
                            if balance_f64 > 0.0 {
                                tokens_with_balance.push((token.token_id.clone(), balance_f64, format!("ETH {}", token.outcome)));
                                println!("      ✅ Found token with balance!");
                            }
                        }
                        Err(e) => {
                            println!("      ⚠️  Failed to check balance: {}", e);
                            // Skip tokens that fail balance check
                            continue;
                        }
                    }
                }
            } else {
                eprintln!("   ❌ Failed to fetch ETH market details");
            }
        }
        
        Ok(tokens_with_balance)
    }

    pub async fn check_usdc_balance_allowance(&self) -> Result<(rust_decimal::Decimal, rust_decimal::Decimal)> {
        let handle = self.ensure_clob_client().await?;
        let (balance_str, allowance_str) = clob_sdk::balance_allowance(handle, "", "Collateral")?;
        let balance = rust_decimal::Decimal::from_str(balance_str.trim()).unwrap_or(rust_decimal::Decimal::ZERO);
        let allowance = rust_decimal::Decimal::from_str(allowance_str.trim()).unwrap_or(rust_decimal::Decimal::ZERO);
        Ok((balance, allowance))
    }

    pub async fn check_balance_only(&self, token_id: &str) -> Result<rust_decimal::Decimal> {
        let handle = self.ensure_clob_client().await?;
        let (balance_str, _) = clob_sdk::balance_allowance(handle, token_id, "Conditional")?;
        rust_decimal::Decimal::from_str(balance_str.trim()).context("Failed to parse balance")
    }

    pub async fn check_balance_allowance(&self, token_id: &str) -> Result<(rust_decimal::Decimal, rust_decimal::Decimal)> {
        let handle = self.ensure_clob_client().await?;
        let (balance_str, allowance_str) = clob_sdk::balance_allowance(handle, token_id, "Conditional")?;
        let balance = rust_decimal::Decimal::from_str(balance_str.trim()).context("Parse balance")?;
        let allowance = rust_decimal::Decimal::from_str(allowance_str.trim()).unwrap_or(rust_decimal::Decimal::ZERO);
        let is_approved_for_all = match self.check_is_approved_for_all().await {
            Ok(true) => true,
            Ok(false) => {
                eprintln!("   ⚠️  setApprovalForAll: NOT SET (Exchange cannot spend tokens)");
                false
            }
            Err(e) => {
                eprintln!("   ⚠️  setApprovalForAll check failed: {}", e);
                false
            }
        };
        let _ = is_approved_for_all;
        Ok((balance, allowance))
    }

    pub async fn update_balance_allowance_for_sell(&self, token_id: &str) -> Result<()> {
        let handle = self.ensure_clob_client().await?;
        clob_sdk::update_balance_allowance(handle, token_id, "Conditional")
    }

    fn get_clob_contract_address(&self) -> Result<String> {
        let config = clob_sdk::contract_config(polygon(), false)
            .context("Failed to get contract config")?
            .ok_or_else(|| anyhow::anyhow!("Contract config not available for chain"))?;
        Ok(format!("{:#x}", config.exchange))
    }

    fn get_ctf_contract_address(&self) -> Result<String> {
        let config = clob_sdk::contract_config(polygon(), false)
            .context("Failed to get contract config")?
            .ok_or_else(|| anyhow::anyhow!("Contract config not available for chain"))?;
        Ok(format!("{:#x}", config.conditional_tokens))
    }

    pub async fn check_is_approved_for_all(&self) -> Result<bool> {
        let config = clob_sdk::contract_config(polygon(), false)
            .context("Failed to load contract config")?
            .ok_or_else(|| anyhow::anyhow!("Contract config not available"))?;
        let ctf_contract_address = config.conditional_tokens;
        let exchange_address = config.exchange;
        let account_to_check = if let Some(proxy_addr) = &self.proxy_wallet_address {
            AlloyAddress::parse_checksummed(proxy_addr, None)
                .context(format!("Failed to parse proxy_wallet_address: {}", proxy_addr))?
        } else {
            let private_key = self.private_key.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Private key required to check approval"))?;
            let signer = LocalSigner::from_str(private_key)
                .context("Failed to create signer from private key")?
                .with_chain_id(Some(polygon()));
            signer.address()
        };
        
        let rpc_url = Self::polygon_rpc_url();
        let provider = ProviderBuilder::new()
            .connect(&rpc_url)
            .await
            .context("Failed to connect to Polygon RPC")?;
        
        let ctf = IERC1155::new(ctf_contract_address, provider);
        
        let approved = ctf
            .isApprovedForAll(account_to_check, exchange_address)
            .call()
            .await
            .context("Failed to check isApprovedForAll")?;
        
        Ok(approved)
    }

    pub async fn check_all_approvals(&self) -> Result<Vec<(String, bool, bool)>> {
        let rpc_url = Self::polygon_rpc_url();
        const USDC_ADDRESS_STR: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
        let usdc_address = AlloyAddress::from_str(USDC_ADDRESS_STR)
            .context("Invalid USDC address")?;
        let config = clob_sdk::contract_config(polygon(), false)
            .context("Failed to load contract config")?
            .ok_or_else(|| anyhow::anyhow!("Contract config not available"))?;
        let neg_risk_config = clob_sdk::contract_config(polygon(), true)
            .context("Failed to load neg risk config")?
            .ok_or_else(|| anyhow::anyhow!("Neg risk contract config not available"))?;
        let account_to_check = if let Some(proxy_addr) = &self.proxy_wallet_address {
            AlloyAddress::parse_checksummed(proxy_addr, None)
                .context(format!("Failed to parse proxy_wallet_address: {}", proxy_addr))?
        } else {
            let private_key = self.private_key.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Private key required to check approval"))?;
            let signer = LocalSigner::from_str(private_key)
                .context("Failed to create signer from private key")?
                .with_chain_id(Some(polygon()));
            signer.address()
        };
        let provider = ProviderBuilder::new()
            .connect(&rpc_url)
            .await
            .context("Failed to connect to Polygon RPC")?;
        let usdc = IERC20::new(usdc_address, provider.clone());
        let ctf = IERC1155::new(config.conditional_tokens, provider.clone());
        let mut targets: Vec<(&str, AlloyAddress)> = vec![
            ("CTF Exchange", config.exchange),
            ("Neg Risk CTF Exchange", neg_risk_config.exchange),
        ];
        if let Some(adapter) = neg_risk_config.neg_risk_adapter {
            targets.push(("Neg Risk Adapter", adapter));
        }
        
        let mut results = Vec::new();
        
        for (name, target) in &targets {
            let usdc_approved = usdc
                .allowance(account_to_check, *target)
                .call()
                .await
                .map(|allowance| allowance > U256::ZERO)
                .unwrap_or(false);
            let ctf_approved = ctf
                .isApprovedForAll(account_to_check, *target)
                .call()
                .await
                .unwrap_or(false);
            
            results.push((name.to_string(), usdc_approved, ctf_approved));
        }
        
        Ok(results)
    }

    pub async fn set_approval_for_all_clob(&self) -> Result<()> {
        let config = clob_sdk::contract_config(polygon(), false)
            .context("Failed to load contract config")?
            .ok_or_else(|| anyhow::anyhow!("Contract config not available"))?;
        let ctf_contract_address = config.conditional_tokens;
        let exchange_address = config.exchange;
        
        eprintln!("🔐 Setting approval for all tokens using CTF contract's setApprovalForAll()");
        eprintln!("   CTF Contract (conditional_tokens): {:#x}", ctf_contract_address);
        eprintln!("   CTF Exchange (exchange/operator): {:#x}", exchange_address);
        eprintln!("   This will approve the Exchange contract to manage ALL your conditional tokens");
        
        // For proxy wallets, use relayer (gasless transactions)
        // For EOA wallets, use direct RPC call
        if let Some(proxy_addr) = &self.proxy_wallet_address {
            eprintln!("   🔄 Using Polymarket relayer for proxy wallet (gasless transaction)");
            eprintln!("   Proxy wallet: {}", proxy_addr);
            
            // Use relayer to execute setApprovalForAll from proxy wallet
            // Based on: https://docs.polymarket.com/developers/builders/relayer-client
            self.set_approval_for_all_via_relayer(ctf_contract_address, exchange_address).await
        } else {
            eprintln!("   🔄 Using direct RPC call for EOA wallet");
            
            // Check if we have a private key (required for signing)
            let private_key = self.private_key.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Private key is required for token approval. Please set private_key in config.json"))?;
            
            // Create signer from private key
            let signer = LocalSigner::from_str(private_key)
                .context("Failed to create signer from private key. Ensure private_key is a valid hex string.")?
                .with_chain_id(Some(polygon()));
            
            let signer_address = signer.address();
            eprintln!("   💰 Wallet that needs MATIC for gas: {:#x}", signer_address);
            
            // Use direct RPC call like SDK example (instead of relayer)
            // Based on: https://github.com/Polymarket/rs-clob-client/blob/main/examples/approvals.rs
            let rpc_url = Self::polygon_rpc_url();
            
            let provider = ProviderBuilder::new()
                .wallet(signer.clone())
                .connect(&rpc_url)
                .await
                .context("Failed to connect to Polygon RPC")?;
            
            // Create IERC1155 contract instance
            let ctf = IERC1155::new(ctf_contract_address, provider.clone());
            
            eprintln!("   📤 Sending setApprovalForAll transaction via direct RPC call...");
            
            // Call setApprovalForAll directly (like SDK example)
            let tx_hash = ctf
                .setApprovalForAll(exchange_address, true)
                .send()
                .await
                .context("Failed to send setApprovalForAll transaction")?
                .watch()
                .await
                .context("Failed to watch setApprovalForAll transaction")?;
            
            eprintln!("   ✅ Successfully sent setApprovalForAll transaction!");
            eprintln!("   Transaction Hash: {:#x}", tx_hash);
            
            Ok(())
        }
    }
    
    async fn set_approval_for_all_via_relayer(
        &self,
        ctf_contract_address: AlloyAddress,
        exchange_address: AlloyAddress,
    ) -> Result<()> {
        // When using a Gnosis Safe (signature_type=2), the Polymarket relayer expects a
        // specific SafeTransaction format and Builder-relayer authorization.
        // Our current relayer payload/signature is for the simpler proxy-flow and can
        // fail with `401 invalid authorization`.
        //
        // Workaround: execute `setApprovalForAll` directly from the Safe using
        // Safe.execTransaction (same approach we already use for Safe redemptions).
        if self.signature_type == Some(2) {
            eprintln!("   🔄 signature_type=2: sending setApprovalForAll via Safe.execTransaction (direct on-chain)...");
            return self.set_approval_for_all_via_safe_exec(ctf_contract_address, exchange_address).await;
        }

        // Check signature_type - warn if using GNOSIS_SAFE (type 2) as it may need different format
        if let Some(2) = self.signature_type {
            eprintln!("   ⚠️  Using signature_type 2 (GNOSIS_SAFE) - relayer may require Safe transaction format");
            eprintln!("   💡 If this fails, the full Safe transaction flow (nonce, Safe address, struct hash) may be needed");
        }
        
        // Function signature: setApprovalForAll(address operator, bool approved)
        // Function selector: keccak256("setApprovalForAll(address,bool)")[0:4] = 0xa22cb465
        let function_selector = hex::decode("a22cb465")
            .context("Failed to decode function selector")?;
        
        // Encode parameters: (address operator, bool approved)
        let mut encoded_params = Vec::new();
        
        // Encode operator address (20 bytes, left-padded to 32 bytes)
        let mut operator_bytes = [0u8; 32];
        operator_bytes[12..].copy_from_slice(exchange_address.as_slice());
        encoded_params.extend_from_slice(&operator_bytes);
        
        // Encode approved (bool) - true = 1, padded to 32 bytes
        let approved_bytes = U256::from(1u64).to_be_bytes::<32>();
        encoded_params.extend_from_slice(&approved_bytes);
        
        // Combine function selector with encoded parameters
        let mut call_data = function_selector;
        call_data.extend_from_slice(&encoded_params);
        
        let call_data_hex = format!("0x{}", hex::encode(&call_data));
        
        eprintln!("   📝 Encoded call data: {}", call_data_hex);
        
        // Use relayer for gasless transaction. The /execute path returns 404; the
        // builder-relayer-client uses POST /submit. See: Polymarket/builder-relayer-client
        const RELAYER_SUBMIT: &str = "https://relayer-v2.polymarket.com/submit";
        
        eprintln!("   📤 Sending setApprovalForAll transaction via relayer (POST /submit)...");
        
        // Build transaction for relayer (matches SafeTransaction: to, operation=Call, data, value)
        let ctf_address_str = format!("{:#x}", ctf_contract_address);
        let transaction = serde_json::json!({
            "to": ctf_address_str,
            "operation": 0u8,   // 0 = Call
            "data": call_data_hex,
            "value": "0"
        });
        
        let relayer_request = serde_json::json!({
            "transactions": [transaction],
            "description": format!("Set approval for all tokens - approve Exchange contract {:#x}", exchange_address)
        });
        
        // Add authentication headers (Builder API credentials)
        let api_key = self.api_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API key required for relayer. Please set api_key in config.json"))?;
        let api_secret = self.api_secret.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API secret required for relayer. Please set api_secret in config.json"))?;
        let api_passphrase = self.api_passphrase.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API passphrase required for relayer. Please set api_passphrase in config.json"))?;
        
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let timestamp_str = timestamp_ms.to_string();
        
        let body_string = serde_json::to_string(&relayer_request)
            .context("Failed to serialize relayer request")?;
        
        let signature = Self::builder_relayer_signature(
            api_secret,
            timestamp_ms,
            "POST",
            "/submit",
            &body_string,
        )?;
        
        // Send request to relayer
        let response = self.client
            .post(RELAYER_SUBMIT)
            .header("User-Agent", "polymarket-trading-bot/1.0")
            .header("POLY_BUILDER_API_KEY", api_key)
            .header("POLY_BUILDER_TIMESTAMP", &timestamp_str)
            .header("POLY_BUILDER_PASSPHRASE", api_passphrase)
            .header("POLY_BUILDER_SIGNATURE", &signature)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&relayer_request)
            .send()
            .await
            .context("Failed to send setApprovalForAll request to relayer")?;
        
        let status = response.status();
        let response_text = response.text().await
            .context("Failed to read relayer response")?;
        
        if !status.is_success() {
            let sig_type_hint = if self.signature_type == Some(2) {
                "\n\n   💡 For signature_type 2 (GNOSIS_SAFE), the relayer expects a Safe transaction format:\n\
                  - Get nonce from /nonce endpoint\n\
                  - Derive Safe address from signer\n\
                  - Build SafeTx struct hash\n\
                  - Sign and pack signature\n\
                  - Send: { from, to, proxyWallet, data, nonce, signature, signatureParams, type: \"SAFE\", metadata }\n\
                  \n\
                  Consider using signature_type 1 (POLY_PROXY) if possible, or implement the full Safe flow."
            } else {
                ""
            };
            
            anyhow::bail!(
                "Relayer rejected setApprovalForAll request (status: {}): {}\n\
                \n\
                CTF Contract Address: {:#x}\n\
                Exchange Contract Address: {:#x}\n\
                Signature Type: {:?}\n\
                \n\
                This may be a relayer endpoint issue, authentication problem, or request format mismatch.\n\
                Please verify your Builder API credentials are correct.{}",
                status, response_text, ctf_contract_address, exchange_address, self.signature_type, sig_type_hint
            );
        }
        
        // Parse relayer response
        let relayer_response: serde_json::Value = serde_json::from_str(&response_text)
            .context("Failed to parse relayer response")?;
        
        let transaction_id = relayer_response["transactionID"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing transactionID in relayer response"))?;
        
        eprintln!("   ✅ Successfully sent setApprovalForAll transaction via relayer!");
        eprintln!("   Transaction ID: {}", transaction_id);
        eprintln!("   💡 The relayer will execute this transaction from your proxy wallet (gasless)");
        
        // Wait for transaction confirmation (like TypeScript SDK's response.wait())
        eprintln!("   ⏳ Waiting for transaction confirmation...");
        self.wait_for_relayer_transaction(transaction_id).await?;
        
        Ok(())
    }

    async fn set_approval_for_all_via_safe_exec(
        &self,
        ctf_contract_address: AlloyAddress,
        exchange_address: AlloyAddress,
    ) -> Result<()> {
        let private_key = self
            .private_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("private_key is required to sign Safe transactions"))?;
        let signer = LocalSigner::from_str(private_key)
            .context("Failed to create signer from private key")?
            .with_chain_id(Some(polygon()));

        let safe_address_str = self
            .proxy_wallet_address
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("proxy_wallet_address is required for signature_type=2"))?;
        let safe_address = AlloyAddress::parse_checksummed(safe_address_str, None)
            .context("Failed to parse proxy_wallet_address (Safe address)")?;

        let rpc_url = Self::polygon_rpc_url();

        // Build call data for: setApprovalForAll(address operator, bool approved)
        // selector: keccak256("setApprovalForAll(address,bool)")[0:4] = 0xa22cb465
        let function_selector = hex::decode("a22cb465").context("Failed to decode setApprovalForAll selector")?;

        let mut operator_bytes = [0u8; 32];
        operator_bytes[12..].copy_from_slice(exchange_address.as_slice());
        let approved_bytes = U256::from(1u64).to_be_bytes::<32>();

        let mut approval_calldata = Vec::new();
        approval_calldata.extend_from_slice(&function_selector);
        approval_calldata.extend_from_slice(&operator_bytes);
        approval_calldata.extend_from_slice(&approved_bytes);

        // Safe transaction preparation:
        // - nonce() and getTransactionHash() reads
        // - sign EIP-191 over the safe tx hash
        // - build execTransaction calldata
        let provider_read = ProviderBuilder::new()
            .connect(&rpc_url)
            .await
            .context("Failed to connect to RPC for Safe reads")?;

        let nonce_selector = keccak256("nonce()".as_bytes());
        let nonce_calldata: Vec<u8> = nonce_selector.as_slice()[..4].to_vec();
        let nonce_tx = TransactionRequest::default()
            .to(safe_address)
            .input(Bytes::from(nonce_calldata.clone()).into());
        let nonce_result = provider_read
            .call(nonce_tx)
            .await
            .context("Safe.nonce() call failed")?;
        let nonce_bytes: [u8; 32] = nonce_result
            .as_ref()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Safe.nonce() did not return 32 bytes"))?;
        let nonce = U256::from_be_slice(&nonce_bytes);

        const SAFE_TX_GAS: u64 = 300_000;
        let get_tx_hash_sig =
            "getTransactionHash(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,uint256)";
        let get_tx_hash_selector = keccak256(get_tx_hash_sig.as_bytes()).as_slice()[..4].to_vec();

        let zero_addr = [0u8; 32];
        let mut to_enc = [0u8; 32];
        to_enc[12..].copy_from_slice(ctf_contract_address.as_slice());

        // data_offset is based on execTransaction(uint256) ABI layout:
        let data_offset_get_hash = U256::from(32u32 * 10u32);

        // Encode getTransactionHash(...) call arguments
        let mut get_tx_hash_calldata = Vec::new();
        get_tx_hash_calldata.extend_from_slice(&get_tx_hash_selector);
        get_tx_hash_calldata.extend_from_slice(&to_enc);
        get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
        get_tx_hash_calldata.extend_from_slice(&data_offset_get_hash.to_be_bytes::<32>());
        // operation=0 (uint8) padded to 32 bytes
        get_tx_hash_calldata.push(0);
        get_tx_hash_calldata.extend_from_slice(&[0u8; 31]);
        get_tx_hash_calldata.extend_from_slice(&U256::from(SAFE_TX_GAS).to_be_bytes::<32>());
        get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>()); // baseGas
        get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>()); // gasPrice
        get_tx_hash_calldata.extend_from_slice(&zero_addr); // gasToken
        get_tx_hash_calldata.extend_from_slice(&zero_addr); // refundReceiver
        get_tx_hash_calldata.extend_from_slice(&nonce.to_be_bytes::<32>());
        // bytes data: length + data
        get_tx_hash_calldata.extend_from_slice(&U256::from(approval_calldata.len()).to_be_bytes::<32>());
        get_tx_hash_calldata.extend_from_slice(&approval_calldata);

        let get_tx_hash_tx = TransactionRequest::default()
            .to(safe_address)
            .input(Bytes::from(get_tx_hash_calldata).into());
        let tx_hash_result = provider_read
            .call(get_tx_hash_tx)
            .await
            .context("Failed to call Safe.getTransactionHash()")?;
        let tx_hash_to_sign: B256 = tx_hash_result
            .as_ref()
            .try_into()
            .map_err(|_| anyhow::anyhow!("getTransactionHash returned unexpected size"))?;

        // EIP-191: "\x19Ethereum Signed Message:\n32" + keccak(tx_hash)
        const EIP191_PREFIX: &[u8] = b"\x19Ethereum Signed Message:\n32";
        let mut eip191_message = Vec::with_capacity(EIP191_PREFIX.len() + 32);
        eip191_message.extend_from_slice(EIP191_PREFIX);
        eip191_message.extend_from_slice(tx_hash_to_sign.as_slice());
        let hash_to_sign = keccak256(&eip191_message);

        let sig = signer
            .sign_hash(&hash_to_sign)
            .await
            .context("Failed to sign Safe transaction hash")?;
        let sig_bytes = sig.as_bytes();
        let (r, s, v) = (&sig_bytes[0..32], &sig_bytes[32..64], sig_bytes[64]);
        let v_safe = if v == 27 || v == 28 { v + 4 } else { v };

        let mut packed_sig: Vec<u8> = Vec::with_capacity(85);
        packed_sig.extend_from_slice(r);
        packed_sig.extend_from_slice(s);
        packed_sig.extend_from_slice(&[v_safe]);

        // Determine threshold and include owner address in signature if needed.
        let get_threshold_selector = keccak256("getThreshold()".as_bytes()).as_slice()[..4].to_vec();
        let threshold_tx = TransactionRequest::default()
            .to(safe_address)
            .input(Bytes::from(get_threshold_selector).into());
        let threshold_result = provider_read
            .call(threshold_tx)
            .await
            .context("Failed to call Safe.getThreshold()")?;
        let threshold_bytes: [u8; 32] = threshold_result
            .as_ref()
            .try_into()
            .map_err(|_| anyhow::anyhow!("getThreshold() returned unexpected size"))?;
        let threshold = U256::from_be_slice(&threshold_bytes);

        if threshold > U256::from(1) {
            let owner = signer.address();
            let mut with_owner = Vec::with_capacity(20 + packed_sig.len());
            with_owner.extend_from_slice(owner.as_slice());
            with_owner.extend_from_slice(&packed_sig);
            packed_sig = with_owner;
        }

        let safe_sig_bytes = packed_sig;

        // execTransaction(to,value,data,operation,safeTxGas,baseGas,gasPrice,gasToken,refundReceiver,signatures)
        let exec_sig =
            "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)";
        let exec_selector = keccak256(exec_sig.as_bytes()).as_slice()[..4].to_vec();
        let data_offset = 32u32 * 10u32;
        let sigs_offset = data_offset + 32 + approval_calldata.len() as u32;

        let mut exec_calldata = Vec::new();
        exec_calldata.extend_from_slice(&exec_selector);
        exec_calldata.extend_from_slice(&to_enc);
        exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
        exec_calldata.extend_from_slice(&U256::from(data_offset).to_be_bytes::<32>());
        exec_calldata.push(0);
        exec_calldata.extend_from_slice(&[0u8; 31]); // operation=0
        exec_calldata.extend_from_slice(&U256::from(SAFE_TX_GAS).to_be_bytes::<32>());
        exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>()); // baseGas
        exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>()); // gasPrice
        exec_calldata.extend_from_slice(&zero_addr); // gasToken
        exec_calldata.extend_from_slice(&zero_addr); // refundReceiver
        exec_calldata.extend_from_slice(&U256::from(sigs_offset).to_be_bytes::<32>());
        exec_calldata.extend_from_slice(&U256::from(approval_calldata.len()).to_be_bytes::<32>());
        exec_calldata.extend_from_slice(&approval_calldata);
        exec_calldata.extend_from_slice(&U256::from(safe_sig_bytes.len()).to_be_bytes::<32>());
        exec_calldata.extend_from_slice(&safe_sig_bytes);

        let provider = ProviderBuilder::new()
            .wallet(signer.clone())
            .connect(&rpc_url)
            .await
            .context("Failed to connect to Polygon RPC with wallet")?;

        let tx_request = TransactionRequest::default()
            .to(safe_address)
            .input(Bytes::from(exec_calldata).into())
            .value(U256::ZERO)
            .gas_limit(400_000u64);

        eprintln!("   📤 Sending Safe.execTransaction setApprovalForAll (gas limit 400_000)...");
        let pending_tx = provider
            .send_transaction(tx_request)
            .await
            .context("Failed to send setApprovalForAll Safe transaction")?;

        let tx_hash = *pending_tx.tx_hash();
        eprintln!("   Transaction sent, waiting for confirmation...");
        eprintln!("   Transaction hash: {:?}", tx_hash);

        let receipt = pending_tx
            .get_receipt()
            .await
            .context("Failed to get Safe approval transaction receipt")?;
        if !receipt.status() {
            anyhow::bail!("Safe.execTransaction mined but inner approval reverted. Tx hash: {:?}", tx_hash);
        }

        eprintln!("   ✅ Successfully setApprovalForAll via Safe.execTransaction!");
        Ok(())
    }
    
    async fn wait_for_relayer_transaction(&self, transaction_id: &str) -> Result<String> {
        // Based on TypeScript SDK pattern: response.wait() returns transactionHash
        // Relayer states: STATE_NEW, STATE_EXECUTED, STATE_MINE, STATE_CONFIRMED, STATE_FAILED, STATE_INVALID
        let status_url = format!("https://relayer-v2.polymarket.com/transaction/{}", transaction_id);
        
        // Poll for transaction confirmation (with timeout)
        let max_wait_seconds = 120;
        let check_interval_seconds = 2;
        let start_time = std::time::Instant::now();
        
        loop {
            let elapsed = start_time.elapsed().as_secs();
            if elapsed >= max_wait_seconds {
                eprintln!("   ⏱️  Timeout waiting for relayer confirmation ({}s)", max_wait_seconds);
                eprintln!("   💡 Transaction was submitted but confirmation timed out");
                eprintln!("   💡 Check status at: {}", status_url);
                anyhow::bail!("Relayer transaction confirmation timeout after {} seconds", max_wait_seconds);
            }
            
            // Check transaction status
            match self.client
                .get(&status_url)
                .header("User-Agent", "polymarket-trading-bot/1.0")
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        let status_text = response.text().await
                            .context("Failed to read relayer status response")?;
                        
                        let status_data: serde_json::Value = serde_json::from_str(&status_text)
                            .context("Failed to parse relayer status response")?;
                        
                        let state = status_data["state"].as_str()
                            .unwrap_or("UNKNOWN");
                        
                        match state {
                            "STATE_CONFIRMED" => {
                                let tx_hash = status_data["transactionHash"].as_str()
                                    .unwrap_or("N/A");
                                eprintln!("   ✅ Transaction confirmed! Hash: {}", tx_hash);
                                return Ok(tx_hash.to_string());
                            }
                            "STATE_FAILED" | "STATE_INVALID" => {
                                let error_msg = status_data["metadata"].as_str()
                                    .unwrap_or("Transaction failed");
                                anyhow::bail!("Relayer transaction failed: {}", error_msg);
                            }
                            "STATE_NEW" | "STATE_EXECUTED" | "STATE_MINE" => {
                                eprintln!("   ⏳ Transaction state: {} (elapsed: {}s)", state, elapsed);
                                tokio::time::sleep(tokio::time::Duration::from_secs(check_interval_seconds)).await;
                                continue;
                            }
                            _ => {
                                eprintln!("   ⏳ Transaction state: {} (elapsed: {}s)", state, elapsed);
                                tokio::time::sleep(tokio::time::Duration::from_secs(check_interval_seconds)).await;
                                continue;
                            }
                        }
                    } else {
                        warn!("Failed to check relayer status (status: {}): will retry", response.status());
                        tokio::time::sleep(tokio::time::Duration::from_secs(check_interval_seconds)).await;
                        continue;
                    }
                }
                Err(e) => {
                    warn!("Failed to check relayer status: {} - will retry", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(check_interval_seconds)).await;
                    continue;
                }
            }
        }
    }

    pub async fn approve_individual_tokens(&self, eth_market_data: &crate::models::Market, btc_market_data: &crate::models::Market) -> Result<()> {
        eprintln!("🔄 Fallback: Approving individual tokens with large allowance...");
        
        // Get token IDs from current markets
        let eth_condition_id = &eth_market_data.condition_id;
        let btc_condition_id = &btc_market_data.condition_id;
        
        let mut token_ids = Vec::new();
        
        // Get ETH market tokens
        if let Ok(eth_details) = self.get_market(eth_condition_id).await {
            for token in &eth_details.tokens {
                token_ids.push((token.token_id.clone(), format!("ETH {}", token.outcome)));
            }
        }
        
        // Get BTC market tokens
        if let Ok(btc_details) = self.get_market(btc_condition_id).await {
            for token in &btc_details.tokens {
                token_ids.push((token.token_id.clone(), format!("BTC {}", token.outcome)));
            }
        }
        
        if token_ids.is_empty() {
            anyhow::bail!("Could not find any token IDs from current markets");
        }
        
        eprintln!("   Found {} tokens to approve", token_ids.len());
        
        // For each token, trigger SDK auto-approval by placing a tiny test sell order
        // The SDK will automatically approve with a large amount (typically max uint256)
        let mut success_count = 0;
        let mut fail_count = 0;
        
        for (token_id, description) in &token_ids {
            eprintln!("   🔐 Checking {} token balance...", description);
            
            // Check if user has balance for this token before attempting approval
            match self.check_balance_allowance(token_id).await {
                Ok((balance, _)) => {
                    let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                    let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                    
                    if balance_f64 == 0.0 {
                        eprintln!("   ⏭️  Skipping {} token - no balance (balance: 0)", description);
                        continue; // Skip tokens user doesn't own
                    }
                    
                    eprintln!("   ✅ {} token has balance: {:.6} - triggering approval...", description, balance_f64);
                }
                Err(e) => {
                    eprintln!("   ⚠️  Could not check balance for {} token: {} - skipping", description, e);
                    continue; // Skip if we can't check balance
                }
            }
            
            // Place a tiny sell order (0.01 shares) to trigger SDK's auto-approval
            // This order will likely fail due to size, but it will trigger the approval process
            // Using 0.01 (minimum non-zero with 2 decimal places) instead of 0.000001 which rounds to 0.00
            match self.place_market_order(token_id, 0.01, "SELL", Some("FAK")).await {
                Ok(_) => {
                    eprintln!("   ✅ {} token approved successfully", description);
                    success_count += 1;
                }
                Err(e) => {
                    // Check if it's an allowance error (which means approval was triggered)
                    let error_str = format!("{}", e);
                    if error_str.contains("balance") || error_str.contains("allowance") {
                        eprintln!("   ✅ {} token approval triggered (order failed but approval succeeded)", description);
                        success_count += 1;
                    } else {
                        eprintln!("   ⚠️  {} token approval failed: {}", description, error_str);
                        fail_count += 1;
                    }
                }
            }
            
            // Small delay between approvals
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        
        if success_count > 0 {
            eprintln!("✅ Successfully approved {}/{} tokens with large allowance", success_count, token_ids.len());
            if fail_count > 0 {
                eprintln!("   ⚠️  {} tokens failed to approve (will retry on sell if needed)", fail_count);
            }
            Ok(())
        } else {
            anyhow::bail!("Failed to approve any tokens. All {} attempts failed.", token_ids.len())
        }
    }

    pub async fn place_market_order(
        &self,
        token_id: &str,
        amount: f64,
        side: &str,
        order_type: Option<&str>, // "FOK" or "FAK", defaults to FOK
    ) -> Result<OrderResponse> {
        if side != "BUY" && side != "SELL" {
            anyhow::bail!("Invalid order side: {}. Must be 'BUY' or 'SELL'", side);
        }
        let ot = order_type.unwrap_or("FOK");
        if ot != "FOK" && ot != "FAK" {
            anyhow::bail!("Invalid order_type: {}. Must be 'FOK' or 'FAK'", ot);
        }
        let is_buy = side == "BUY";
        let normalized_amount = if is_buy {
            Self::normalize_buy_usdc_amount(amount)
        } else {
            amount
        };
        // Keep higher precision in string form; FFI applies final asset-specific truncation.
        let amount_str = format!("{:.8}", normalized_amount);
        if is_buy {
            if normalized_amount <= 0.0 {
                anyhow::bail!(
                    "Buy amount {} is below minimum precision after normalization (2 decimals).",
                    amount
                );
            }
            if let Ok((usdc_balance, _)) = self.check_usdc_balance_allowance().await {
                let need = rust_decimal::Decimal::from_str(&amount_str).unwrap_or(rust_decimal::Decimal::ZERO);
                if usdc_balance < need {
                    anyhow::bail!("Insufficient USDC balance. Required: {}, Available: {}", amount_str, usdc_balance);
                }
            }
        } else if amount <= 0.0 {
            anyhow::bail!("Invalid shares amount: {}. Must be > 0.", amount);
        } else if amount < 0.01 {
            // CLOB conditional-token shares are effectively quantized to 2 decimals.
            // Smaller values become 0.00 and are rejected by /order.
            anyhow::bail!(
                "Sell amount {} is below minimum lot size (0.01 shares) and would round to 0.00.",
                amount
            );
        }
        let handle = self.ensure_clob_client().await?;
        let max_retries = if is_buy { 2 } else { 6 };
        let mut current_amount = normalized_amount;
        for attempt in 1..=max_retries {
            let amount_str = format!("{:.8}", current_amount);
            match clob_sdk::post_market_order(handle, token_id, side, &amount_str, is_buy, ot) {
                Ok(order_id) => {
                    log::info!("   ✅ Posted | Order {}", order_id);
                    return Ok(OrderResponse {
                        order_id: Some(order_id.clone()),
                        status: "LIVE".to_string(),
                        message: Some(format!("Market order executed. Order ID: {}", order_id)),
                    });
                }
                Err(e) => {
                    let err_s = format!("{}", e);
                    log::warn!("post_market_order failed: {}", err_s);
                    let is_allowance = err_s.contains("allowance");
                    let is_balance = err_s.contains("balance") && !err_s.contains("allowance");
                    let is_signer = err_s.contains("signer") || err_s.contains("invalid: signer");
                    if is_signer {
                        let hint = self.proxy_wallet_address.as_deref()
                            .map(|p| format!(" Your config has proxy_wallet_address = {}. Create the Polymarket API key in the CLOB dashboard for that same wallet (your proxy/Safe).", p))
                            .unwrap_or_else(|| " The API key in config.json must have been created for the wallet that private_key represents.".to_string());
                        anyhow::bail!("CLOB rejected: invalid signer.{}", hint);
                    }
                    if is_balance {
                        anyhow::bail!("Insufficient token balance: {}", e);
                    }
                    if is_buy
                        && Self::is_buy_min_size_error(&err_s)
                        && current_amount < 1.01
                        && attempt < max_retries
                    {
                        current_amount = 1.01;
                        log::warn!(
                            "BUY min-size guard: retrying with amount {:.2} USD",
                            current_amount
                        );
                        continue;
                    }
                    if is_allowance && !is_buy && attempt < max_retries {
                        let _ = self.update_balance_allowance_for_sell(token_id).await;
                        tokio::time::sleep(tokio::time::Duration::from_millis(700 * attempt as u64)).await;
                        continue;
                    }
                    anyhow::bail!("Failed to post market order: {}", e);
                }
            }
        }
        unreachable!()
    }

    /// Same as place_market_order but skips the USDC balance pre-check to save one RTT on the hot path (copy-trading).
    /// If balance is insufficient, the CLOB will reject the order and we return that error.
    pub async fn place_market_order_fast(
        &self,
        token_id: &str,
        amount: f64,
        side: &str,
        order_type: Option<&str>,
    ) -> Result<OrderResponse> {
        if side != "BUY" && side != "SELL" {
            anyhow::bail!("Invalid order side: {}. Must be 'BUY' or 'SELL'", side);
        }
        let ot = order_type.unwrap_or("FAK");
        if ot != "FOK" && ot != "FAK" {
            anyhow::bail!("Invalid order_type: {}. Must be 'FOK' or 'FAK'", ot);
        }
        let is_buy = side == "BUY";
        let normalized_amount = if is_buy {
            Self::normalize_buy_usdc_amount(amount)
        } else {
            amount
        };
        let amount_str = format!("{:.8}", normalized_amount);
        if is_buy && normalized_amount <= 0.0 {
            anyhow::bail!(
                "Buy amount {} is below minimum precision after normalization (2 decimals).",
                amount
            );
        }
        if !is_buy && amount <= 0.0 {
            anyhow::bail!("Invalid shares amount: {}. Must be > 0.", amount);
        } else if !is_buy && amount < 0.01 {
            anyhow::bail!(
                "Sell amount {} is below minimum lot size (0.01 shares) and would round to 0.00.",
                amount
            );
        }
        let handle = self.ensure_clob_client().await?;
        let max_retries = if is_buy { 2 } else { 6 };
        let mut current_amount = normalized_amount;
        for attempt in 1..=max_retries {
            let amount_str = format!("{:.8}", current_amount);
            match clob_sdk::post_market_order(handle, token_id, side, &amount_str, is_buy, ot) {
                Ok(order_id) => {
                    log::info!("   ✅ Posted | Order {}", order_id);
                    return Ok(OrderResponse {
                        order_id: Some(order_id.clone()),
                        status: "LIVE".to_string(),
                        message: Some(format!("Market order executed. Order ID: {}", order_id)),
                    });
                }
                Err(e) => {
                    let err_s = format!("{}", e);
                    log::warn!("post_market_order failed: {}", err_s);
                    let is_allowance = err_s.contains("allowance");
                    let is_balance = err_s.contains("balance") && !err_s.contains("allowance");
                    let is_signer = err_s.contains("signer") || err_s.contains("invalid: signer");
                    if is_signer {
                        let hint = self.proxy_wallet_address.as_deref()
                            .map(|p| format!(" Your config has proxy_wallet_address = {}. Polymarket API keys are tied to one wallet: create the API key in Polymarket CLOB dashboard while using that same wallet (your proxy/Safe), or switch to the wallet the key was created for.", p))
                            .unwrap_or_else(|| " Polymarket API keys are tied to one wallet: the API key in config.json must have been created for the same wallet that private_key (or proxy_wallet_address) represents.".to_string());
                        anyhow::bail!("CLOB rejected order: invalid signer.{}", hint);
                    }
                    if is_balance {
                        anyhow::bail!("Insufficient balance: {}", e);
                    }
                    if is_buy
                        && Self::is_buy_min_size_error(&err_s)
                        && current_amount < 1.01
                        && attempt < max_retries
                    {
                        current_amount = 1.01;
                        log::warn!(
                            "BUY min-size guard: retrying with amount {:.2} USD",
                            current_amount
                        );
                        continue;
                    }
                    if is_allowance && !is_buy && attempt < max_retries {
                        let _ = self.update_balance_allowance_for_sell(token_id).await;
                        tokio::time::sleep(tokio::time::Duration::from_millis(400 * attempt as u64)).await;
                        continue;
                    }
                    anyhow::bail!("Failed to post market order: {}", e);
                }
            }
        }
        unreachable!()
    }

    pub async fn place_market_orders(
        &self,
        orders: &[(&str, f64, &str, Option<&str>)], // (token_id, amount, side, order_type)
    ) -> Result<Vec<OrderResponse>> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }
        let handle = self.ensure_clob_client().await?;
        let mut out = Vec::with_capacity(orders.len());
        for (i, (token_id, amount, side, order_type)) in orders.iter().enumerate() {
            let ot = order_type.unwrap_or("FOK");
            if *side != "BUY" && *side != "SELL" {
                out.push(OrderResponse {
                    order_id: None,
                    status: "REJECTED".to_string(),
                    message: Some(format!("Invalid order side: {}", side)),
                });
                continue;
            }
            let amount_str = format!("{:.8}", amount);
            let is_buy = *side == "BUY";
            match clob_sdk::post_market_order(handle, token_id, side, &amount_str, is_buy, ot) {
                Ok(order_id) => out.push(OrderResponse {
                    order_id: Some(order_id.clone()),
                    status: "LIVE".to_string(),
                    message: Some(format!("Order ID: {}", order_id)),
                }),
                Err(e) => {
                    eprintln!("   Order {} (token {}...) rejected: {}", i + 1, &token_id[..token_id.len().min(16)], e);
                    out.push(OrderResponse {
                        order_id: None,
                        status: "REJECTED".to_string(),
                        message: Some(e.to_string()),
                    });
                }
            }
        }
        Ok(out)
    }
    
    #[allow(dead_code)]
    async fn place_order_hmac(&self, order: &OrderRequest) -> Result<OrderResponse> {
        let path = "/orders";
        let url = format!("{}{}", self.clob_url, path);
        
        // Serialize order to JSON string for signature
        let body = serde_json::to_string(order)
            .context("Failed to serialize order to JSON")?;
        
        let mut request = self.client.post(&url).json(order);
        
        // Add HMAC-SHA256 authentication headers (L2 authentication)
        request = self.add_auth_headers(request, "POST", path, &body)
            .context("Failed to add authentication headers")?;

        eprintln!("📤 Posting order to Polymarket (HMAC): {} {} {} @ {}", 
              order.side, order.size, order.token_id, order.price);

        let response = request
            .send()
            .await
            .context("Failed to place order")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            
            // Provide helpful error messages
            if status == 401 || status == 403 {
                anyhow::bail!(
                    "Authentication failed (status: {}): {}",
                    status, error_text
                );
            }
            
            anyhow::bail!("Failed to place order (status: {}): {}", status, error_text);
        }

        let order_response: OrderResponse = response
            .json()
            .await
            .context("Failed to parse order response")?;

        eprintln!("✅ Order placed successfully: {:?}", order_response);
        Ok(order_response)
    }

    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some() && self.api_passphrase.is_some()
    }

    pub fn get_wallet_address(&self) -> Result<String> {
        if let Some(ref proxy) = self.proxy_wallet_address {
            return Ok(proxy.clone());
        }
        let pk = self.private_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("private_key required to get wallet address"))?;
        let signer = LocalSigner::from_str(pk)
            .context("Invalid private_key")?;
        Ok(format!("{}", signer.address()))
    }

    pub async fn get_positions(&self, user: &str) -> Result<Vec<DataApiPosition>> {
        const PAGE_SIZE: u32 = 500;
        const MAX_OFFSET: u32 = 10_000;
        let user = if user.starts_with("0x") { user.to_string() } else { format!("0x{}", user) };
        let mut all = Vec::with_capacity(PAGE_SIZE as usize * 2);
        let mut offset = 0u32;
        while offset <= MAX_OFFSET {
            let limit_str = PAGE_SIZE.to_string();
            let offset_str = offset.to_string();
            let response = self.client
                .get("https://data-api.polymarket.com/positions")
                .query(&[("user", user.as_str()), ("limit", &limit_str), ("offset", &offset_str)])
                .send()
                .await
                .context("Failed to fetch positions")?;
            if !response.status().is_success() {
                anyhow::bail!("Data API positions returned {}", response.status());
            }
            let page: Vec<Value> = response.json().await.unwrap_or_default();
            for p in &page {
                let size = p.get("size")
                    .and_then(|v| v.as_f64())
                    .or_else(|| p.get("size").and_then(|v| v.as_u64().map(|u| u as f64)))
                    .or_else(|| p.get("size").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()));
                let size = match size { Some(s) => s, None => continue };
                let cur_price = p.get("curPrice")
                    .or(p.get("cur_price"))
                    .and_then(|v| v.as_f64().or_else(|| v.as_u64().map(|u| u as f64)).or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok())))
                    .unwrap_or(0.0);
                if size <= 0.0 || cur_price <= 0.0 || (cur_price - 1.0).abs() < 1e-9 {
                    continue;
                }
                let asset = match p.get("asset").and_then(|a| a.as_str()) {
                    Some(a) => a.to_string(),
                    None => continue,
                };
                let end_date = p.get("endDate").or(p.get("end_date")).and_then(|v| v.as_str()).map(String::from);
                if let Some(ref ed) = end_date {
                    if chrono::DateTime::parse_from_rfc3339(ed).map(|t| t.timestamp() < chrono::Utc::now().timestamp()).unwrap_or(false) {
                        continue;
                    }
                }
                all.push(DataApiPosition {
                    asset,
                    size,
                    cur_price,
                    condition_id: p.get("conditionId").or(p.get("condition_id")).and_then(|c| c.as_str()).map(String::from),
                    end_date,
                    slug: p.get("slug").and_then(|s| s.as_str()).map(String::from),
                    outcome: p.get("outcome").and_then(|o| o.as_str()).map(String::from),
                });
            }
            if page.len() < PAGE_SIZE as usize {
                break;
            }
            offset += PAGE_SIZE;
        }
        Ok(all)
    }

    pub async fn get_redeemable_positions(&self, wallet: &str) -> Result<Vec<String>> {
        let url = "https://data-api.polymarket.com/positions";
        let user = if wallet.starts_with("0x") {
            wallet.to_string()
        } else {
            format!("0x{}", wallet)
        };
        let response = self.client
            .get(url)
            .query(&[("user", user.as_str()), ("redeemable", "true"), ("limit", "500")])
            .send()
            .await
            .context("Failed to fetch redeemable positions")?;
        if !response.status().is_success() {
            anyhow::bail!("Data API returned {} for redeemable positions", response.status());
        }
        let positions: Vec<Value> = response.json().await.unwrap_or_default();
        let mut condition_ids: Vec<String> = positions
            .iter()
            .filter(|p| {
                let size = p.get("size")
                    .and_then(|s| s.as_f64())
                    .or_else(|| p.get("size").and_then(|s| s.as_u64().map(|u| u as f64)))
                    .or_else(|| p.get("size").and_then(|s| s.as_str()).and_then(|s| s.parse::<f64>().ok()));
                size.map(|s| s > 0.0).unwrap_or(false)
            })
            .filter_map(|p| p.get("conditionId").and_then(|c| c.as_str()).map(|s| {
                if s.starts_with("0x") { s.to_string() } else { format!("0x{}", s) }
            }))
            .collect();
        condition_ids.sort();
        condition_ids.dedup();
        Ok(condition_ids)
    }

    fn derive_safe_address_polygon(eoa: &AlloyAddress) -> AlloyAddress {
        const SAFE_FACTORY_POLYGON: [u8; 20] = [
            0xaa, 0xcf, 0xee, 0xa0, 0x3e, 0xb1, 0x56, 0x1c, 0x4e, 0x67,
            0xd6, 0x61, 0xe4, 0x06, 0x82, 0xbd, 0x20, 0xe3, 0x54, 0x1b,
        ];
        const SAFE_INIT_CODE_HASH: [u8; 32] = [
            0x2b, 0xce, 0x21, 0x27, 0xff, 0x07, 0xfb, 0x63, 0x2d, 0x16, 0xc8, 0x34, 0x7c, 0x4e, 0xbf, 0x50,
            0x1f, 0x48, 0x41, 0x16, 0x8b, 0xed, 0x00, 0xd9, 0xe6, 0xef, 0x71, 0x5d, 0xdb, 0x6f, 0xce, 0xcf,
        ];
        // Salt = keccak256(abi.encode(address)) — 32 bytes: 12 zero + 20 byte address
        let mut salt_input = [0u8; 32];
        salt_input[12..32].copy_from_slice(eoa.as_slice());
        let salt = keccak256(salt_input);
        // CREATE2: keccak256(0xff ++ deployer (20) ++ salt (32) ++ initCodeHash (32))[12..32]
        let mut preimage = Vec::with_capacity(85);
        preimage.push(0xff);
        preimage.extend_from_slice(&SAFE_FACTORY_POLYGON);
        preimage.extend_from_slice(salt.as_slice());
        preimage.extend_from_slice(&SAFE_INIT_CODE_HASH);
        let hash = keccak256(&preimage);
        AlloyAddress::from_slice(&hash[12..32])
    }

    pub async fn redeem_tokens(
        &self,
        condition_id: &str,
        _token_id: &str,
        outcome: &str,
    ) -> Result<RedeemResponse> {
        let private_key = self.private_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Private key is required for order signing. Please set private_key in config.json"))?;

        let signer = LocalSigner::from_str(private_key)
            .context("Failed to create signer from private key. Ensure private_key is a valid hex string.")?
            .with_chain_id(Some(polygon()));

        let parse_address_hex = |s: &str| -> Result<AlloyAddress> {
            let hex_str = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(hex_str).context("Invalid hex in address")?;
            let len = bytes.len();
            let arr: [u8; 20] = bytes.try_into().map_err(|_| anyhow::anyhow!("Address must be 20 bytes, got {}", len))?;
            Ok(AlloyAddress::from(arr))
        };

        let collateral_token = parse_address_hex("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
            .context("Failed to parse USDC address")?;

        let condition_id_clean = condition_id.strip_prefix("0x").unwrap_or(condition_id);
        let condition_id_b256 = B256::from_str(condition_id_clean)
            .context(format!("Failed to parse condition_id as B256: {}", condition_id))?;

        let index_set = if outcome.to_uppercase().contains("UP") || outcome == "1" {
            U256::from(1)
        } else {
            U256::from(2)
        };

        eprintln!("Redeeming winning tokens for condition {} (outcome: {}, index_set: {})",
              condition_id, outcome, index_set);

        const CTF_CONTRACT: &str = "0x4d97dcd97ec945f40cf65f87097ace5ea0476045";
        let rpc_url = Self::polygon_rpc_url();
        const PROXY_WALLET_FACTORY: &str = "0xaB45c5A4B0c941a2F231C04C3f49182e1A254052";

        let ctf_address = parse_address_hex(CTF_CONTRACT)
            .context("Failed to parse CTF contract address")?;

        let parent_collection_id = B256::ZERO;
        let use_proxy = self.proxy_wallet_address.is_some();
        let sig_type = self.signature_type.unwrap_or(1);
        // Use winning outcome only (same as EOA/Proxy). Passing [1,2] can revert if Safe doesn't hold both outcomes.
        let index_sets: Vec<U256> = vec![index_set];

        eprintln!("   Prepared redemption parameters:");
        eprintln!("   - CTF Contract: {}", ctf_address);
        eprintln!("   - Collateral token (USDC): {}", collateral_token);
        eprintln!("   - Condition ID: {} ({:?})", condition_id, condition_id_b256);
        eprintln!("   - Index set(s): {:?} (outcome: {})", index_sets, outcome);

        // Manual ABI encode for redeemPositions(address,bytes32,bytes32,uint256[])
        let selector = hex::decode("3d7d3f5a").context("redeemPositions selector")?;
        let mut redeem_calldata = Vec::new();
        redeem_calldata.extend_from_slice(&selector);
        let mut addr_bytes = [0u8; 32];
        addr_bytes[12..].copy_from_slice(collateral_token.as_slice());
        redeem_calldata.extend_from_slice(&addr_bytes);
        redeem_calldata.extend_from_slice(parent_collection_id.as_slice());
        redeem_calldata.extend_from_slice(condition_id_b256.as_slice());
        let array_offset = 32u32 * 4;
        redeem_calldata.extend_from_slice(&U256::from(array_offset).to_be_bytes::<32>());
        redeem_calldata.extend_from_slice(&U256::from(index_sets.len()).to_be_bytes::<32>());
        for idx in &index_sets {
            redeem_calldata.extend_from_slice(&idx.to_be_bytes::<32>());
        }

        // sig_type 2: proxy_wallet_address must be a Gnosis Safe (v1.3); it must have nonce(), getTransactionHash(), execTransaction()
        let (tx_to, tx_data, gas_limit, used_safe_redemption) = if use_proxy && sig_type == 2 {
            let safe_address_str = self.proxy_wallet_address.as_deref()
                .ok_or_else(|| anyhow::anyhow!("proxy_wallet_address required for Safe redemption"))?;
            let safe_address = parse_address_hex(safe_address_str)
                .context("Failed to parse proxy_wallet_address (Safe address)")?;
            eprintln!("   Using Gnosis Safe (sig_type 2): signing and executing redemption via Safe.execTransaction");
            let nonce_selector = keccak256("nonce()".as_bytes());
            let nonce_calldata: Vec<u8> = nonce_selector.as_slice()[..4].to_vec();
            let provider_read = ProviderBuilder::new()
                .connect(&rpc_url)
                .await
                .context("Failed to connect to RPC for Safe read calls")?;
            let nonce_tx = TransactionRequest::default()
                .to(safe_address)
                .input(Bytes::from(nonce_calldata.clone()).into());
            let nonce_result = match provider_read.call(nonce_tx).await {
                Ok(r) => r,
                Err(e) => {
                    anyhow::bail!(
                        "Safe.nonce() failed: {}. \
                        For sig_type 2, proxy_wallet_address must be a Gnosis Safe (has nonce()). \
                        If you use Polymarket proxy (Magic Link/Google), use signature_type: 1 in config.",
                        e
                    );
                }
            };
            let nonce_bytes: [u8; 32] = nonce_result.as_ref().try_into()
                .map_err(|_| anyhow::anyhow!("Safe.nonce() did not return 32 bytes"))?;
            let nonce = U256::from_be_slice(&nonce_bytes);
            const SAFE_TX_GAS: u64 = 300_000;
            let get_tx_hash_sig = "getTransactionHash(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,uint256)";
            let get_tx_hash_selector = keccak256(get_tx_hash_sig.as_bytes()).as_slice()[..4].to_vec();
            let zero_addr = [0u8; 32];
            let mut to_enc = [0u8; 32];
            to_enc[12..].copy_from_slice(ctf_address.as_slice());
            let data_offset_get_hash = U256::from(32u32 * 10u32);
            let mut get_tx_hash_calldata = Vec::new();
            get_tx_hash_calldata.extend_from_slice(&get_tx_hash_selector);
            get_tx_hash_calldata.extend_from_slice(&to_enc);
            get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&data_offset_get_hash.to_be_bytes::<32>());
            get_tx_hash_calldata.push(0); get_tx_hash_calldata.extend_from_slice(&[0u8; 31]);
            get_tx_hash_calldata.extend_from_slice(&U256::from(SAFE_TX_GAS).to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&zero_addr);
            get_tx_hash_calldata.extend_from_slice(&zero_addr);
            get_tx_hash_calldata.extend_from_slice(&nonce.to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&U256::from(redeem_calldata.len()).to_be_bytes::<32>());
            get_tx_hash_calldata.extend_from_slice(&redeem_calldata);
            let get_tx_hash_tx = TransactionRequest::default()
                .to(safe_address)
                .input(Bytes::from(get_tx_hash_calldata).into());
            let tx_hash_result = provider_read.call(get_tx_hash_tx).await
                .context("Failed to call Safe.getTransactionHash()")?;
            let tx_hash_to_sign: B256 = tx_hash_result.as_ref().try_into()
                .map_err(|_| anyhow::anyhow!("getTransactionHash did not return 32 bytes"))?;
            const EIP191_PREFIX: &[u8] = b"\x19Ethereum Signed Message:\n32";
            let mut eip191_message = Vec::with_capacity(EIP191_PREFIX.len() + 32);
            eip191_message.extend_from_slice(EIP191_PREFIX);
            eip191_message.extend_from_slice(tx_hash_to_sign.as_slice());
            let hash_to_sign = keccak256(&eip191_message);
            let sig = signer.sign_hash(&hash_to_sign).await
                .context("Failed to sign Safe transaction hash")?;
            let sig_bytes = sig.as_bytes();
            let r = &sig_bytes[0..32];
            let s = &sig_bytes[32..64];
            let v = sig_bytes[64];
            let v_safe = if v == 27 || v == 28 { v + 4 } else { v };
            let mut packed_sig: Vec<u8> = Vec::with_capacity(85);
            packed_sig.extend_from_slice(r);
            packed_sig.extend_from_slice(s);
            packed_sig.extend_from_slice(&[v_safe]);
            let get_threshold_selector = keccak256("getThreshold()".as_bytes()).as_slice()[..4].to_vec();
            let threshold_tx = TransactionRequest::default()
                .to(safe_address)
                .input(Bytes::from(get_threshold_selector).into());
            let threshold_result = provider_read.call(threshold_tx).await
                .context("Failed to call Safe.getThreshold()")?;
            let threshold_bytes: [u8; 32] = threshold_result.as_ref().try_into()
                .map_err(|_| anyhow::anyhow!("getThreshold did not return 32 bytes"))?;
            let threshold = U256::from_be_slice(&threshold_bytes);
            if threshold > U256::from(1) {
                let owner = signer.address();
                let mut with_owner = Vec::with_capacity(20 + packed_sig.len());
                with_owner.extend_from_slice(owner.as_slice());
                with_owner.extend_from_slice(&packed_sig);
                packed_sig = with_owner;
            }
            let safe_sig_bytes = packed_sig;
            let exec_sig = "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)";
            let exec_selector = keccak256(exec_sig.as_bytes()).as_slice()[..4].to_vec();
            let data_offset = 32u32 * 10u32;
            let sigs_offset = data_offset + 32 + redeem_calldata.len() as u32;
            let mut exec_calldata = Vec::new();
            exec_calldata.extend_from_slice(&exec_selector);
            exec_calldata.extend_from_slice(&to_enc);
            exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&U256::from(data_offset).to_be_bytes::<32>());
            exec_calldata.push(0); exec_calldata.extend_from_slice(&[0u8; 31]);
            exec_calldata.extend_from_slice(&U256::from(SAFE_TX_GAS).to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&zero_addr);
            exec_calldata.extend_from_slice(&zero_addr);
            exec_calldata.extend_from_slice(&U256::from(sigs_offset).to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&U256::from(redeem_calldata.len()).to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&redeem_calldata);
            exec_calldata.extend_from_slice(&U256::from(safe_sig_bytes.len()).to_be_bytes::<32>());
            exec_calldata.extend_from_slice(&safe_sig_bytes);
            (safe_address, exec_calldata, 400_000u64, true)
        } else if use_proxy && sig_type == 1 {
            eprintln!("   Using proxy wallet: sending redemption via Proxy Wallet Factory");
            let factory_address = parse_address_hex(PROXY_WALLET_FACTORY)
                .context("Failed to parse Proxy Wallet Factory address")?;
            let selector = keccak256("proxy((uint8,address,uint256,bytes)[])".as_bytes());
            let proxy_selector = &selector.as_slice()[..4];
            let mut proxy_calldata = Vec::with_capacity(4 + 32 * 3 + 128 + 32 + redeem_calldata.len());
            proxy_calldata.extend_from_slice(proxy_selector);
            proxy_calldata.extend_from_slice(&U256::from(32u32).to_be_bytes::<32>());
            proxy_calldata.extend_from_slice(&U256::from(1u32).to_be_bytes::<32>());
            proxy_calldata.extend_from_slice(&U256::from(96u32).to_be_bytes::<32>());
            let mut type_code = [0u8; 32];
            type_code[31] = 1;
            proxy_calldata.extend_from_slice(&type_code);
            let mut to_bytes = [0u8; 32];
            to_bytes[12..].copy_from_slice(ctf_address.as_slice());
            proxy_calldata.extend_from_slice(&to_bytes);
            proxy_calldata.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
            proxy_calldata.extend_from_slice(&U256::from(128u32).to_be_bytes::<32>());
            let data_len = redeem_calldata.len();
            proxy_calldata.extend_from_slice(&U256::from(data_len).to_be_bytes::<32>());
            proxy_calldata.extend_from_slice(&redeem_calldata);
            (factory_address, proxy_calldata, 400_000u64, false)
        } else {
            eprintln!("   Sending redemption from EOA to CTF contract");
            (ctf_address, redeem_calldata, 300_000, false)
        };

        let provider = ProviderBuilder::new()
            .wallet(signer.clone())
            .connect(&rpc_url)
            .await
            .context("Failed to connect to Polygon RPC")?;

        let tx_request = TransactionRequest::default()
            .to(tx_to)
            .input(Bytes::from(tx_data).into())
            .value(U256::ZERO)
            .gas_limit(gas_limit);

        let pending_tx = match provider.send_transaction(tx_request).await {
            Ok(tx) => tx,
            Err(e) => {
                let err_msg = format!("Failed to send redeem transaction: {}", e);
                eprintln!("   {}", err_msg);
                anyhow::bail!("{}", err_msg);
            }
        };

        let tx_hash = *pending_tx.tx_hash();
        eprintln!("   Transaction sent, waiting for confirmation...");
        eprintln!("   Transaction hash: {:?}", tx_hash);

        let receipt = pending_tx.get_receipt().await
            .context("Failed to get transaction receipt")?;

        if !receipt.status() {
            anyhow::bail!("Redemption transaction failed. Transaction hash: {:?}", tx_hash);
        }

        if used_safe_redemption {
            let payout_redemption_topic = keccak256(
                b"PayoutRedemption(address,address,bytes32,bytes32,uint256[],uint256)"
            );
            let logs = receipt.logs();
            let ctf_has_payout = logs.iter().any(|log| {
                log.address() == ctf_address && log.topics().first().map(|t| t.as_slice()) == Some(payout_redemption_topic.as_slice())
            });
            if !ctf_has_payout {
                anyhow::bail!(
                    "Redemption tx was mined but the inner redeem reverted (no PayoutRedemption from CTF). \
                    Possible causes: (1) Safe does not hold conditional tokens for this conditionId – check Safe balance on Polygonscan; (2) tokens already redeemed; (3) conditionId/outcome mismatch. Tx: {:?}",
                    tx_hash
                );
            }
        }

        let redeem_response = RedeemResponse {
            success: true,
            message: Some(format!("Successfully redeemed tokens. Transaction: {:?}", tx_hash)),
            transaction_hash: Some(format!("{:?}", tx_hash)),
            amount_redeemed: None,
        };
        eprintln!("Successfully redeemed winning tokens!");
        eprintln!("Transaction hash: {:?}", tx_hash);
        if let Some(block_number) = receipt.block_number {
            eprintln!("Block number: {}", block_number);
        }
        Ok(redeem_response)
    }

    pub async fn merge_complete_sets(&self, condition_id: &str) -> Result<RedeemResponse> {
        self.redeem_tokens(condition_id, "", "Up+Down (merge complete sets)").await
    }
}

