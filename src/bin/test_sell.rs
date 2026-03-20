use anyhow::Result;
use clap::Parser;
use rust_decimal::Decimal;
use polymarket_trading_bot::{PolymarketApi, Config};
use polymarket_trading_bot::models::TokenPrice;

const CLOB_UNITS_SCALE: u64 = 1_000_000;

#[derive(Parser, Debug)]
#[command(name = "test_sell")]
#[command(about = "Test selling tokens from your portfolio")]
struct Args {
    #[arg(short, long)]
    token_id: Option<String>,
    
    #[arg(short, long)]
    shares: Option<f64>,
    
    #[arg(short, long, default_value = "config.json")]
    config: String,
    
    #[arg(long)]
    check_only: bool,
    
    #[arg(long)]
    list: bool,
    
    #[arg(long)]
    sell_all: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();
    let config_path = std::path::PathBuf::from(&args.config);
    let config = Config::load(&config_path)?;

    // Create API client
    let api = PolymarketApi::new(
        config.polymarket.gamma_api_url.clone(),
        config.polymarket.clob_api_url.clone(),
        config.polymarket.api_key.clone(),
        config.polymarket.api_secret.clone(),
        config.polymarket.api_passphrase.clone(),
        config.polymarket.private_key.clone(),
        config.polymarket.proxy_wallet_address.clone(),
        config.polymarket.signature_type,
    );

    // If --list, --sell-all flag, or no token_id provided, scan portfolio
    if args.list || args.sell_all || args.token_id.is_none() {
        println!("🔍 Scanning your portfolio for tokens with balance...\n");
        
        // Get condition IDs from config
        let btc_condition_id = config.trading.btc_condition_id.as_deref();
        let eth_condition_id = config.trading.eth_condition_id.as_deref();
        
        let tokens_result: Result<Vec<(String, f64, String)>, _> = api.get_portfolio_tokens(btc_condition_id, eth_condition_id).await;
        match tokens_result {
            Ok(tokens) => {
                if tokens.is_empty() {
                    println!("   ⚠️  No tokens found with balance > 0");
                    println!("\n💡 Tips:");
                    println!("   - Make sure you've bought tokens from your portfolio");
                    println!("   - Check that BTC/ETH condition IDs are set in config.json");
                    println!("   - Try buying a token manually and run this again");
                    return Ok(());
                }
                
                println!("📋 Found {} token(s) with balance:\n", tokens.len());
                for (idx, (token_id, balance, description)) in tokens.iter().enumerate() {
                    println!("   {}. {} - Balance: {:.6} shares", idx + 1, description, balance);
                    println!("      Token ID: {}", token_id);
                    
                    // Get current price
                    if let Ok(Some(price)) = api.get_best_price(token_id).await {
                        if let Some(bid) = price.bid {
                            println!("      Current BID (sell) price: ${:.6}", bid);
                            println!("      Estimated value: ${:.6}", f64::try_from(bid).unwrap_or(0.0) * balance);
                        }
                    }
                    println!();
                }
                
                if args.check_only || (args.list && !args.sell_all) {
                    println!("✅ Portfolio scan complete");
                    println!("\n💡 To sell all tokens, run:");
                    println!("   cargo run --bin test_sell -- --sell-all");
                    println!("\n💡 To sell a specific token, run:");
                    println!("   cargo run --bin test_sell -- --token-id <TOKEN_ID>");
                    return Ok(());
                }
                
                // If --sell-all, sell all tokens
                if args.sell_all {
                    println!("💰 Selling all {} token(s) in portfolio...\n", tokens.len());
                    let mut success_count = 0;
                    let mut fail_count = 0;
                    
                    for (token_id, balance, description) in &tokens {
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        println!("Selling: {} (Balance: {:.6} shares)", description, balance);
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
                        
                        match sell_token(&api, token_id, *balance, None, false).await {
                            Ok(_) => {
                                success_count += 1;
                                println!("✅ Successfully sold {}\n", description);
                            }
                            Err(e) => {
                                fail_count += 1;
                                eprintln!("❌ Failed to sell {}: {}\n", description, e);
                            }
                        }
                    }
                    
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    println!("📊 Summary:");
                    println!("   ✅ Successfully sold: {} token(s)", success_count);
                    println!("   ❌ Failed: {} token(s)", fail_count);
                    println!("   📦 Total: {} token(s)", tokens.len());
                    return Ok(());
                }
                
                // If no token_id specified but we have tokens, use the first one
                if args.token_id.is_none() && !tokens.is_empty() {
                    println!("💰 No token ID specified. Using first token: {}\n", tokens[0].2);
                    // Continue with first token
                    return sell_token(&api, &tokens[0].0, tokens[0].1, args.shares, args.check_only).await;
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to scan portfolio: {}", e);
                eprintln!("\n💡 You can still specify a token ID manually:");
                eprintln!("   cargo run --bin test_sell -- --token-id <TOKEN_ID>");
                return Err(e);
            }
        }
    }
    
    // If token_id is provided, sell that specific token
    let token_id = args.token_id.as_ref().ok_or_else(|| anyhow::anyhow!("Token ID is required. Use --list to scan portfolio first."))?;
    
    println!("🔍 Checking your portfolio...\n");
    println!("📊 Checking token: {}\n", token_id);
    
    match api.check_balance_allowance(token_id).await {
        Ok((balance, allowance)) => {
            // CLOB returns conditional token balance/allowance in base units (1e6 scale).
            // Convert to human shares for display and order sizing.
            let balance_decimal = balance / Decimal::from(CLOB_UNITS_SCALE);
            let allowance_decimal = allowance / Decimal::from(CLOB_UNITS_SCALE);
            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
            let allowance_f64 = f64::try_from(allowance_decimal).unwrap_or(0.0);

            println!("   ✅ Raw Balance: {}", balance_decimal);
            println!("   ✅ Raw Allowance: {}", allowance_decimal);
            println!("   ✅ Balance (f64): {:.10} shares", balance_f64);
            println!("   ✅ Allowance (f64): {:.10} shares", allowance_f64);

            if balance_f64 == 0.0 {
                println!("   ⚠️  No balance for this token. Nothing to sell.");
                return Ok(());
            }

            // Get current price
            if let Some(ref token_id) = args.token_id {
                if let Ok(Some(price)) = api.get_best_price(token_id).await {
                    println!("   📈 Current Prices:");
                    if let Some(bid) = price.bid {
                        println!("      BID (sell price): ${:.6}", bid);
                    }
                    if let Some(ask) = price.ask {
                        println!("      ASK (buy price): ${:.6}", ask);
                    }
                }
            }

            if args.check_only {
                println!("\n✅ Portfolio check complete (--check-only mode, not selling)");
                return Ok(());
            }

            // Sell the token
            sell_token(&api, token_id, balance_f64, args.shares, false).await?;
        }
        Err(e) => {
            eprintln!("❌ Failed to check balance: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

async fn sell_token(
    api: &PolymarketApi,
    token_id: &str,
    balance: f64,
    shares: Option<f64>,
    check_only: bool,
) -> Result<()> {
    if check_only {
        return Ok(());
    }

    // Check balance and allowance before selling
    println!("   🔍 Checking token balance and allowance before selling...");
    
    // Determine how many shares to sell (will be updated after balance check)
    let mut shares_to_sell = shares.unwrap_or(balance);
    
    match api.check_balance_allowance(token_id).await {
        Ok((balance_decimal, allowance_decimal)) => {
            let balance_f64 = f64::try_from(balance_decimal / Decimal::from(CLOB_UNITS_SCALE)).unwrap_or(0.0);
            let allowance_f64 = f64::try_from(allowance_decimal / Decimal::from(CLOB_UNITS_SCALE)).unwrap_or(0.0);
            
            println!("   📊 Balance & Allowance Check:");
            println!("      Token Balance: {:.6} shares", balance_f64);
            println!("      Token Allowance: {:.6} shares", allowance_f64);
            
            // Update shares_to_sell based on actual balance
            shares_to_sell = shares.unwrap_or(balance_f64);
            if shares_to_sell > balance_f64 {
                println!("   ⚠️  Requested shares ({:.6}) > balance ({:.6}), selling all available", 
                    shares_to_sell, balance_f64);
                shares_to_sell = balance_f64;
            }
            
            println!("      Shares to Sell: {:.6} shares", shares_to_sell);
            
            // Check if we have enough balance
            if balance_f64 < shares_to_sell {
                anyhow::bail!("Insufficient balance: {:.6} < {:.6}", balance_f64, shares_to_sell);
            }
            
            // Check if we have enough allowance (for proxy wallets)
            if allowance_f64 < shares_to_sell {
                println!("   ⚠️  WARNING: Token allowance ({:.6}) is less than shares to sell ({:.6})", 
                    allowance_f64, shares_to_sell);
                println!("   📝 Token Allowance Explanation:");
                println!("      - Token allowance is permission for the CLOB contract to spend your tokens");
                println!("      - Required for proxy wallets before selling tokens");
                println!("      - Current allowance: {:.6} shares", allowance_f64);
                println!("      - Required: {:.6} shares", shares_to_sell);
                println!("   🔄 Trying allowance cache refresh first...");
                api.update_balance_allowance_for_sell(token_id).await?;
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

                let mut refreshed_allowance_ok = false;
                match api.check_balance_allowance(token_id).await {
                    Ok((_balance_decimal2, allowance_decimal2)) => {
                        let allowance2_f64 = f64::try_from(allowance_decimal2 / Decimal::from(CLOB_UNITS_SCALE)).unwrap_or(0.0);
                        println!("   🔍 Allowance after refresh: {:.6} shares", allowance2_f64);
                        if allowance2_f64 >= shares_to_sell {
                            refreshed_allowance_ok = true;
                        }
                    }
                    Err(e) => {
                        println!("   ⚠️  Allowance re-check after refresh failed: {}", e);
                    }
                }

                if !refreshed_allowance_ok {
                    let is_approved_for_all = api.check_is_approved_for_all().await.unwrap_or(false);
                    if is_approved_for_all {
                        println!(
                            "   ⚠️  Allowance cache still shows 0, but setApprovalForAll is true."
                        );
                        println!(
                            "   🔄 Proceeding with sell attempt anyway (CLOB backend can lag before allowance refresh appears)."
                        );
                    } else {
                        println!("   🔄 setApprovalForAll is not set. Attempting on-chain approval...");
                        api.set_approval_for_all_clob().await?;
                        println!("   ✅ Approval submitted. Waiting 3s for chain confirmation...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                        match api.check_balance_allowance(token_id).await {
                            Ok((_balance_decimal2, allowance_decimal2)) => {
                                let allowance2_f64 = f64::try_from(allowance_decimal2 / Decimal::from(CLOB_UNITS_SCALE)).unwrap_or(0.0);
                                println!("   🔍 Allowance after approval: {:.6} shares", allowance2_f64);
                                if allowance2_f64 < shares_to_sell {
                                    println!(
                                        "   ⚠️  Allowance still low after approval submit; proceeding with sell attempt to let SDK/CLOB retry."
                                    );
                                }
                            }
                            Err(e) => {
                                println!(
                                    "   ⚠️  Approval submitted but allowance re-check failed: {}. Proceeding with sell attempt.",
                                    e
                                );
                            }
                        }
                    }
                }
            } else {
                println!("   ✅ Balance and allowance are sufficient");
            }
        }
        Err(e) => {
            eprintln!("   ⚠️  Failed to check balance/allowance: {} - proceeding anyway", e);
            // Use original balance if check fails
            shares_to_sell = shares.unwrap_or(balance);
        }
    }

    println!("\n💰 Attempting to sell {:.6} shares...", shares_to_sell);
    
    // Get current ASK price for selling
    let price_info: Option<TokenPrice> = api.get_best_price(token_id).await?;
    let current_price = price_info
        .and_then(|p| p.ask)
        .map(|p| f64::try_from(p).unwrap_or(0.0))
        .unwrap_or(0.0);

    if current_price == 0.0 {
        anyhow::bail!("Cannot determine current price for selling. No ASK price available.");
    }

    println!("   Current ASK price: ${:.6}", current_price);
    println!("   Expected revenue: ${:.6}", current_price * shares_to_sell);

    // Attempt to sell
    match api.place_market_order(token_id, shares_to_sell, "SELL", Some("FAK")).await {
        Ok(response) => {
            println!("\n✅ SELL ORDER PLACED SUCCESSFULLY!");
            println!("   Order ID: {:?}", response.order_id);
            println!("   Status: {}", response.status);
            if let Some(msg) = &response.message {
                println!("   Message: {}", msg);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ SELL ORDER FAILED: {}", e);
            Err(e)
        }
    }
}
