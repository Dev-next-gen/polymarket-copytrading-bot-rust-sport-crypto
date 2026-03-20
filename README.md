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

Works across politics, sports, crypto, and macro—tune `trade.toml` filters to match the markets you copy. Everything runs locally on your machine. Your keys never leave your server.

---

## What inspired me to build this bot

Copy trading isn’t impossible — but most attempts fail. Even when execution got faster (e.g. Solana, Jito), many people still lost. The main reason is **strategy**: blindly mirroring one wallet isn’t a strategy. You need filters, multiple targets, and a way to decide *which* signals to act on. **AI** is the right tool for that; single-target bots are fragile.

I’ve been developing this idea for a long time and started building about five months ago — testing filters, timing logic, and position sizing. Simulations and a small live balance showed **steady, consistent gains** without chasing volatility.

Most Polymarket copy bots are TypeScript or Python, slow and single-threaded. **Rust** fixes that: fast execution, WebSocket feeds, and real-time processing. This repo is full Rust (backend + frontend compiled to WASM). Once config is set, all copy activity runs in the backend at full speed; the UI is for **insight** — portfolio, top traders, live activity, AI analysis, and colored logs per target so you can tell who did what.

When multiple leaders trade the same market, **AI can help** pick the stronger signal (e.g. by win rate in that category) instead of copying everyone. And because markets and leaders change, the bot streams PnL and activity so you can use the **AI Agent** (OpenRouter, Claude, ChatGPT, etc.) to reassess who’s worth following over time.

The goal isn’t quick wins — it’s **steady growth**, risk spread across markets (sports, crypto, politics, macro), and an AI-assisted system built for the long term.


---

**By [FemtoDev](https://t.me/femtodev)** — questions, feedback, and contributions welcome.

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

**API key and trading wallet:** Polymarket CLOB API keys are tied to one wallet. If you use `proxy_wallet_address` and `signature_type: 2`, create the API key in the Polymarket CLOB dashboard **for that proxy/Safe address**. If you use `signature_type: 0`, the key must be for the wallet that owns `private_key`. A mismatch causes *"Validation: invalid: signer"* when placing orders.

**`trade.toml`** — who to copy and how. You must use the leader's **proxy wallet address** (0x + 40 hex characters), not a profile URL or username:

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

**Finding a leader's address:** The bot matches trades by the **proxy wallet** Polymarket uses when that user trades (the `proxyWallet` in the activity feed). A profile URL like `https://polymarket.com/@0xbetty` or a short username is not valid. Use the full `0x...` address (42 characters total). If copy-trading never triggers: (1) Run the bot and watch the logs when that leader trades — you may see *"Activity from proxy 0x... is not in your target list"*; add that exact address to `target_address` or `target_addresses`. (2) On Polymarket, open the leader's profile and copy their wallet address if shown (ensure it's 0x + 40 hex). Invalid entries in `trade.toml` are skipped and a warning is logged at startup.

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

## Troubleshooting: "Validation: invalid: signer"

This error means the CLOB server rejected the order because the **order’s signer/maker** does not match the **wallet your API key is registered to**.

**If you use a proxy** (`proxy_wallet_address` + `signature_type: 1` or `2`):

1. **Your API key must be created for the proxy wallet**, not for the EOA (the address derived from `private_key`). When you created the key on Polymarket, you must have been connected with the **same** wallet as `proxy_wallet_address` (e.g. your Safe or Magic proxy). Check [polymarket.com/settings](https://polymarket.com/settings) — the address shown there is your proxy; create/derive the API key while that wallet is “active”.
2. At startup the bot prints both **Proxy (funder)** and **EOA (from private_key)**. The API key must be for the **Proxy**, not the EOA. If you created the key while MetaMask (or another EOA) was connected, the key is tied to the EOA and you will get `invalid: signer` when trading via proxy.
3. **If you’re sure the key is for the proxy** and the error persists, the CLOB SDK library (`lib.so`) may be building orders with the wrong maker (EOA instead of proxy). In that case you need a fixed SDK build that sets the order maker to the funder for `signature_type` 2, or use the official [Polymarket TypeScript](https://github.com/Polymarket/clob-client) or [Python](https://github.com/Polymarket/py-clob-client) client to place orders.

**If you trade from an EOA** (`signature_type: 0`, no `proxy_wallet_address`): the API key must be for the same EOA as `private_key`. Create the key while connected with that EOA.

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
