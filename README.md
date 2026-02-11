# ETH/BTC Mean Reversion Strategy (Hyperliquid)

This repository implements a production-grade ETH/BTC relative value mean reversion strategy for Hyperliquid. It includes configuration management, data ingestion, funding controls, live/paper execution, state persistence, and a full backtesting engine.

## Modules

- **config**: Typed configuration structs, TOML loading, env/CLI overrides, validation, and baseline defaults.
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
3. Environment variables (`STRATEGY_*`)
4. CLI overrides

Where to put what:

- **config.toml**: full configuration (strategy + runtime + auth).
- **CLI flags**: optional runtime overrides (`--base-url`, `--state-path`, `--interval-secs`, `--once`, `--paper`, `--disable-funding`).
- **Environment variables**: not used for config overrides (except `RUST_LOG` for logging).

Statistics log:

- 默认 `[logging].stats_path = "stats.log"`，每根 15m 记录 r/μ/σ/σ_eff/Z、权重、名义、funding 等字段（JSON/TEXT 任选，默认 JSON）。
- 可选 `[logging].trade_path` 记录每次开/平仓事件。
- 将 `[logging].price_db_path` 设为 `.sqlite` 文件时，实时行情会追加到 SQLite（可供回测读取）。

Quick queries (JSON format examples):

```bash
# Entries only
jq -c 'select(.event == "Entry")' trades.log

# Exits with reason
jq -c 'select(.event | startswith("Exit")) | {timestamp, event, direction, eth_price, btc_price}' trades.log

# Last 20 trade records
tail -n 20 trades.log | jq -c '.'
```

See `config.toml.example` and `.env.example` for all available settings.

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

Output：
- 以 `.json` 结尾 → 写 JSON 数组（15m bar）。
- 以 `.sqlite` 结尾 → 直接写入 SQLite（表 `price_bars`，可用 backtest `--db` 读取）。

### Paper trading (no live orders)

```bash
cargo run --release -- \
  --config ./config.toml \
  --state-path ./state.sqlite \
  --paper \
  --interval-secs 900
```

### Live trading (Hyperliquid)

Provide a private key and (optional) vault address via env or CLI. The first non-empty key wins.

```bash
export HYPERLIQUID_PRIVATE_KEY=0xYOUR_PRIVATE_KEY
# optional
export HYPERLIQUID_VAULT_ADDRESS=0xVAULT

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
- `--config` + `logging.price_db_path`: 开启实时价格持久化（SQLite）
- 位置管理：`position.min_size_policy = "SKIP" | "ADJUST"`（默认 SKIP；ADJUST 会自动抬到交易所最小下单）

## Environment variables

See `.env.example` for the full list, including:

- `STRATEGY_*` overrides (z-score window, risk, funding, sizing, etc.)
- `HYPERLIQUID_PRIVATE_KEY`, `HYPERLIQUID_VAULT_ADDRESS`
- `RUST_LOG` for logging verbosity

## Notes

- State persistence uses SQLite and is optional (`--state-path`).
- Live trading requires valid Hyperliquid credentials.
- Funding filters rely on current funding rates (no historical funding yet).
- Set `logging.price_db_path` to persist fetched candles into SQLite for later analysis.
- 单腿残留自动修复：检测到只剩一条腿时会立即尝试 `repair_residual` 并记录事件。

## Tests

```bash
cargo test
```
