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
- `runtime.once = true` runs one cycle and exits (useful for cron scheduling).

Statistics log:

- `[logging].stats_path` writes one record per 15m bar (r/mu/sigma/sigma_eff/zscore, weights, notional, funding fields, state, `unrealized_pnl`).
- `[logging].trade_path` writes per-entry/per-exit records (`realized_pnl`, `cumulative_realized_pnl`).
- If `[logging].price_db_path` points to `.sqlite`, fetched bars are persisted to SQLite (`price_bars`) and can be reused by backtest.

Quick queries (JSON format examples):

```bash
# Entries only
jq -c 'select(.event == "Entry")' trades.log

# Exits with reason
jq -c 'select(.event | type == "object" and has("Exit")) | {timestamp, event, direction, eth_price, btc_price}' trades.log

# Last 20 trade records
tail -n 20 trades.log | jq -c '.'
```

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
- Trade log `realized_pnl` (for new exits) deducts estimated funding cost when funding data is available.
- Residual-leg auto-repair: if only one leg remains, the runner attempts `repair_residual` and logs the event.

## Tests

```bash
cargo test
```
