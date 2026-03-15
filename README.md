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

## What is this?

A self-hosted trading companion for [Polymarket](https://polymarket.com). Instead of watching polymarket.com and manually tracking traders, you get:

- **Instant copy-trading** — follow one or many top traders with configurable size, filters, and exit rules
- **Portfolio at a glance** — your wallet's positions, total value, and active trades in one view
- **Real-time activity feed** — every trade from your targets, streamed live, organized by category
- **AI-powered analysis** — ask the Agent about any market, position, or trader; get structured research (sentiment, timing, catalysts, red flags) and actionable guidance
- **Simulation mode** — test strategies with zero risk before going live

Everything runs locally on your machine. Your keys never leave your server.

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

## Quick Start

### 1. Clone

```bash
git clone https://github.com/frogansol/fast-polymarket-copytrading-bot-rust.git
cd fast-polymarket-copytrading-bot-rust
```

### 2. Configure

Create two files in the project root:

**`config.json`** — your Polymarket CLOB credentials:

```jsonc
{
  "polymarket": {
    "clob_api_url": "https://clob.polymarket.com",
    "private_key": "your-private-key",
    "api_key": "your-api-key",
    "api_secret": "your-api-secret",
    "api_passphrase": "your-api-passphrase"
  }
}
```

**`trade.toml`** — who to copy and how:

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

**`.env`** *(optional, for AI Agent)*:

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
|---------|-----------|
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
