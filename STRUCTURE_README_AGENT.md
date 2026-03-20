# fast-polymarket-copytrading-bot-rust - Structure README (Agent)
This guide is copy/pasteable for another Cursor AI agent so it can understand the repo without needing prior chat context.

---

## 1) Architecture Overview

### What the system does
- Connects to Polymarket’s live activity WebSocket.
- Filters incoming trades by configured **leader proxy wallet addresses** (from `trade.toml`).
- Copies matched leader trades by posting **CLOB market orders** via the Polymarket CLOB API/SDK.
- Tracks copied entries (average price + size) and optionally exits via TP/SL/trailing stop.
- Serves a local Web UI (and SSE updates) showing portfolio + live activity + logs.
- Optionally exposes an “Agent” endpoint that calls LLM providers for analysis.

### Typical data flow
1. `src/bin/main_copytrading.rs`
   - Loads `config.json` (CLOB credentials + signature settings)
   - Loads `trade.toml` (targets + filters + exit rules)
   - Spawns:
     - Axum web server (serves UI + `/api/state` SSE + agent endpoints)
     - Activity stream loop (WebSocket consumer)
     - Optional exit loop (TP/SL/trailing)
     - Positions refresh loop (polls Data API / CLOB user positions)
2. `src/activity_stream.rs`
   - Connects to `wss://ws-live-data.polymarket.com`
   - Subscribes to `activity:trades`
   - Extracts `proxyWallet` (or fallback owner/maker fields) from each payload
   - Converts payload into a normalized `LeaderTrade`
   - Calls `copy_trade(...)` for matching targets
3. `src/copy_trading.rs`
   - Applies filters (`should_copy_trade`)
   - Sizes orders using `size_multiplier` (+ buy-caps / min order constraints)
   - Places orders via `PolymarketApi::place_market_order_fast(...)`
   - On success records entry (for exit loop) and pushes UI updates
4. `src/api.rs`
   - Builds/maintains a CLOB client handle (lazy init)
   - Posts market orders (limit-marketable under the hood)
   - Provides balances/allowances and positions for UI/exit logic
5. `src/web_state.rs`
   - Shared in-memory state for UI logs + positions
   - `/api/state/stream` streams updates via broadcast channel

---

## 2) Repository Layout
```
fast-polymarket-copytrading-bot-rust/
├── src/
│   ├── bin/
│   │   └── main_copytrading.rs          # HTTP server + orchestration
│   ├── activity_stream.rs            # WebSocket consumer (leader trades)
│   ├── copy_trading.rs              # copy sizing + filters + exit loop logic
│   ├── api.rs                       # Data/CLOB API client (order placement, positions)
│   ├── clob_sdk.rs                  # dynamic loading of the CLOB SDK .so (FFI)
│   └── web_state.rs                # shared UI state + push helpers
├── frontend/
│   └── (Leptos build output in `frontend/dist` served by Axum)
├── config.json                      # CLOB credentials + proxy settings
└── trade.toml                       # targets + trade filters + exit rules
```

---

## 3) Entry Points (`src/`)

### Main entrypoint / HTTP server
- `src/bin/main_copytrading.rs`
  - Spawns Axum server:
    - Serves `frontend/dist` via `tower_http::services::ServeDir`
    - `GET /api/state`: returns current UI state snapshot
    - `GET /api/state/stream`: SSE updates (broadcast-based)
    - `GET /api/leaderboard`: forwards to Polymarket data API
    - `GET /api/agent/providers`: returns LLM providers configured by env vars
    - `POST /api/agent/chat`: calls OpenRouter / OpenAI / Anthropic based on provider selection
    - `GET /logs`, `/settings`, `/portfolio`, etc: UI routes (served as static assets)
  - Spawns background tasks:
    - `api.authenticate()` (CLOB client init + prints proxy/funder info)
    - `activity_stream::spawn_activity_stream(...)`
    - `spawn_exit_loop(...)` (only if TP/SL/trailing are enabled)
  - Runs an infinite positions refresh loop:
    - Polls `api.get_positions(user)` for `wallet` + `targets`
    - Calls `web_state::set_positions(...)`
    - On first load pushes `POS | ... | loaded` log entries

### Activity stream (WebSocket consumer)
- `src/activity_stream.rs`
  - Connects to `ACTIVITY_WS_URL = wss://ws-live-data.polymarket.com`
  - Subscribes with:
    - `{"action":"subscribe","subscriptions":[{"topic":"activity","type":"trades"}]}`
  - Extracts the leader identifier:
    - Primary: `payload.proxyWallet`
    - Fallback: `owner`, `maker`, `makerAddress`, `user`
  - Normalizes to lowercase and checks membership in `targets_lower`
  - De-duplicates trades via `seen: HashSet<String>` (uses tx-hash + timestamp)
  - Applies `should_copy_trade(...)` and triggers `copy_trade(...)`
  - UI updates:
    - On success: `LIVE | ... | ok` pushed into shared state
    - On failure: `LIVE | ... | FAILED: <error>` pushed into shared state

---

## 4) Copy Trading Logic

### Config (`trade.toml`)
- `src/copy_trading.rs` defines `CopyTradingConfig`
  - Targets: `copy.target_address` / `copy.target_addresses`
  - Order sizing:
    - `copy.size_multiplier`
    - `filter.buy_amount_limit_in_usd` (BUY cap)
  - Filtering:
    - `filter.entry_trade_sec` (max age after match time)
    - `filter.trade_sec_from_resolve` (time window before resolve)
    - `copy.revert_trade` (whether to copy SELL legs)
  - Exit behavior:
    - `exit.take_profit`, `exit.stop_loss`, `exit.trailing_stop`

### Filter & sizing
- `copy_trading.rs::should_copy_trade`
  - Blocks copying SELL if `revert_trade=false`
  - Optional time-based filtering on match time + resolve time
- `copy_trading.rs::copy_trade`
  - Parses `trade.size` and `trade.price` into decimals (handles scientific notation)
  - Computes order amount:
    - BUY: `amount_usd = size * price * size_multiplier`, converts to shares by dividing by price
    - SELL: `amount_shares = size * size_multiplier`
  - Min constraints:
    - BUY min USD (e.g. ~$0.01)
    - SELL min shares (tiny threshold)
  - Places marketable order with `order_type = Some("FAK")`
  - On BUY success returns `(size_out, price)` so the exit loop can compute PnL.

---

## 5) Transaction Placement Layer (CLOB)

### CLOB client lifecycle
- `src/api.rs::PolymarketApi`
  - `ensure_clob_client()` lazily builds a CLOB client handle once:
    - Uses `clob_sdk::client_create(...)`
    - Passes `private_key`, `api_key/api_secret/api_passphrase`, plus:
      - optional `proxy_wallet_address` as the **funder** (for signature types 1/2)
      - `signature_type` (0=EOA, 1=POLY_PROXY, 2=GNOSIS_SAFE)
  - `authenticate()` calls `ensure_clob_client()` and prints wallet details to help validate signer/funder mismatch.

### Order placement
- `place_market_order_fast(...)`
  - Calls `clob_sdk::post_market_order(handle, token_id, side, amount, is_buy, ot)`
  - Retries only as configured (fast path typically for copy flow)
  - Error classification:
    - If error contains `signer` / `invalid: signer`, it returns a hint about API key wallet mismatch.

### FFI / SDK loading
- `src/clob_sdk.rs`
  - Loads a shared library from:
    - `LIBCOB_SDK_SO` env var, or
    - `./lib/lib.so` fallback
  - Exposes FFI entrypoints used by `api.rs` (client create/destroy, post_market_order, tick_size, balances, etc.)

---

## 6) Exit Loop (TP/SL/Trailing)

- `src/copy_trading.rs::spawn_exit_loop`
  - Runs every `EXIT_INTERVAL_MS = 15_000ms`
  - Uses `entries: HashMap<asset_id, Entry>` updated when copy trades succeed
  - For each entry:
    - Reads current position price/size from Data API positions snapshot
    - Computes PnL % vs `entry_price`
    - Computes trailing % based on `entry.max_price`
  - If TP/SL/trailing conditions trigger:
    - Sells using `api.place_market_order(asset, amount, "SELL", Some("FAK"))`
  - After sell:
    - Updates `entries` sizes and removes fully closed positions.

---

## 7) “What to check when something breaks”

### A) Activity is seen but copy doesn’t happen
- Look for:
  - `Copy skipped | ...` (filters / min size / limit cap)
  - or `LIVE | ... | FAILED: ...` (CLOB rejected order)

### B) `Validation: invalid: signer`
- Means the server rejected the order because the order maker/signer doesn’t match the wallet the API key is registered to.
- For `signature_type: 2` with `proxy_wallet_address`:
  - your API key must be created for the **proxy/Safe address** you configured as `proxy_wallet_address`.

### C) “Activity from proxy ... is not in your target list”
- Your `trade.toml` `copy.target_address(es)` doesn’t match the leader’s **proxy wallet** address.
- Ensure targets are `0x` + 40 hex chars and lowercased; do not use profile URLs/usernames.

---

## 8) How to run (quick)

### Requirements
- `config.json`: Polymarket CLOB L2 credentials + EOA/private key + proxy address + signature_type.
- `trade.toml`: copy targets and risk settings.
- Frontend build output: `frontend/dist` served by Axum.

### Run
- Start the backend/UI:
```bash
cargo run --release --bin main_copytrading
```

- Simulation mode:
```bash
cargo run --release --bin main_copytrading -- --simulation
```

Expected:
- UI available at `http://localhost:8000`
- Activity stream connects to Polymarket WS and logs `Copy | ...` / `LIVE | ... | ok` / `FAILED: ...`

