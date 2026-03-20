use anyhow::{Context, Result};
use clap::Parser;
use polymarket_trading_bot::{Config, PolymarketApi};

#[derive(Parser, Debug)]
#[command(name = "test_buy")]
#[command(about = "Quick test: place a small BUY market order on Polymarket")]
struct Args {
    /// Market slug (e.g. "will-btc-go-up-5min-073000-pm-et-mar-20").
    /// Use this OR --token-id.
    #[arg(short, long)]
    slug: Option<String>,

    /// Token ID to buy directly (skips slug lookup).
    #[arg(short, long)]
    token_id: Option<String>,

    /// Which outcome to buy when using --slug (e.g. "Yes", "Up", "Down").
    /// Defaults to the first token listed.
    #[arg(short, long)]
    outcome: Option<String>,

    /// Amount in USD to spend (default $1.00)
    #[arg(short, long, default_value = "1.0")]
    amount: f64,

    /// Path to config.json
    #[arg(short, long, default_value = "config.json")]
    config: String,

    /// Authenticate and show what WOULD be bought, but don't place the order
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();

    if args.slug.is_none() && args.token_id.is_none() {
        eprintln!("Error: provide either --slug or --token-id\n");
        eprintln!("Examples:");
        eprintln!("  cargo run --release --bin test_buy -- --slug will-btc-go-up-5min-073000-pm-et-mar-20 --amount 1");
        eprintln!("  cargo run --release --bin test_buy -- --token-id 8613680357... --amount 0.50");
        std::process::exit(1);
    }

    let config = Config::load(&std::path::PathBuf::from(&args.config))?;

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

    println!("=== Polymarket Test Buy ===\n");

    // 1. Authenticate
    println!("[1/3] Authenticating with CLOB...");
    api.authenticate().await.context("Authentication failed")?;
    println!("  OK\n");

    // 2. Resolve token_id
    let token_id = if let Some(tid) = args.token_id {
        println!("[2/3] Token: {}", tid);
        tid
    } else {
        let slug = args.slug.as_deref().unwrap();
        println!("[2/3] Looking up slug: {}", slug);
        let market = api.get_market_by_slug(slug).await
            .context(format!("Could not find market with slug '{}'", slug))?;

        println!("  Market:   {}", market.question);
        println!("  Active:   {}  Closed: {}", market.active, market.closed);

        // The Gamma API returns token info as JSON-encoded strings, not a
        // structured array.  Parse clobTokenIds + outcomes + outcomePrices.
        let token_ids: Vec<String> = market.clob_token_ids.as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .or_else(|| {
                market.tokens.as_ref().map(|ts| ts.iter().map(|t| t.token_id.clone()).collect())
            })
            .context("Market has no token IDs (clobTokenIds or tokens)")?;

        let outcomes: Vec<String> = market.outcomes.as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| {
                market.tokens.as_ref()
                    .map(|ts| ts.iter().map(|t| t.outcome.clone()).collect())
                    .unwrap_or_default()
            });

        if token_ids.is_empty() {
            anyhow::bail!("Market has no tokens");
        }

        println!("  Outcomes:");
        for (i, tid) in token_ids.iter().enumerate() {
            let name = outcomes.get(i).map(|s| s.as_str()).unwrap_or("?");
            println!("    [{}] {} — token {}...{}", i, name,
                &tid[..16.min(tid.len())], &tid[tid.len().saturating_sub(6)..]);
        }

        let chosen_idx = if let Some(ref want) = args.outcome {
            let want_lower = want.to_lowercase();
            outcomes.iter().position(|o| o.to_lowercase() == want_lower)
                .with_context(|| format!("Outcome '{}' not found. Available: {:?}", want, outcomes))?
        } else {
            0
        };

        let chosen_tid = &token_ids[chosen_idx];
        let chosen_name = outcomes.get(chosen_idx).map(|s| s.as_str()).unwrap_or("?");

        println!("\n  Buying: {} (token {}...{})",
            chosen_name,
            &chosen_tid[..16.min(chosen_tid.len())],
            &chosen_tid[chosen_tid.len().saturating_sub(6)..]
        );
        // Print the full CLOB token id so it can be copy/pasted into test_sell.
        println!("  Full token id: {}", chosen_tid);
        chosen_tid.clone()
    };

    // 3. Place order
    println!("\n[3/3] BUY ${:.2} USDC  |  FAK  |  token {}", args.amount, &token_id[..20.min(token_id.len())]);

    if args.dry_run {
        println!("\n  ** DRY RUN — order NOT placed **");
        println!("  Remove --dry-run to execute for real.");
        return Ok(());
    }

    match api.place_market_order(&token_id, args.amount, "BUY", Some("FAK")).await {
        Ok(resp) => {
            println!("\n  SUCCESS");
            println!("  Order ID: {}", resp.order_id.as_deref().unwrap_or("n/a"));
            println!("  Status:   {}", resp.status);
            if let Some(msg) = &resp.message {
                println!("  Message:  {}", msg);
            }
        }
        Err(e) => {
            eprintln!("\n  FAILED: {:#}", e);
            std::process::exit(1);
        }
    }

    println!("\nDone.");
    Ok(())

}
