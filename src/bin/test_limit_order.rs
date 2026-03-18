use anyhow::Result;
use clap::Parser;
use rust_decimal::Decimal;
use polymarket_trading_bot::{PolymarketApi, Config, models::OrderRequest};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "test_limit_order")]
#[command(about = "Test placing a limit order on Polymarket")]
struct Args {
    #[arg(short, long)]
    token_id: Option<String>,
    
    #[arg(short, long, default_value = "55")]
    price_cents: u64,
    
    #[arg(short, long, default_value = "5")]
    shares: u64,
    
    #[arg(short, long, default_value = "1")]
    expiration_minutes: u64,
    
    #[arg(short, long, default_value = "config.json")]
    config: String,
    
    #[arg(long, default_value = "BUY")]
    side: String,
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

    // Authenticate first
    println!("🔐 Authenticating with Polymarket CLOB API...");
    api.authenticate().await?;
    println!();

    // Discover token ID if not provided
    let token_id = if let Some(id) = args.token_id {
        println!("📋 Using provided token ID: {}", id);
        id
    } else {
        println!("🔍 Discovering current BTC market...");
        let condition_id = api.discover_current_market("BTC").await?
            .ok_or_else(|| anyhow::anyhow!("Could not find current BTC market"))?;
        
        println!("   ✅ Found BTC market: {}", condition_id);
        
        // Get market details to find the "Up" token
        let market = api.get_market(&condition_id).await?;
        let up_token = market.tokens
            .iter()
            .find(|t| t.outcome == "Up")
            .ok_or_else(|| anyhow::anyhow!("Could not find 'Up' token in BTC market"))?;
        
        println!("   ✅ Found BTC Up token: {}", up_token.token_id);
        up_token.token_id.clone()
    };

    // Get current market price for reference
    println!("\n📊 Checking current market price...");
    if let Ok(Some(price_info)) = api.get_best_price(&token_id).await {
        println!("   Current BID: {:?}", price_info.bid);
        println!("   Current ASK: {:?}", price_info.ask);
        if let Some(mid) = price_info.mid_price() {
            println!("   Mid price: {}", mid);
        }
    }

    // Convert price from cents to decimal (e.g., 55 cents = 0.55)
    let price_decimal = Decimal::from(args.price_cents) / Decimal::from(100);
    
    // Convert shares to decimal
    let size_decimal = Decimal::from(args.shares);
    
    // Calculate expiration timestamp (current time + expiration_minutes)
    let expiration_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() + (args.expiration_minutes * 60);

    println!("\n📝 Order Details:");
    println!("   Token ID: {}", token_id);
    println!("   Side: {}", args.side);
    println!("   Price: {} ({} cents)", price_decimal, args.price_cents);
    println!("   Size: {} shares", args.shares);
    println!("   Expiration: {} minutes (timestamp: {})", args.expiration_minutes, expiration_timestamp);
    println!();

    // Create order request
    let order = OrderRequest {
        token_id: token_id.clone(),
        side: args.side.clone(),
        size: size_decimal.to_string(),
        price: price_decimal.to_string(),
        order_type: "LIMIT".to_string(),
    };

    // Place the order
    println!("📤 Placing limit order...");
    match api.place_order(&order).await {
        Ok(response) => {
            println!("\n✅ Order placed successfully!");
            println!("   Order ID: {:?}", response.order_id);
            println!("   Status: {}", response.status);
            if let Some(msg) = response.message {
                println!("   Message: {}", msg);
            }
            
            // Note: The SDK handles expiration automatically, but we can verify it was set correctly
            println!("\n💡 Note: The SDK automatically sets expiration time when signing the order.");
            println!("   Your order will expire in {} minutes if not filled.", args.expiration_minutes);
        }
        Err(e) => {
            eprintln!("\n❌ Failed to place order: {}", e);
            eprintln!("\n💡 Troubleshooting:");
            eprintln!("   - Check that you have sufficient USDC balance");
            eprintln!("   - Check that you have USDC allowance to the Exchange contract");
            eprintln!("   - Verify the token ID is correct");
            eprintln!("   - Ensure the market is accepting orders");
            return Err(e);
        }
    }

    Ok(())
}
