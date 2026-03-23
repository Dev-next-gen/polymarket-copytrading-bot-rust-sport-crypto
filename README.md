# Polymarket Copytrading Bot (Rust) - Installation

## Requirements

- Rust 1.70+ ([rustup.rs](https://rustup.rs))
- Polymarket account with USDC on Polygon + CLOB API keys ([docs](https://docs.polymarket.com/developers/CLOB/))
- `trunk` (`cargo install trunk`)
- WASM target (`rustup target add wasm32-unknown-unknown`)

## 1) Clone

```bash
git clone https://github.com/Krypto-Hashers-Community/polymarket-copytrading-bot-rust-sport-crypto.git
cd polymarket-copytrading-bot-rust-sport-crypto
```

## 2) Configure

Create two files in the project root.

`config.json` (Polymarket CLOB credentials):

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

`trade.toml` (copy settings):

```toml
[copy]
target_address   = "0x1979ae6B7E6534dE9c4539D0c205E582cA637C9D"
# target_addresses = ["0x...", "0x..."]
size_multiplier   = 0.01
poll_interval_sec = 0.5

[exit]
take_profit   = 0
stop_loss     = 0
trailing_stop = 0

[filter]
buy_amount_limit_in_usd = 0
entry_trade_sec         = 0
trade_sec_from_resolve  = 0
```

Optional `.env` (for AI Agent):

```env
OPENROUTER_API_KEY=sk-or-...
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
```

## 3) Build and Run

```bash
cd frontend && trunk build --release && cd ..
cargo run --release
```

Open [http://localhost:8000](http://localhost:8000).

## 4) Simulation Mode

```bash
cargo run --release -- --simulation
```
