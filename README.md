# Polymarket copy-trading bot

Copy trades from one or more leaders on Polymarket with a size multiplier, optional take-profit/stop-loss, and a web UI for logs and positions. Real-time WebSocket for all targets (filtered client-side). Built in Rust.

---

## Why this exists

By [FemtoTrader](https://t.me/femtotrader).

Most Polymarket copy bots are slow, brittle, or hard to trust. This one is built for speed and clarity: Rust, async I/O, and the same activity WebSocket the official tooling uses when you follow a single address. You get instant trade flow for one leader, or parallel position checks for several, plus a small dashboard so you can see what’s happening without digging through logs.

Not a black box—you can read the code, change the config, and run it on your own machine.

---

## What you need

- Rust 1.70+
- A Polymarket account (USDC on Polygon) and CLOB API keys
- For the web UI: Trunk and the wasm target (`cargo install trunk` then `rustup target add wasm32-unknown-unknown`)

---

## Quick start

```bash
git clone https://github.com/frogansol/fastest-polymarket-copytrading-bot-sport-crypto.git
cd fastest-polymarket-copytrading-bot-sport-crypto
```

Put **config.json** (CLOB URL, keys, wallet) and **trade.toml** (targets, multiplier, filters) in the project root. Example `trade.toml`:

```toml
[copy]
target_address = "0x1979ae6B7E6534dE9c4539D0c205E582cA637C9D"   # or target_addresses = ["0x...", "0x..."]
size_multiplier = 0.01
poll_interval_sec = 0.5

[exit]
take_profit = 0
stop_loss = 0
trailing_stop = 0

[filter]
buy_amount_limit_in_usd = 0
entry_trade_sec = 0
trade_sec_from_resolve = 0
```

Build and run:

```bash
cargo build --release --bin main_copytrading
cargo run --release --bin main_copytrading
```

Open **http://localhost:8000** for the UI. The API is up either way; the dashboard is optional (build with `cd frontend && trunk build --release` if you want it).

**Simulation only (no real orders):**  
`cargo run --release --bin main_copytrading -- --simulation`

---

## Real-time for one or many targets

The bot subscribes to Polymarket’s **activity WebSocket** (`wss://ws-live-data.polymarket.com`) once and filters client-side by your target address(es). So you get **instant trade flow for every leader**—one or ten. Trades are pushed as they happen; you copy with minimal delay and see them in the UI in real time. A separate loop only refreshes positions for the dashboard; all copying is driven by the stream.

---

## Config reference

**config.json** — CLOB API: `clob_api_url`, `private_key`, `api_key`, `api_secret`, `api_passphrase`. Optional: `proxy_wallet_address`, `signature_type` for proxy/Magic wallets.

**trade.toml** — Copy and server:

| Section   | Notes |
|----------|--------|
| `[copy]` | `target_address` (string) or `target_addresses` (array), `size_multiplier`, `poll_interval_sec`, `revert_trade` |
| `[exit]` | `take_profit`, `stop_loss`, `trailing_stop` (0 = off) |
| `[filter]` | `buy_amount_limit_in_usd`, `entry_trade_sec`, `trade_sec_from_resolve` |
| `[ui]`    | `delta_highlight_sec`, `delta_animation_sec` (for the dashboard) |

Top-level: `clob_host`, `chain_id`, `port`, `simulation`.

---

## Running in production

1. Build the frontend once: `cd frontend && trunk build --release && cd ..`
2. Run only the backend: `cargo run --release --bin main_copytrading`
3. The binary serves both the API and the static UI. Use the URL it prints (e.g. `http://<your-ip>:8000` from another device).

Don’t rely on `trunk serve` for remote access; the backend on port 8000 is the single entry point.

---

## Project layout (copy-trading)

| Path | Role |
|------|------|
| `src/bin/main_copytrading.rs` | Entrypoint, HTTP server, single/multi-target branching |
| `src/activity_stream.rs` | WebSocket client for activity/trades (single target) |
| `src/copy_trading.rs` | trade.toml, filters, copy_trade, exit loop, position diff |
| `src/web_state.rs` | Shared state for UI; `/api/state` JSON + SSE |
| `frontend/` | Leptos UI: dashboard, log, settings, live positions |

---

## References

- [Polymarket CLOB](https://docs.polymarket.com/developers/CLOB/)
- [Polymarket API](https://docs.polymarket.com/api-reference/introduction)
