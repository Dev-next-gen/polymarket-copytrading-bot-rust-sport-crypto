## PolyMarket Copy-trading bot (backend + Leptos UI)

Rust clone of the [Polymarket copy-trading bot](https://github.com/dev-protocol/polymarket-copytrading-bot-sport): follow one or more leader addresses, copy their trades with a size multiplier, optional take-profit/stop-loss/trailing exit, and a **Leptos** web UI for logs, dashboard, settings, and live positions.

### Prerequisites

- **Rust** 1.70+
- **Trunk** (for building the Leptos frontend): `cargo install trunk`
- **wasm32 target**: `rustup target add wasm32-unknown-unknown`
- Polymarket account (USDC on Polygon) and CLOB API credentials in `config.json`

### Install

1. **Clone the repo**

   ```bash
   git clone https://github.com/frogansol/fastest-polymarket-copytrading-bot-sport-crypto.git
   cd fastest-polymarket-copytrading-bot-sport-crypto
   ```

2. **Install Rust** (if needed): [rustup.rs](https://rustup.rs)

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **Install Trunk and wasm target** (for the Leptos UI)

   ```bash
   cargo install trunk
   rustup target add wasm32-unknown-unknown
   ```

4. **Add config files** in the project root: `config.json` (CLOB API keys and wallet) and `trade.toml` (copy-trading options). See [Config](#config) for the format.

5. **(Optional) Build the frontend**

   ```bash
   cd frontend && trunk build --release && cd ..
   ```

Then run the bot: see [Build and run](#build-and-run).

### Config

- **config.json** – Same as other bots: `clob_api_url`, `private_key`, `api_key`, `api_secret`, `api_passphrase`, optional `proxy_wallet_address`, `signature_type`.
- **trade.toml** – Copy-trading config (same shape as the TypeScript project):

```toml
clob_host = "https://clob.polymarket.com"
chain_id = 137
port = 8000
simulation = false

[copy]
target_address = ["0x1979ae6B7E6534dE9c4539D0c205E582cA637C9D"]
revert_trade = false
size_multiplier = 0.01
poll_interval_sec = 10

[exit]
take_profit = 0
stop_loss = 0
trailing_stop = 0

[filter]
buy_amount_limit_in_usd = 0
entry_trade_sec = 0
trade_sec_from_resolve = 0

[ui]
delta_highlight_sec = 10
delta_animation_sec = 2
```

### Build and run

**1. Backend (copy-trading + API server)**

```bash
cargo build --release --bin main_copytrading
# Or run directly:
cargo run --release --bin main_copytrading -- -c config.json -t trade.toml
```

**2. Leptos frontend (optional; for the dashboard UI)**

```bash
cd frontend
trunk build --release
cd ..
```

This writes the UI into `frontend/dist/`. The backend serves it at `http://localhost:8000` (or the `port` in `trade.toml`). If `frontend/dist` is missing, the server still runs and `/api/state` works; only the static UI will 404 until you build the frontend.

**3. Run copy-trading + UI**

```bash
# After building the frontend once:
cargo run --release --bin main_copytrading
# Open http://localhost:8000
```

- **Simulation (no real orders):** `cargo run --release --bin main_copytrading -- --simulation`
- **Custom UI dir:** `--ui-dir /path/to/dist`

### Project layout (copy-trading)

- `src/bin/main_copytrading.rs` – Entry point: load config, spawn HTTP server, poll leaders and copy trades.
- `src/copy_trading.rs` – Config (trade.toml), filters, copy_trade, exit loop, position diff.
- `src/web_state.rs` – Shared state for the UI (logs, status, positions); served as JSON at `/api/state`.
- `frontend/` – Leptos (CSR) UI: Dashboard, Log, Settings, Live positions; fetches `/api/state` every 3s.

---

## References

- [Polymarket CLOB](https://docs.polymarket.com/developers/CLOB/)
- [Polymarket API Reference](https://docs.polymarket.com/api-reference/introduction)
