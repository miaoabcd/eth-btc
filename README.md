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

### Useful flags

- `--base-url`: Hyperliquid API base URL (default: `https://api.hyperliquid.xyz`)
- `--once`: run a single iteration and exit
- `--disable-funding`: ignore funding filters
- `--paper`: use paper executor instead of live

## Environment variables

See `.env.example` for the full list, including:

- `STRATEGY_*` overrides (z-score window, risk, funding, sizing, etc.)
- `HYPERLIQUID_PRIVATE_KEY`, `HYPERLIQUID_VAULT_ADDRESS`
- `RUST_LOG` for logging verbosity

## Notes

- State persistence uses SQLite and is optional (`--state-path`).
- Live trading requires valid Hyperliquid credentials.
- Funding filters rely on current funding rates (no historical funding yet).

## Tests

```bash
cargo test
```

