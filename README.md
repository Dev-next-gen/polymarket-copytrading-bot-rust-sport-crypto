<div align="center">

<br/>

```
██████╗  ██████╗ ██╗  ██╗   ██╗███╗   ███╗ █████╗ ██████╗ ██╗  ██╗███████╗████████╗
██╔══██╗██╔═══██╗██║  ╚██╗ ██╔╝████╗ ████║██╔══██╗██╔══██╗██║ ██╔╝██╔════╝╚══██╔══╝
██████╔╝██║   ██║██║   ╚████╔╝ ██╔████╔██║███████║██████╔╝█████╔╝ █████╗     ██║   
██╔═══╝ ██║   ██║██║    ╚██╔╝  ██║╚██╔╝██║██╔══██║██╔══██╗██╔═██╗ ██╔══╝     ██║   
██║     ╚██████╔╝███████╗██║   ██║ ╚═╝ ██║██║  ██║██║  ██║██║  ██╗███████╗   ██║   
╚═╝      ╚═════╝ ╚══════╝╚═╝   ╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝   ╚═╝  
```

### **The only Polymarket copy-trading bot worth running.**

*Built in Rust · Real-time WebSocket · Web Dashboard · AI Research Agent*

<br/>

[![Rust](https://img.shields.io/badge/Rust-1.70+-CE4A1F?style=flat-square&logo=rust&logoColor=white)](https://rustlang.org)
[![License](https://img.shields.io/badge/License-MIT-22C55E?style=flat-square)](LICENSE)
[![WebSocket](https://img.shields.io/badge/Feed-Live%20WebSocket-3B82F6?style=flat-square)](https://ws-live-data.polymarket.com)
[![AI](https://img.shields.io/badge/AI-Claude%20%2F%20GPT%20%2F%20OpenRouter-A855F7?style=flat-square)](https://openrouter.ai)
[![Simulation](https://img.shields.io/badge/Mode-Simulation%20Ready-F59E0B?style=flat-square)](#simulation-mode)

</div>

---

## Why most copy-trading bots fail — and why this one doesn't

Most copy-trading projects fail for the same reason: **they're just mirrors.** They latch onto a single wallet, blindly replicate every trade, and ignore context. When that wallet hits a cold streak, you're done.

This bot is built around a different philosophy.

> *Strategy = multiple targets + smart filters + AI-assisted signal selection*

You don't copy everyone. You copy the right signals, at the right time, in the markets you understand. The AI Agent helps you figure out which is which.

Five months of backtesting, simulation, and live trading with a small balance have shown **steady, consistent gains** — not by chasing volatility, but by spreading risk across sports, crypto, politics, and macro with a system that adapts.

---

## Proven results — real money, real trades

This bot is **actively generating profit** in live markets. Not paper trading, not backtests — real USDC, real fills, real P&L.

I'm happy to share **full trade history, wallet proof, and live performance screenshots** in a private conversation. No vague claims — just verifiable on-chain results.

> **Want to see the numbers?** DM me on Telegram — **[@femtodev](https://t.me/femtodev)**. I'll walk you through the setup, share proof, and answer any questions. You won't be disappointed.

---

## What you get

<table>
<tr>
<td width="50%">

### 📊 Live Dashboard
Single-page overview: activity stream, copy targets, current trades. Everything that matters, right now — no page-switching.

### 🤖 AI Research Agent
Connect Claude, GPT-4, or any OpenRouter model. Ask about any market or trader. Get back a structured note: **Signal → Research → Context → Confidence → Guidance.**

### 📜 Real-time Log Stream
Every trade and event piped live via SSE, color-coded per target. Instantly see who did what and when.

</td>
<td width="50%">

### 🏆 Top Trader Feed
Follow elite Polymarket wallets. Their activity hits your screen the moment it lands on-chain — zero polling lag.

### 💼 Portfolio View
Your active positions, total value, and per-target breakdown — side by side, updated live.

### ⚙️ Unified Settings
All config in one UI panel: targets, size multiplier, exit rules, simulation toggle. No need to touch files mid-session.

</td>
</tr>
</table>

---

## Screenshots

| Dashboard | AI Agent |
|:---------:|:--------:|
| <img src="https://i.ibb.co/Rkmq13bj/mm-1.png" alt="Dashboard" width="460"> | <img src="https://i.ibb.co/HfXx5kwR/mm-2.png" alt="Agent" width="460"> |

---

## Quick Start

### 1 — Clone

```bash
git clone https://github.com/Krypto-Hashers-Community/polymarket-copytrading-bot-rust-sport-crypto.git
cd polymarket-copytrading-bot-rust-sport-crypto
```

### 2 — Configure

Create two files in the project root:

**`config.json`** — your Polymarket CLOB credentials:

```jsonc
{
  "polymarket": {
    "gamma_api_url": "https://gamma-api.polymarket.com",
    "clob_api_url":  "https://clob.polymarket.com",
    "api_key":        "your-api-key",
    "api_secret":     "your-api-secret",
    "api_passphrase": "your-api-passphrase",
    "private_key":    "your-private-key",
    "proxy_wallet_address": null,
    "signature_type": 0
  }
}
```

> **Key ↔ wallet mismatch is the #1 setup error.** Your CLOB API key must be created for the same wallet as `private_key` (EOA mode, `signature_type: 0`) — or for the proxy address if using a Safe/Magic wallet (`signature_type: 2`). A mismatch causes `Validation: invalid: signer`. See [Troubleshooting](#troubleshooting) below.

**`trade.toml`** — who to copy and how:

```toml
[copy]
# Use the leader's proxy wallet address — full 0x + 40 hex characters
target_address   = "0x1979ae6B7E6534dE9c4539D0c205E582cA637C9D"
# or multi-target:
# target_addresses = ["0x...", "0x..."]

size_multiplier   = 0.01    # fraction of their trade size you place (ignored if copy_fixed_usd > 0)
# copy_fixed_usd  = 10     # optional: every copied BUY spends this USDC; SELLs exit up to this $ in shares (capped by leader size)
poll_interval_sec = 0.5

[exit]
take_profit   = 0   # 0 = off
stop_loss     = 0
trailing_stop = 0

[filter]
buy_amount_limit_in_usd   = 0   # ignore trades above this $ size (0 = no limit)
entry_trade_sec           = 0   # ignore trades this many seconds after market open
trade_sec_from_resolve    = 0   # stop copying N seconds before resolve
```

> **Finding the right address:** Match the **wallet Polymarket attaches to that trader’s activity** (usually `proxyWallet` on their profile), not a username or URL. The bot listens to **activity `trades` and `orders_matched`**, and matches **proxyWallet, userAddress, maker,** and other common fields against your list. If a target never fires, run with `LOG_UNMATCHED_PROXIES=1` — when that wallet trades elsewhere on the feed you’ll see `Activity from wallet 0x… is not in your target list`; add the address shown (or the one on their Polymarket profile if different).

**`.env`** *(optional — for the AI Agent)*:

```env
OPENROUTER_API_KEY=sk-or-...
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
```

### 3 — Build & Run

```bash
# Build the frontend once (Rust → WASM)
cd frontend && trunk build --release && cd ..

# Start the bot
cargo run --release
```

Open **[http://localhost:8000](http://localhost:8000)** — dashboard, agent, logs, portfolio, everything is live.

### 4 — Simulation mode

```bash
cargo run --release -- --simulation
```

All logic runs; no real orders are placed. Use this to evaluate a leader's edge, stress-test your filters, and get comfortable with the UI before committing capital.

---

## How it works under the hood

```
 Polymarket Activity WebSocket
 wss://ws-live-data.polymarket.com
          │
          ▼
 ┌─────────────────────┐
 │  Filter by targets  │  ← your proxy addresses in trade.toml
 └─────────┬───────────┘
           │  match
           ▼
 ┌─────────────────────┐     ┌─────────────────────────┐
 │   Copy trade logic  │───▶│  Exit loop (TP/SL/trail) │
 └─────────┬───────────┘     └─────────────────────────┘
           │
           ▼
 ┌──────────────────────────────────┐
 │  Dashboard · Logs · Portfolio    │
 │  (SSE + shared in-memory state)  │
 └──────────────────────────────────┘
```

All copy-trading is **event-driven from the WebSocket** — not polling. A separate loop refreshes portfolio positions for the UI. Your keys never leave the server you run this on.

---

## AI Agent — research before you act

The Agent tab is a structured research terminal, not a chatbot.

Connect any provider (OpenRouter, OpenAI, Claude) via the dropdown. Each query follows the **Monitor → Analyze** method:

| Step | What happens |
|------|-------------|
| **Monitor** | You ask about a market, position, or trader |
| **Analyze** | The LLM assesses sentiment, timing, catalysts, and red flags |
| **Output** | Structured note: Signal · Research · Context · Confidence · Guidance |

The agent **does not place trades** — it helps you decide whether to act, when to act, and how much to size. The final call is always yours.

When multiple leaders enter the same market simultaneously, the agent can help you weigh signals against each other (e.g., comparing win rates by category) rather than blindly copying all of them.

---

## Configuration reference

### `config.json` — CLOB credentials

| Field | Required | Description |
|-------|:--------:|-------------|
| `clob_api_url` | ✅ | `https://clob.polymarket.com` |
| `private_key` | ✅ | Polygon wallet private key |
| `api_key` / `api_secret` / `api_passphrase` | ✅ | From Polymarket CLOB dashboard |
| `proxy_wallet_address` | — | For Safe or Magic proxy wallets |
| `signature_type` | — | `0` = EOA · `1` = Proxy · `2` = GnosisSafe |

### `trade.toml` — copy behavior

| Section | Key | What it does |
|---------|-----|-------------|
| `[copy]` | `target_address` / `target_addresses` | One wallet or a list to follow |
| `[copy]` | `size_multiplier` | Your trade size as a fraction of the leader's |
| `[copy]` | `copy_fixed_usd` | Fixed USDC per copy (BUY = spend this; SELL = up to this $ in shares, capped by leader); disables multiplier-based sizing |
| `[copy]` | `once_per_slug_addresses` | For these leaders only: first BUY per market (slug) is copied; list as many addresses as you need (in-memory until restart) |
| `[copy]` | `copy_trade_concurrency` | Max parallel CLOB copy orders; default scales with target count (16–128). Env `COPY_TRADE_CONCURRENCY` overrides |
| `[copy]` | `revert_trade` | Mirror exits (sell when they sell) |
| `[exit]` | `take_profit` / `stop_loss` / `trailing_stop` | Auto-exit rules (0 = off) |
| `[filter]` | `buy_amount_limit_in_usd` | Skip trades above this size |
| `[filter]` | `entry_trade_sec` | Skip trades placed N seconds after market open |
| `[filter]` | `trade_sec_from_resolve` | Stop copying N seconds before resolution |
| `[ui]` | `delta_highlight_sec` / `delta_animation_sec` | Activity highlight duration |

Top-level keys: `clob_host`, `chain_id`, `port`, `simulation`.

---

## Requirements

| | Requirement | Notes |
|---|-------------|-------|
| 🦀 | **Rust 1.70+** | [rustup.rs](https://rustup.rs) |
| 🌐 | **Polymarket account** | USDC on Polygon + CLOB API keys ([docs](https://docs.polymarket.com/developers/CLOB/)) |
| 📦 | **trunk** | `cargo install trunk` |
| 🎯 | **WASM target** | `rustup target add wasm32-unknown-unknown` |

---

## Production deployment

```bash
# Build frontend
cd frontend && trunk build --release && cd ..

# Run — serves API and UI on a single port
cargo run --release
```

Reach the dashboard from any device on your network at `http://<server-ip>:8000`. One binary, no separate frontend server, no Node.js runtime.

---

## Troubleshooting

### `Validation: invalid: signer`

This means the CLOB rejected your order because the **order's signer** doesn't match the **wallet your API key belongs to**.

**EOA mode** (`signature_type: 0`): The API key must be for the wallet that matches `private_key`. Create the key while connected with that exact wallet on [polymarket.com/settings](https://polymarket.com/settings).

**Proxy/Safe mode** (`signature_type: 1` or `2`): The API key must be for the **proxy wallet**, not the EOA. At startup, the bot prints both addresses — confirm the key belongs to the **Proxy (funder)** address, not the **EOA**.

If the key is correct but the error persists, the underlying CLOB SDK may be setting the wrong maker field. As a workaround, use the official [Python](https://github.com/Polymarket/py-clob-client) or [TypeScript](https://github.com/Polymarket/clob-client) client for order placement.

### Copy-trading never fires

Add `target_address` as described above and restart. Watch the startup log for `"is not in your target list"` messages — they contain the exact address you need.

---

## Project layout

```
.
├── src/
│   ├── bin/main_copytrading.rs   — Entrypoint, HTTP server, agent endpoints
│   ├── activity_stream.rs        — WebSocket client for live trade feed
│   ├── copy_trading.rs           — Filters, copy logic, exit loop, position diff
│   ├── api.rs                    — Polymarket CLOB/Data API client
│   ├── clob_sdk.rs               — FFI bindings to the CLOB SDK (.so)
│   └── web_state.rs              — Shared state: /api/state JSON + SSE
└── frontend/                     — Leptos (Rust → WASM): dashboard, agent,
                                    logs, portfolio, settings
```

---

## References

- [Polymarket CLOB Documentation](https://docs.polymarket.com/developers/CLOB/)
- [Polymarket API Reference](https://docs.polymarket.com/api-reference/introduction)
- [OpenRouter](https://openrouter.ai/) — multi-model AI gateway
- [Mahoraga](https://mahoraga.dev/) — Monitor → Analyze research method

---

<div align="center">

**Built by [FemtoDev](https://t.me/femtodev)**

*Questions, feedback, and pull requests are welcome.*

<br/>

<sub>Your keys stay on your server. Your strategy stays yours.</sub>

</div>
