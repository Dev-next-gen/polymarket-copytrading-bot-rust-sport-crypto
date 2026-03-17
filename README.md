<p align="center">
  <h1 align="center">Polymarket CopyTrading Bot</h1>
  <p align="center">
    <strong>Copy top traders. Track portfolios. Get AI analysis. All from one fast interface.</strong>
  </p>
  <p align="center">
    Built in Rust &middot; Real-time WebSocket &middot; Web Dashboard &middot; AI Agent
  </p>
</p>

---

## What inspired this bot?
Lots of traders have bad thoughts that copy trading is  totally impossible? Is it true? Actually Not !!! There is nothing impossible... There are things causing copy trading's failure... Its also been mentioning in solana & EVM chains' copy tradings... copy trading top traders' activities has been lots of traders' goal.. but only very small percent of them got success in copy trading.. just 2 years ago, we got jito and other lots of services in solana to land transacton much faster. so copy trading in solana had lots of progress and success.. but also at that time, lots of pople tried copy trading in solana, and failed. why? cuz the main reason is stragety, just copy trading cant get you a success... there must be a strategy. Recently, we got getting good powerful resource for it.. Thats AI... also lots of bots are only relying on one target to copy trade at once.. its not a good way.. but we can get good ai analysis from copy trading, analyzing, comparing results from multiple targets 

I've been coming up with this idea for long-term, and started started development 5 months ago, spending weeks of time for testing, refining filters, timing logic, and position sizes. Running simulations on historical data and testing with a small live balance showed promising results: steady, consistent gains without chasing extreme volatility.

Btw, all existing copy trading bots are not profitable in lots of cases, the main reason is their bot performance is very bad. its not well-optimized... Lots of polymarket copy trading bots are written in typescript in github, sometimes, python...  they are not fast, not providing multi-threading for faster speed.. with the benefit of rust, we can resolve this issue... 

Rust compiled to WebAssembly (WASM)... Extremely fast, low memory.. Actually Ui performance is not matter... cuz once target wallets and private key are well defined, copy trading activities will be done via backend very fast

This repo is fully written with rust for front end and backend... rust backend is the best choice for buy/sell executoin speed, websocket price feeds, etc.. Rather than using only script based or backend based bot, there are lots of cons and disadvantages to get insight into copy trading... in this case, with UI, can get lots of information real-time using websocket, etc. We can get portoflio, top traders, real-time trading avitigites, ai anlysis and hint whether to copy trade or not.. even for multi-targets' copy trading, can choose colors per every target and to emphasize thier address as colored text in logs, so that we can easily who they are in lots of logs easily...

If we target at multiple targets, sometimes we might face their trade markets are same.. in this case, i thought of AI analysis to choose one target to copy trade by giving target's speicfic category's winning rate from AI analysis, rather than copy trading all targets activies in case they are trading same market some times...

Also sometimes, top traders' strategy are not always right. market keep changes. so their strategy might be wrong sometimes.  so bot's copy trading strategy should be keep changing. So i thought of giving information for top traders' pnl and volume real-time, and ai anlysis into their trading activities...  Also with introduced ai agent, they can getta see and anslysze every top traders' activiteis to decide if they are fit to copy trade hour by hour, day by day, month  by month... in ai agent, can select varoius famous ai services like openrouter.ai, claude.ai, chatgpt, etc to get better lots of response.....  

This isn't about quick wins or chasing hype. It's an AI-powered system that spreads risk across multiple markets — sports, crypto, politics, macroeconomics — and lets smarter decision-making do the heavy lifting. The goal is simple: consistent, steady growth over the long term.

---

## What is this?

A self-hosted trading companion for [Polymarket](https://polymarket.com). Instead of watching polymarket.com and manually tracking traders, you get:

- **Instant copy-trading** — follow one or many top traders with configurable size, filters, and exit rules
- **Portfolio at a glance** — your wallet's positions, total value, and active trades in one view
- **Real-time activity feed** — every trade from your targets, streamed live, organized by category
- **AI-powered analysis** — ask the Agent about any market, position, or trader; get structured research (sentiment, timing, catalysts, red flags) and actionable guidance
- **Simulation mode** — test strategies with zero risk before going live

Works across politics, sports, crypto, and macro—tune `trade.toml` filters to match the markets you copy. Everything runs locally on your machine. Your keys never leave your server.

---

**By [FemtoTrader](https://t.me/femtotrader)** — questions, feedback, and contributions welcome.

---

## Features

| | Feature | What it does |
|---|---------|-------------|
| :bar_chart: | **Dashboard** | Live status, activity stream, copy targets — single page for "what's happening right now" |
| :robot: | **Agent** | AI chat (OpenRouter / OpenAI / Claude). Research any market or position — get trade hints, risk analysis, and confidence signals |
| :scroll: | **Logs** | Full trade and event log, streamed in real time via SSE |
| :trophy: | **Top Traders** | Follow the best wallets on Polymarket. See their activity the moment it happens |
| :briefcase: | **Portfolio** | Your wallet's active trades, total value, and per-target positions side by side |
| :gear: | **Settings** | All config at a glance — targets, multiplier, exit rules, simulation toggle |

---

## Screenshots

| **Dashboard** | **Agent** |
|:---:|:---:|
| <img src="https://i.ibb.co/Rkmq13bj/mm-1.png" alt="mm 1" border="0"> | <img src="https://i.ibb.co/HfXx5kwR/mm-2.png" alt="mm 2" border="0"> |

---

## Quick Start

### 1. Clone

```bash
git clone https://github.com/Krypto-Hashers-Community/polymarket-copytrading-bot-rust-sport-crypto.git
cd polymarket-copytrading-bot-rust-sport-crypto
```

### 2. Configure

Create two files in the project root:

**`config.json`** — your Polymarket CLOB credentials:

```jsonc
{
  "polymarket": {
    "gamma_api_url": "https://gamma-api.polymarket.com",
    "clob_api_url": "https://clob.polymarket.com",
    "api_key": "your-api-key",
    "api_secret": "your-api-secret",
    "api_passphrase": "your-api-passphrase",
    "private_key": "your-private-key",
    "proxy_wallet_address": null,
    "signature_type": 0
  }
}
```

**`trade.toml`** — who to copy and how. Use any leader's wallet address from [Polymarket](https://polymarket.com) as `target_address` or in `target_addresses`:

```toml
[copy]
target_address = "0x1979ae6B7E6534dE9c4539D0c205E582cA637C9D"
# or target_addresses = ["0x...", "0x..."]
size_multiplier = 0.01
poll_interval_sec = 0.5

[exit]
take_profit = 0      # 0 = off
stop_loss = 0
trailing_stop = 0

[filter]
buy_amount_limit_in_usd = 0
entry_trade_sec = 0
trade_sec_from_resolve = 0
```

**`.env`** *(not required, for AI Agent)*:

```env
OPENROUTER_API_KEY=sk-or-...
# or OPENAI_API_KEY=sk-...
# or ANTHROPIC_API_KEY=sk-ant-...
```

### 3. Build & Run

```bash
# Build the frontend (once)
cd frontend && trunk build --release && cd ..

# Run
cargo run --release --bin main_copytrading
```

Open **http://localhost:8000** — that's it. Dashboard, agent, logs, portfolio, everything is there.

### 4. Simulation mode (no real orders)

```bash
cargo run --release --bin main_copytrading -- --simulation
```

Perfect for testing your setup, exploring the UI, and evaluating traders before risking capital.

---

## Requirements

| Requirement | Details |
|------------|---------|
| **Rust** | 1.70+ |
| **Polymarket account** | USDC on Polygon + CLOB API keys ([docs](https://docs.polymarket.com/developers/CLOB/)) |
| **Frontend tooling** | `cargo install trunk` and `rustup target add wasm32-unknown-unknown` |

---

## How it works

The bot subscribes to Polymarket's **activity WebSocket** (`wss://ws-live-data.polymarket.com`) and filters by your target addresses client-side. Trades are pushed the instant they happen — you copy with minimal delay and see them in the UI in real time.

A separate loop refreshes positions for the portfolio view. All copy-trading is driven by the live stream, not polling.

```
Activity WebSocket ──▶ Filter by targets ──▶ Copy trade ──▶ Dashboard + Logs
                                                │
                                         Exit rules (TP/SL/trailing)
```

---

## AI Agent

The Agent tab turns your dashboard into a research terminal. Pick a provider (OpenRouter, OpenAI, or Claude) from the dropdown — it uses whichever API keys you set in `.env`.

The agent follows the **Monitor → Analyze** method (inspired by [Mahoraga](https://mahoraga.dev/)):

- You ask a question about a market, a position, or a trader
- The LLM researches sentiment, timing, catalysts, and red flags
- You get back a structured note: **Signal → Research → Context → Confidence → Guidance**

No execution — research and guidance only. You decide when to act.

---

## Config Reference

**`config.json`** — CLOB API credentials and wallet:

| Field | Required | Notes |
|-------|----------|-------|
| `clob_api_url` | Yes | `https://clob.polymarket.com` |
| `private_key` | Yes | Polygon wallet private key |
| `api_key` / `api_secret` / `api_passphrase` | Yes | From Polymarket CLOB dashboard |
| `proxy_wallet_address` | No | For proxy/Magic wallets |
| `signature_type` | No | `0` = EOA, `1` = Proxy, `2` = GnosisSafe |

**`trade.toml`** — copy-trading behavior:

| Section | Key fields |
|---------|------------|
| `[copy]` | `target_address` or `target_addresses`, `size_multiplier`, `poll_interval_sec`, `revert_trade` |
| `[exit]` | `take_profit`, `stop_loss`, `trailing_stop` (0 = off) |
| `[filter]` | `buy_amount_limit_in_usd`, `entry_trade_sec`, `trade_sec_from_resolve` |
| `[ui]` | `delta_highlight_sec`, `delta_animation_sec` |

Top-level: `clob_host`, `chain_id`, `port`, `simulation`.

---

## Production Deployment

```bash
# 1. Build frontend
cd frontend && trunk build --release && cd ..

# 2. Run (serves both API and UI on one port)
cargo run --release --bin main_copytrading
```

Access from any device on your network at `http://<your-server-ip>:8000`. The binary is the single entry point — no separate frontend server needed.

---

## Project Layout

| Path | Role |
|------|------|
| `src/bin/main_copytrading.rs` | Entrypoint, HTTP server, agent endpoints |
| `src/activity_stream.rs` | WebSocket client for real-time trades |
| `src/copy_trading.rs` | Config, filters, copy logic, exit loop, position diff |
| `src/api.rs` | Polymarket CLOB/Data API client |
| `src/clob_sdk.rs` | FFI bindings to the CLOB SDK `.so` |
| `src/web_state.rs` | Shared state for UI, `/api/state` JSON + SSE |
| `frontend/` | Leptos (Rust → WASM): dashboard, agent, logs, portfolio, settings |

---

## References

- [Polymarket CLOB Documentation](https://docs.polymarket.com/developers/CLOB/)
- [Polymarket API Reference](https://docs.polymarket.com/api-reference/introduction)
- [OpenRouter](https://openrouter.ai/) — multi-model AI gateway
- [Mahoraga](https://mahoraga.dev/) — Monitor → Analyze method

---

<p align="center"><sub>Built for traders who want speed, transparency, and an edge.</sub></p>
