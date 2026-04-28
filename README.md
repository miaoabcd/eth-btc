# ETH/BTC Mean Reversion Strategy (Hyperliquid)

This repository implements a production-grade ETH/BTC relative value mean reversion strategy for Hyperliquid. It includes configuration management, data ingestion, funding controls, live/paper execution, state persistence, and a full backtesting engine.

## Modules

- **config**: Typed configuration structs, TOML loading, CLI overrides, validation, and baseline defaults.
- **cli**: Command-line flags for live runner and backtest subcommand.
- **data**: Price ingestion via Hyperliquid `/info` candleSnapshot; price field selection (MID/MARK/CLOSE).
- **funding**: Funding-rate fetcher using Hyperliquid `/info` metaAndAssetCtxs; funding filters/thresholds.
- **execution**: Live and paper order executors, retry logic, and order rollback handling.
- **core**: Strategy engine, position sizing, and orchestration across signals, risk, and execution.
- **signals**: Entry/exit signals and decision logic.
- **indicators**: Z-score and sigma floor calculations (CONST/QUANTILE/EWMA mix).
- **integration**: Glue logic across data, funding, execution, and strategy state.
- **runtime**: Live loop runner with optional state persistence.
- **state**: SQLite-backed persistence and crash recovery helpers.
- **logging**: Structured logging and alert hooks.
- **backtest**: Backtest engine with metrics and export helpers.

## Configuration

Configuration is loaded in this order:

1. Built-in defaults
2. TOML file (via `--config`)
3. CLI overrides

Where to put what:

- **config.toml**: full configuration (strategy + runtime + auth).
- **CLI flags**: optional runtime overrides (`--base-url`, `--state-path`, `--interval-secs`, `--once`, `--paper`, `--disable-funding`).
- **Environment variables**: only for process environment (for example `RUST_LOG`), not config overrides.

Key behaviors:

- `position.c_mode = "EQUITY_RATIO"` uses account equity from Hyperliquid `marginSummary.totalRawUsd` in live mode.
- `execution.leverage` is optional. If set, `updateLeverage` is sent before open orders.
- `funding.modes = ["THRESHOLD"]` is now enforced in entry gating: effective entry threshold becomes `entry_z + k * normalized_funding_cost`.
- `funding.funding_cost_threshold` is denominated in estimated quote-currency cost for the configured position size, not bps.
- `[cost_gate]` can compute cost-aware entry diagnostics in shadow mode and, when `enforce = true`, block entries whose estimated net edge is below `min_net_edge_bps`.
- `[stale_cross]` is an optional guarded recovery path for missed crossing signals after a stop-loss cooldown releases; it only fires inside a short recovery window when z-score is still in the entry band and is reverting.
- `runtime.once = true` runs one cycle and exits (useful for cron scheduling).
- `execution.order_type = "POST_ONLY"` enables passive maker-style entry orders. If both legs rest successfully, the strategy enters a local `PendingEntry` state and waits for the next reconciliation cycle to confirm the actual fill.
- `POST_ONLY` is currently entry-only. Exits still use marketable orders so take-profit / stop-loss logic is not left resting on the book.
- `execution.post_only_ttl_secs` is a bot-side pending-entry timeout, not an exchange-native order TTL. When it expires, the runner cancels the resting maker orders on the next cycle.

Statistics log:

- `[logging].stats_path` writes one record per 15m bar (r/mu/sigma/sigma_eff/zscore, weights, notional, funding fields, cost-gate fields, state, `unrealized_pnl`).
- `[logging].trade_path` writes per-entry/per-exit records (`realized_pnl`, `cumulative_realized_pnl`, `fee`, `exchange_closed_pnl`, `pnl_source`).
- In live mode, trade PnL is reconciled from Hyperliquid fills by order id when available: `realized_pnl = closedPnl - fee`, matching the net fill-history/exported trade-history basis. If fills cannot be fetched or matched, the record falls back to `MODEL_ESTIMATE`.
- If `[logging].price_db_path` points to `.sqlite`, fetched bars are persisted to SQLite (`price_bars`) and can be reused by backtest.
- For maker entry diagnostics, stats records now distinguish "no signal" from "signal blocked" cases via `entry_block_reason`, and `trade_path` records `EntrySubmitted` before a passive order becomes a live position.

Quick queries (JSON format examples):

```bash
# Entries only
jq -c 'select(.event == "Entry")' trades.log

# Exits with reason
jq -c 'select(.event | type == "object" and has("Exit")) | {timestamp, event, direction, eth_price, btc_price}' trades.log

# Last 20 trade records
tail -n 20 trades.log | jq -c '.'
```

Trade-history attribution:

```bash
cargo run --bin eth_btc_strategy -- analyze-trades \
  --trade-history data/trade_history.csv \
  --stats-log data/logs/stats.log \
  --since 2026-03-08T00:00:00Z
```

This reconstructs flat-to-flat cycles and reports paired vs single-leg PnL, fees, net/gross edge bps, direction splits, and optional stats-log candidate replay results.

See `config.toml.example` for all available settings.

## Usage

### Build

```bash
cargo build --release
```

### Backtest

```bash
cargo run --release -- backtest \
  --bars ./data/bars.json \
  --output-dir ./out
```

If `--output-dir` is omitted, metrics are printed to stdout.

### Download Hyperliquid 15m bars

```bash
cargo run --release -- download \
  --start 2024-01-01T00:00:00Z \
  --end 2024-01-02T00:00:00Z \
  --output ./data/hyperliquid_bars.json
```

Output:
- `.json` suffix: writes a JSON array of 15m bars.
- `.sqlite` suffix: writes directly into SQLite table `price_bars` (usable by backtest `--db`).

### Paper trading (no live orders)

```bash
cargo run --release -- \
  --config ./config.toml \
  --state-path ./state.sqlite \
  --paper \
  --interval-secs 900
```

### Live trading (Hyperliquid)

Provide a private key and (optional) vault address via config or CLI.

```bash
cargo run --release -- \
  --config ./config.toml \
  --state-path ./state.sqlite \
  --interval-secs 900
```

You can also pass the credentials via flags:

```bash
cargo run --release -- \
  --private-key 0xYOUR_PRIVATE_KEY \
  --vault-address 0xVAULT
```

Credential precedence in live mode:

1. `--private-key` (or legacy `--api-key`)
2. `auth.private_key` in TOML config

### Execution modes

`[execution].order_type` supports three modes:

- `MARKET`: submits IOC-style marketable limit orders using `slippage_bps`.
- `LIMIT`: submits standard GTC limit orders at the model price.
- `POST_ONLY`: submits ALO/passive entry orders using `post_only_bps` and `post_only_ttl_secs`.

Recommended starting point for maker entries:

```toml
[execution]
order_type = "POST_ONLY"
slippage_bps = 5
post_only_bps = 2
post_only_ttl_secs = 840
```

Notes:

- `post_only_bps = 2` means the order is quoted 2 bps inside the passive side of the book.
- `post_only_ttl_secs = 840` means the bot will cancel unfilled resting maker orders after 14 minutes on the next strategy cycle.
- If both legs are accepted as resting orders, the strategy moves to `PendingEntry`, waits for exchange reconciliation before logging a real `Entry`, and actively cancels the outstanding orders when the local timeout is reached.
- Exit orders ignore `POST_ONLY` and still execute as marketable orders.

### Cost-Aware Entry Diagnostics

Use shadow mode first so the bot records edge/cost estimates without changing trading behavior:

```toml
[cost_gate]
enabled = true
enforce = false
min_net_edge_bps = 1.0
entry_fee_bps = 3.4
exit_fee_bps = 4.4
slippage_bps = 1.0
spread_bps = 0.5
long_eth_short_btc_extra_bps = 0.0
short_eth_long_btc_extra_bps = 2.0
```

When enabled, stats logs include `expected_edge_bps`, `estimated_cost_bps`, `estimated_net_edge_bps`, `cost_gate_required_net_edge_bps`, and `cost_gate_pass`. Set `enforce = true` only after reviewing the shadow distribution.

### Order test (single IOC order)

Use `order-test` to validate signing + exchange connectivity without running the full strategy.

```bash
cargo run --release -- order-test \
  --symbol ETH-PERP \
  --side BUY \
  --qty 0.01 \
  --limit-price 1000
```

Add `--reduce-only` to send a reduce-only close, or `--dry-run` to print the order payload without submitting.

### Cancel a resting order

Use `cancel-order` to explicitly cancel a known Hyperliquid order id:

```bash
cargo run --release -- cancel-order \
  --symbol ETH-PERP \
  --oid 375051900617
```

Add `--dry-run` to print the request intent without submitting it.

### Useful flags

- `--base-url`: Hyperliquid API base URL (default: `https://api.hyperliquid.xyz`)
- `--once`: run a single iteration and exit
- `--disable-funding`: ignore funding filters
- `--paper`: use paper executor instead of live
- `--config` + `logging.price_db_path`: enable live price persistence to SQLite
- Sizing: `position.min_size_policy = "SKIP" | "ADJUST"` (default SKIP; ADJUST bumps to exchange minimums)

### Recommended Deployment (cron + --once)

1. Build release binary:
   ```bash
   cargo build --release
   ```
2. Ensure scripts are executable:
   ```bash
   chmod +x scripts/run_live_once.sh
   ```
3. Install cron schedule:
   ```bash
   crontab /home/noone/work/eth-btc/scripts/eth-btc-live.cron
   ```
4. Verify installed cron:
   ```bash
   crontab -l
   ```
5. Runtime logs and lock:
   - Script log: `data/logs/cron-run.log`
   - Stats log: path from `logging.stats_path`
   - Trade log: path from `logging.trade_path`
   - Lock file: `data/locks/trading_job.lock`

`scripts/run_live_once.sh` details:

- Adds `START_DELAY_SECS` (default `10`) before execution.
- Uses `flock -n` to avoid overlapping runs.
- Treats lock contention as "skipped" and exits `0`.
- Supports overrides via env (`BIN_PATH`, `CONFIG_PATH`, `LOCK_PATH`, `LOG_PATH`, `START_DELAY_SECS`, `RUST_LOG`).

If you still prefer systemd long-running mode, run without `--once` and keep `runtime.once = false`.

## Environment variables

Config is TOML-first. Runtime environment variables are optional for process behavior, for example:

- `RUST_LOG=info`
- script-only overrides for `run_live_once.sh` (`START_DELAY_SECS`, `LOCK_PATH`, `LOG_PATH`, etc.)

## Notes

- State persistence uses SQLite and is optional (`--state-path`).
- Live trading requires valid Hyperliquid credentials.
- Funding controls rely on current funding rates (no historical funding series in live loop).
- Funding `THRESHOLD` mode is active in strategy entry gating: `effective_entry_z = entry_z + funding_threshold_k * normalized_funding_cost`.
- Set `logging.price_db_path` to persist fetched candles into SQLite for later analysis.
- Trade log `realized_pnl` uses Hyperliquid fill history in live mode when available; fallback model exits deduct estimated funding cost when funding data is available.
- Residual-leg auto-repair: if only one leg remains, the runner attempts `repair_residual` and logs the event.
- Passive entry workflow: with `POST_ONLY`, an order can be accepted by the exchange without an immediate fill. The strategy persists this as `PendingEntry`, confirms it on the next balance/position sync if it fills, or explicitly cancels the outstanding resting orders before returning to `Flat`.

## Tests

```bash
cargo test
```
