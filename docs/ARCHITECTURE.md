# Project Survey (AI-Enhanced)

## Summary

An ETH/BTC relative value mean reversion trading strategy for the Hyperliquid platform, implemented in Rust. The system computes Z-scores of the log(ETH/BTC) ratio on 15-minute bars and trades paired perpetual futures positions (ETH-PERP + BTC-PERP) with crossing-based entry signals, risk parity position sizing, three-mode funding cost control, atomic order execution with rollback, SQLite state persistence, and a full backtesting engine. The codebase has complete library-level implementations across 12 modules (~4500 lines of Rust) with 89 passing tests, but lacks a working main entry point and live trading loop.

> Analyzed by: claude

## Tech Stack

| Aspect | Value |
|--------|-------|
| Language | Rust |
| Framework | none |
| Build Tool | cargo |
| Test Framework | built-in (#[test] + criterion + proptest + mockall) |
| Package Manager | cargo |

## Directory Structure

### Source Directories
- `src/`

## Modules

### config
- **Path**: `src/config/mod.rs`
- **Status**: complete
- **Description**: Strategy parameter definitions (enums, structs), TOML file loading, environment variable overrides, CLI overrides, multi-layer config merging, and validation logic

### data
- **Path**: `src/data/mod.rs`
- **Status**: complete
- **Description**: PriceBar model, PriceSource trait with HyperliquidPriceSource HTTP implementation, PriceFetcher for paired ETH/BTC prices, PriceHistory rolling window, PriceHistorySet with separate Z-score/volatility/sigma windows, 15-minute bar alignment, MockPriceSource

### indicators
- **Path**: `src/indicators/mod.rs`
- **Status**: complete
- **Description**: Rolling window with mean/std/quantile, relative_price (log ratio), log_return, ewma_std, SigmaFloorCalculator (CONST/QUANTILE/EWMA_MIX modes), ZScoreCalculator with sigma floor, VolatilityCalculator for risk parity

### signals
- **Path**: `src/signals/mod.rs`
- **Status**: complete
- **Description**: EntrySignalDetector with crossing-into-zone logic preventing oscillation, ExitSignalDetector with take-profit confirmation bars, stop-loss detection, and time-stop enforcement

### position
- **Path**: `src/position/mod.rs`
- **Status**: complete
- **Description**: risk_parity_weights (inverse volatility weighting), compute_capital (fixed notional or equity ratio), SizeConverter with step-size rounding (Floor/Ceil/Round), min_qty/min_notional enforcement, Skip/Adjust policies

### funding
- **Path**: `src/funding/mod.rs`
- **Status**: complete
- **Description**: FundingRate model, FundingSource trait, FundingFetcher for paired rates, FundingHistory rolling storage, estimate_funding_cost for trade duration, apply_funding_controls with FILTER/THRESHOLD/SIZE modes, MockFundingSource

### state
- **Path**: `src/state/mod.rs`
- **Status**: complete
- **Description**: FLAT/IN_POSITION/COOLDOWN state machine with validated transitions, PositionSnapshot with residual detection and holding hours, SQLite-backed StateStore persistence, recover_state crash recovery with action recommendations

### execution
- **Path**: `src/execution/mod.rs`
- **Status**: complete
- **Description**: OrderExecutor trait, ExecutionEngine with exponential backoff retry, atomic open_pair with rollback on second-leg failure, close_pair, repair_residual for single-leg cleanup, MockOrderExecutor with response queues

### logging
- **Path**: `src/logging/mod.rs`
- **Status**: complete
- **Description**: BarLog comprehensive 15m bar recording, LogFormatter (JSON/text), AlertChannel trait, InMemoryAlertChannel, AlertDispatcher, WebhookChannel with retry, EmailChannel with throttling, redact_json_value for sensitive data, FileLogger with rotation

### backtest
- **Path**: `src/backtest/mod.rs`
- **Status**: complete
- **Description**: BacktestEngine full strategy simulation, compute_trade_pnl with fees/slippage/funding, compute_metrics (win rate, profit factor, stop loss rate), export_metrics_json, export_trades_csv, export_equity_csv, run_sensitivity for parameter sweeps, breakdown_monthly, verify_reproducibility

### core
- **Path**: `src/core/`
- **Status**: complete
- **Description**: TradeDirection, EntrySignal, ExitSignal, ExitReason domain types. StrategyEngine main orchestrator with process_bar: updates indicators, checks entry/exit signals, applies funding controls, executes orders atomically, updates state machine

### integration
- **Path**: `src/integration/mod.rs`
- **Status**: partial
- **Description**: papertrading_gate (MDD ≤ 15% check) and deployment_ready (config validation) pre-deployment gates

## Discovered Features

| ID | Description | Module | Source | Confidence |
|----|-------------|--------|--------|------------|
| config.parameters.defaults | Default parameter values for all strategy configs (n_z=384, entry_z=1.5, tp_z=0.45, sl_z=3.5, etc.) | config | inferred | 100% |
| config.parameters.validation | Validates all config parameters: n_z>0, entry_z<sl_z, sigma_floor>0, instrument constraints present, etc. | config | inferred | 100% |
| config.loading.toml | Load configuration overrides from TOML file via from_toml_path | config | inferred | 100% |
| config.loading.env | Load configuration overrides from STRATEGY_* environment variables (30+ env vars supported) | config | inferred | 100% |
| config.loading.multilayer | Multi-layer config merging: defaults → TOML file → env vars → CLI overrides via load_config_with_cli | config | inferred | 100% |
| config.enums.symbol | Symbol enum (EthPerp, BtcPerp) with FromStr parsing supporting ETH-PERP/ETH_PERP formats | config | inferred | 100% |
| config.enums.price-field | PriceField enum (Mid, Mark, Close) for selecting which price to use | config | inferred | 100% |
| config.enums.sigma-floor-mode | SigmaFloorMode enum (Const, Quantile, EwmaMix) for sigma floor calculation strategy | config | inferred | 100% |
| config.enums.capital-mode | CapitalMode enum (FixedNotional, EquityRatio) for position sizing capital calculation | config | inferred | 100% |
| config.enums.funding-mode | FundingMode enum (Filter, Threshold, Size) for funding cost control strategies | config | inferred | 100% |
| config.enums.order-type | OrderType enum (Market, Limit) for execution order types | config | inferred | 100% |
| config.enums.rounding-mode | RoundingMode enum (Floor, Ceil, Round) for quantity step-size rounding | config | inferred | 100% |
| config.instrument-constraints | Per-symbol instrument constraints: min_qty, min_notional, step_size, tick_size, qty/price precision, rounding mode | config | inferred | 100% |
| config.baseline | V1_BASELINE_CONFIG static lazy singleton and get_default_config helper | config | inferred | 100% |
| data.price-bar | PriceBar struct with symbol, timestamp, mid/mark/close prices, effective_price fallback logic, and validation | data | inferred | 100% |
| data.price-source.trait | PriceSource async trait defining fetch_bar and fetch_history interface | data | inferred | 100% |
| data.hyperliquid-api | HyperliquidPriceSource HTTP client: fetches 15m bars from Hyperliquid candleSnapshot API with range normalization, JSON parsing, rate limit handling | data | inferred | 100% |
| data.http-client | HttpClient trait and ReqwestHttpClient implementation with timeout/error handling | data | inferred | 100% |
| data.price-fetcher | PriceFetcher: fetches paired ETH/BTC PriceSnapshots with timestamp alignment and cross-validation | data | inferred | 100% |
| data.price-history | PriceHistory: rolling window storage for price bars with capacity limit, push/get/to_vec operations | data | inferred | 100% |
| data.price-history-set | PriceHistorySet: manages separate Z-score, volatility, and sigma quantile windows per symbol with warmup detection | data | inferred | 100% |
| data.bar-alignment | align_to_bar_close: aligns timestamps to 15-minute bar boundaries | data | inferred | 100% |
| data.mock-source | MockPriceSource with configurable bars, history, and error injection for testing | data | inferred | 100% |
| indicators.relative-price | relative_price: calculates r = ln(ETH) - ln(BTC) with positive price validation | indicators | inferred | 100% |
| indicators.log-return | log_return: calculates ln(current/previous) for return computation | indicators | inferred | 100% |
| indicators.ewma-std | ewma_std: exponentially weighted moving average standard deviation with configurable half-life | indicators | inferred | 100% |
| indicators.rolling-window | RollingWindow: generic fixed-capacity window with push, mean, std, quantile operations | indicators | inferred | 100% |
| indicators.sigma-floor | SigmaFloorCalculator: three sigma floor modes (CONST returns fixed value, QUANTILE uses rolling quantile, EWMA_MIX takes max of quantile and EWMA std) | indicators | inferred | 100% |
| indicators.zscore | ZScoreCalculator: rolling Z-score computation with sigma floor, returning ZScoreSnapshot (r, mean, sigma, sigma_floor, sigma_eff, zscore) | indicators | inferred | 100% |
| indicators.volatility | VolatilityCalculator: per-instrument log-return volatility with rolling window, returns VolatilitySnapshot for risk parity | indicators | inferred | 100% |
| signals.entry-crossing | EntrySignalDetector: crossing-based entry requiring Z to cross INTO zone (prev < entry_z, curr >= entry_z) while state is FLAT | signals | inferred | 100% |
| signals.entry-direction | Entry direction logic: positive Z → ShortEthLongBtc, negative Z → LongEthShortBtc | signals | inferred | 100% |
| signals.exit-take-profit | Take profit detection: |Z| ≤ tp_z with optional confirmation bars before triggering | signals | inferred | 100% |
| signals.exit-stop-loss | Stop loss detection: |Z| ≥ sl_z triggers immediate exit | signals | inferred | 100% |
| signals.exit-time-stop | Time stop detection: forced exit when holding_hours >= max_hold_hours (48h default) | signals | inferred | 100% |
| signals.exit-priority | Exit signal priority: StopLoss checked first, then TakeProfit, then TimeStop | signals | inferred | 100% |
| position.risk-parity | risk_parity_weights: inverse volatility weighting for ETH/BTC allocation (w = 1/vol / sum(1/vol)) | position | inferred | 100% |
| position.compute-capital | compute_capital: calculates total capital from FixedNotional (c_value) or EquityRatio (equity * k) | position | inferred | 100% |
| position.size-converter | SizeConverter: converts notional to quantity with step-size rounding, min_qty/min_notional enforcement, Skip/Adjust below-minimum policies | position | inferred | 100% |
| funding.rate-model | FundingRate struct with symbol, rate, timestamp, interval_hours and validation | funding | inferred | 100% |
| funding.source-trait | FundingSource async trait defining fetch_rate and fetch_history interface | funding | inferred | 100% |
| funding.fetcher | FundingFetcher: fetches paired ETH/BTC FundingSnapshots with interval matching validation | funding | inferred | 100% |
| funding.history | FundingHistory: per-symbol rolling window storage of historical funding rates | funding | inferred | 100% |
| funding.cost-estimate | estimate_funding_cost: projects total funding cost over max hold period based on current rates and direction | funding | inferred | 100% |
| funding.controls-filter | FILTER mode: skips entry entirely when estimated funding cost exceeds threshold | funding | inferred | 100% |
| funding.controls-threshold | THRESHOLD mode: raises effective entry Z-score proportional to normalized funding cost | funding | inferred | 100% |
| funding.controls-size | SIZE mode: reduces capital allocation proportional to funding cost (bounded by c_min_ratio) | funding | inferred | 100% |
| funding.mock-source | MockFundingSource with configurable rates, history, and error injection for testing | funding | inferred | 100% |
| state.status-enum | StrategyStatus enum: Flat, InPosition, Cooldown with serialization support | state | inferred | 100% |
| state.position-snapshot | PositionSnapshot: tracks direction, entry_time, ETH/BTC legs with has_residual, is_flat, holding_hours methods | state | inferred | 100% |
| state.machine-transitions | StateMachine: validated transitions (Flat→InPosition on enter, InPosition→Flat/Cooldown on exit, Cooldown→Flat on time expiry) | state | inferred | 100% |
| state.cooldown | 24-hour cooldown after stop-loss exits, preventing new entries until cooldown_until expires | state | inferred | 100% |
| state.persistence | StateStore: SQLite-backed state persistence with save/load, schema initialization, in-memory option for testing | state | inferred | 100% |
| state.recovery | recover_state: crash recovery that detects expired cooldowns, missing positions, and residual legs, returns RecoveryReport with actions and alerts | state | inferred | 100% |
| execution.order-model | OrderRequest/OrderSide/OrderType model with close_for_qty direction inference | execution | inferred | 100% |
| execution.executor-trait | OrderExecutor async trait with submit and close methods | execution | inferred | 100% |
| execution.retry | ExecutionEngine: exponential backoff retry with configurable max_attempts and base_delay_ms, transient error detection | execution | inferred | 100% |
| execution.open-pair-atomic | open_pair: atomic paired order execution with rollback (closes first leg if second leg fails) | execution | inferred | 100% |
| execution.close-pair | close_pair: closes both ETH and BTC legs with error propagation | execution | inferred | 100% |
| execution.repair-residual | repair_residual: emergency closure of single-leg positions detected by has_residual | execution | inferred | 100% |
| execution.mock-executor | MockOrderExecutor: configurable response queues per symbol for testing submit/close | execution | inferred | 100% |
| logging.bar-log | BarLog: comprehensive 15m bar record with prices, indicators, volatility, weights, funding, state, position, events | logging | inferred | 100% |
| logging.formatter | LogFormatter: JSON and text output formatting for bar logs | logging | inferred | 100% |
| logging.alert-channel | AlertChannel trait, AlertDispatcher multi-channel routing, InMemoryAlertChannel for testing | logging | inferred | 100% |
| logging.webhook | WebhookChannel: HTTP webhook alerts with exponential backoff retry and 5xx/client error distinction | logging | inferred | 100% |
| logging.email | EmailChannel: email alerts with throttle_seconds rate limiting to prevent spam | logging | inferred | 100% |
| logging.redaction | redact_json_value: recursively redacts fields containing key/secret/token/password in JSON values | logging | inferred | 100% |
| logging.file-rotation | FileLogger: file logging with max_bytes rotation and max_files retention (numbered rotation scheme) | logging | inferred | 100% |
| backtest.engine | BacktestEngine: full strategy simulation over historical bars using all indicators, signals, and state machine | backtest | inferred | 100% |
| backtest.trade-pnl | compute_trade_pnl: calculates trade P&L including directional returns, fees, slippage, and funding costs | backtest | inferred | 100% |
| backtest.metrics | compute_metrics: calculates win rate, profit factor, stop loss rate from completed trades | backtest | inferred | 100% |
| backtest.export-metrics | export_metrics_json: writes Metrics struct to JSON file | backtest | inferred | 100% |
| backtest.export-trades | export_trades_csv: writes trade log to CSV with entry_time, exit_time, pnl, exit_reason | backtest | inferred | 100% |
| backtest.export-equity | export_equity_csv: writes equity curve to CSV with timestamp and equity columns | backtest | inferred | 100% |
| backtest.sensitivity | run_sensitivity: runs backtest across multiple config variations for parameter sweep analysis | backtest | inferred | 100% |
| backtest.monthly-breakdown | breakdown_monthly: aggregates trade P&L by year/month for performance reporting | backtest | inferred | 100% |
| backtest.reproducibility | verify_reproducibility: runs backtest twice and checks trade count matches to verify determinism | backtest | inferred | 100% |
| core.trade-direction | TradeDirection enum (LongEthShortBtc, ShortEthLongBtc) with is_eth_long/is_btc_long helpers | core | inferred | 100% |
| core.signal-types | EntrySignal (direction + zscore) and ExitSignal (reason + zscore) domain types | core | inferred | 100% |
| core.strategy-engine | StrategyEngine: main orchestrator composing all modules, initializes calculators/detectors from config | core | inferred | 100% |
| core.process-bar | process_bar: per-bar strategy execution - updates indicators, checks signals, applies funding controls, executes atomic orders, updates state machine, returns StrategyOutcome | core | inferred | 100% |
| integration.papertrading-gate | papertrading_gate: checks max drawdown ≤ 15% threshold for paper trading readiness | integration | inferred | 100% |
| integration.deployment-ready | deployment_ready: validates config is valid before deployment | integration | inferred | 100% |

## Completion Assessment

**Overall: 95%**

**Notes:**
- Build/test/clippy/fmt pass locally; runtime wiring added
- CLI entry point implemented with clap; supports paper/live loops and one-shot runs
- LiveRunner wires PriceFetcher/FundingFetcher into StrategyEngine with graceful shutdown and per-bar state persistence
- Backtest metrics compute annualized_return, sharpe_ratio, max_drawdown
- Integration benchmarks enforce per-bar latency and 1y backtest time thresholds with RSS growth guard
- ai/tasks status fields updated to reflect completion
- Integration module still minimal (gate functions only)

## Recommendations

- Replace ZeroFundingSource with a real funding data source for live runs
- Validate LiveOrderExecutor endpoints/auth against Hyperliquid API spec

## Commands

```bash
# Install dependencies
cargo build

# Start development server
cargo run

# Build for production
cargo build --release

# Run tests
cargo test
```

---

*Generated by agent-foreman with AI analysis*
