# Code Walkthrough (Function-Level)

This document walks through the codebase by module and function. It focuses on what each function does and the key invariants it enforces.

---

## src/lib.rs

- Exposes crate modules: `backtest`, `cli`, `config`, `core`, `data`, `execution`, `funding`, `indicators`, `integration`, `logging`, `position`, `runtime`, `signals`, `state`, `util`.

---

## src/main.rs

- `main()`
  - Parses CLI, initializes tracing, loads `config.toml`.
  - Resolves runtime parameters (base URL, interval, once/paper/disable_funding, state path) from CLI overrides or config.
  - Handles subcommands:
    - `backtest`: loads bars JSON, runs backtest engine, writes metrics/trades/equity.
    - `download`: fetches Hyperliquid candles and writes backtest bars JSON.
  - For live/paper:
    - Builds `PriceFetcher`, optional `FundingFetcher`, and `ExecutionEngine`.
    - Restores persisted state if `state_path` is provided.
    - Runs once or starts the loop with Ctrl-C shutdown.
- `parse_rfc3339(value: &str) -> Result<DateTime<Utc>>`
  - Parses RFC3339 timestamps for the download command and normalizes to UTC.

---

## src/cli.rs

Command-line schema (Clap derive). No functions.

- `Cli` struct: top-level flags and subcommands.
- `Command` enum: `Backtest`, `Download`.
- `BacktestArgs`: bars path and optional output dir.
- `DownloadArgs`: start/end RFC3339 and output path.

---

## src/config/mod.rs

### Enums and parsing
- `Symbol::all() -> &'static [Symbol]`: returns `[ETH, BTC]` in fixed order.
- `impl FromStr for Symbol::from_str`: parses `ETH-PERP`/`BTC-PERP`.
- `impl FromStr for PriceField::from_str`: parses `MID|MARK|CLOSE`.
- `impl FromStr for SigmaFloorMode::from_str`: parses `CONST|QUANTILE|EWMA_MIX`.
- `impl FromStr for CapitalMode::from_str`: parses `FIXED_NOTIONAL|EQUITY_RATIO`.
- `impl FromStr for FundingMode::from_str`: parses `FILTER|THRESHOLD|SIZE`.
- `impl FromStr for OrderType::from_str`: parses `MARKET|LIMIT`.
- `impl FromStr for LogFormat::from_str`: parses `JSON|TEXT`.
- `impl FromStr for RoundingMode::from_str`: parses `FLOOR|CEIL|ROUND`.

### Defaults
- `StrategyConfig::default()`: default z-score parameters.
- `SigmaFloorConfig::default()`: default sigma floor parameters.
- `PositionConfig::default()`: default sizing parameters.
- `FundingConfig::default()`: default funding filters and thresholds.
- `RiskConfig::default()`: default max hold/cooldown/confirm bars.
- `DataConfig::default()`: default price field (MID).
- `ExecutionConfig::default()`: default order type and slippage.
- `RuntimeConfig::default()`: default base URL and loop settings.
- `LoggingConfig::default()`: default log level and format.
- `BacktestConfig::default()`: default backtest cost flags.
- `InstrumentConstraints::default()`: default min qty/notional/step/tick and rounding.
- `Config::default()`: composes all defaults and sets constraints for ETH/BTC.

### Validation and loading
- `Config::validate()`
  - Checks z-score bounds, sigma floor params, position parameters, funding limits,
    runtime base URL and interval, and instrument constraints.
- `Config::from_toml_path(path)`
  - Loads overrides from TOML, applies to defaults, and validates.
- `Config::apply_overrides(overrides)`
  - Applies overrides section-by-section, including runtime and auth.
- `ConfigOverrides::from_toml_path(path)`
  - Parses TOML into overrides (no env overlays).
- `load_config(path)`
  - Convenience wrapper for `load_config_with_cli(path, None)`.
- `load_config_with_cli(path, cli_overrides)`
  - Applies defaults, TOML overrides, then CLI overrides, and validates.
- `get_default_config()`
  - Returns a clone of the lazy baseline config.

---

## src/backtest/mod.rs

### Core types
- `TradeExitReason::from(ExitReason)`
  - Converts live exit reasons into backtest exit reasons.
- `Metrics::default()`
  - Zero-initialized metrics.

### Engine
- `BacktestEngine::new(config)`
  - Creates a backtest engine with the provided config.
- `BacktestEngine::run(bars)`
  - Runs the signal pipeline over bars, simulates trades,
    and produces trades, equity curve, logs, and metrics.

### PnL and metrics
- `compute_trade_pnl(input, config)`
  - Calculates PnL per trade (price moves, fees, slippage, optional funding).
- `compute_metrics(trades, equity_curve, risk_free_rate)`
  - Computes win rate, profit factor, stop-loss rate, annualized return,
    sharpe ratio, and max drawdown.

### IO helpers
- `export_metrics_json(path, metrics)`
  - Writes metrics as pretty JSON.
- `load_backtest_bars(path)`
  - Reads bars JSON into `Vec<BacktestBar>`.
- `export_trades_csv(path, trades)`
  - Writes trades to CSV.
- `export_equity_csv(path, equity)`
  - Writes equity curve to CSV.

### Utilities
- `run_sensitivity(configs, bars)`
  - Runs multiple configs against the same bars.
- `breakdown_monthly(trades)`
  - Aggregates PnL by year/month.
- `verify_reproducibility(config, bars)`
  - Runs twice and compares trade counts for determinism.

---

## src/backtest/download.rs

- `HyperliquidDownloader::new(base_url)`
  - Creates a downloader with default Hyperliquid HTTP client.
- `HyperliquidDownloader::with_client(base_url, http)`
  - Creates a downloader with injected HTTP client (testable).
- `HyperliquidDownloader::fetch_backtest_bars(start, end)`
  - Downloads ETH/BTC candles and returns aligned backtest bars.
- `HyperliquidDownloader::map_prices(bars)`
  - Converts candle bars into a timestamp->price map (uses close, then mid/mark).

---

## src/data/mod.rs

### Price bars
- `PriceBar::new(symbol, timestamp, mid, mark, close)`
  - Constructs a price bar.
- `PriceBar::effective_price(preferred)`
  - Chooses price by field preference with fallbacks.
- `PriceBar::validate()`
  - Ensures present prices are positive.

### Price sources and HTTP
- `PriceSource::fetch_bar(symbol, timestamp)`
  - Trait: fetch a single bar.
- `PriceSource::fetch_history(symbol, start, end)`
  - Trait: fetch a history range.
- `HttpClient::post(url, body)`
  - Trait: POST JSON to an API.
- `ReqwestHttpClient::new()`
  - Builds HTTP client.
- `ReqwestHttpClient::default()`
  - Default constructor.
- `ReqwestHttpClient::post(url, body)`
  - POSTs JSON and returns response body/status.

### Hyperliquid price source
- `HyperliquidPriceSource::new(base_url)`
  - Uses reqwest client and a fixed rate limiter.
- `HyperliquidPriceSource::with_client(base_url, http)`
  - Injects custom HTTP client.
- `HyperliquidPriceSource::with_client_and_rate_limiter(base_url, http, limiter)`
  - Fully injected constructor.
- `HyperliquidPriceSource::endpoint_url()`
  - Returns `/info` endpoint.
- `HyperliquidPriceSource::symbol_string(symbol)`
  - Maps symbols to Hyperliquid coins (`ETH`, `BTC`).
- `HyperliquidPriceSource::normalize_range(start, end)`
  - Aligns to 15m closes and validates ordering.
- `HyperliquidPriceSource::parse_decimal(value)`
  - Parses numeric JSON into `Decimal`.
- `HyperliquidPriceSource::parse_candles(symbol, body)`
  - Parses candleSnapshot response into `PriceBar` list.
- `HyperliquidPriceSource::fetch_bar(symbol, timestamp)`
  - Fetches a single aligned bar via `fetch_history`.
- `HyperliquidPriceSource::fetch_history(symbol, start, end)`
  - Calls Hyperliquid candleSnapshot and returns bars in range.

### Price fetcher
- `PriceFetcher::new(source, price_field)`
  - Stores a source and preferred price field.
- `PriceFetcher::fetch_pair_prices(timestamp)`
  - Fetches ETH/BTC bars, validates alignment, and builds a `PriceSnapshot`.

### MockPriceSource (testing)
- `MockPriceSource::insert_bar(bar)`
- `MockPriceSource::insert_history(symbol, bars)`
- `MockPriceSource::insert_error(symbol, timestamp, error)`
- `MockPriceSource::insert_history_error(symbol, error)`
- `MockPriceSource::read_bar(symbol, timestamp)`
- `MockPriceSource::read_history(symbol, start, end)`
- `MockPriceSource::fetch_bar(...)` (trait impl)
- `MockPriceSource::fetch_history(...)` (trait impl)

### Price history buffers
- `PriceHistory::new(capacity)`
- `PriceHistory::push(bar)`
- `PriceHistory::get(offset)`
- `PriceHistory::len()`
- `PriceHistory::is_empty()`
- `PriceHistory::is_warmed_up(required)`
- `PriceHistory::iter()`
- `PriceHistory::to_vec()`

### PriceHistorySet
- `PriceHistorySet::new(z_capacity, vol_capacity, sigma_capacity)`
  - Validates window sizes and allocates ETH/BTC histories.
- `PriceHistorySet::push_pair(eth_bar, btc_bar)`
  - Validates symbol/time ordering and inserts both bars.
- `PriceHistorySet::is_warmed_up(window)`
  - Reports readiness for z-score/vol/sigma windows.
- `PriceHistorySet::window(symbol, window)`
  - Returns iterator over requested history window.

### Utilities
- `align_to_bar_close(timestamp)`
  - Aligns to 15-minute bar close boundary.

---

## src/execution/mod.rs

### Orders and sides
- `OrderSide::close_for_qty(qty)`
  - Returns Buy/Sell side to close a position with signed qty.

### HTTP and nonces
- `OrderHttpClient::post(url, body)`
  - Trait: POST JSON for order endpoints.
- `NonceProvider::next_nonce()`
  - Trait: monotonic nonce source.
- `TimeNonceProvider::new()`
  - Creates nonce provider using wall-clock millis.
- `TimeNonceProvider::next_nonce()`
  - Monotonic timestamp-based nonce.

### Retry and error handling
- `ExecutionError::is_transient()`
  - Indicates if error is retryable.
- `RetryConfig::fast()`
  - Minimal retry policy (2 attempts, 1ms base).
- `RetryConfig::default()`
  - Uses `fast()`.

### Order executors
- `OrderExecutor::submit(order)`
  - Trait: submit open order.
- `OrderExecutor::close(order)`
  - Trait: submit close order.

### Reqwest order client
- `ReqwestOrderClient::new(api_key)`
  - Creates client with optional bearer auth.
- `ReqwestOrderClient::post(url, body)`
  - Sends JSON and returns HTTP response.

### Hyperliquid signing
- `HyperliquidSigner::new(private_key)`
  - Wraps EIP-712 signing key.
- `HyperliquidSigner::signer()`
  - Builds `PrivateKeySigner` from hex key.
- `HyperliquidSigner::connection_id(action, nonce, vault)`
  - Derives connection ID hash from action, nonce, and optional vault address.
- `HyperliquidSigner::sign(action, nonce, is_testnet, vault)`
  - Produces EIP-712 signature fields for request.

### Hyperliquid response helpers
- `HyperliquidOrderType::ioc_limit()`
  - Builds an IOC limit order type payload.
- `HyperliquidExecResponse::filled_qty(self)`
  - Extracts filled quantity from exchange response.
- `HyperliquidExecOrderResponseData::filled_qty(self)`
  - Extracts filled quantity from order status response.

### Hyperliquid live executor
- `LiveOrderExecutor::new(base_url)`
  - Creates executor with default HTTP client and rate limiter.
- `LiveOrderExecutor::with_api_key(base_url, api_key)`
  - Uses API key for HTTP auth.
- `LiveOrderExecutor::with_client(base_url, client)`
  - Injects HTTP client.
- `LiveOrderExecutor::with_client_and_rate_limiter(base_url, client, limiter)`
  - Full injection constructor.
- `LiveOrderExecutor::with_private_key(base_url, private_key)`
  - Configures signer from private key.
- `LiveOrderExecutor::with_signer(signer)`
  - Injects signer.
- `LiveOrderExecutor::with_nonce_provider(nonce_provider)`
  - Injects nonce provider.
- `LiveOrderExecutor::with_vault_address(vault_address)`
  - Sets optional vault.
- `LiveOrderExecutor::with_testnet(is_testnet)`
  - Overrides testnet flag.
- `LiveOrderExecutor::infer_testnet(base_url)`
  - Heuristic testnet detection from base URL.
- `LiveOrderExecutor::exchange_url()`
  - Returns `/exchange` endpoint URL.
- `LiveOrderExecutor::info_url()`
  - Returns `/info` endpoint URL.
- `LiveOrderExecutor::load_asset_specs()`
  - Loads asset IDs and size decimals via `/info`.
- `LiveOrderExecutor::asset_spec(symbol)`
  - Fetches cached asset spec or loads.
- `LiveOrderExecutor::align_size(qty, decimals)`
  - Rounds size to exchange precision.
- `LiveOrderExecutor::post_order(...)`
  - Signs and POSTs order requests.
- `LiveOrderExecutor::submit(order)`
  - Places a new order; returns filled qty.
- `LiveOrderExecutor::close(order)`
  - Places close order; returns filled qty.

### Execution engine
- `ExecutionEngine::new(executor, retry)`
  - Wraps an executor with retry config.
- `ExecutionEngine::open_pair(eth_order, btc_order)`
  - Submits both legs with rollback on partial fills.
- `ExecutionEngine::close_pair(eth_order, btc_order)`
  - Closes both legs with rollback on partial fills.
- `ExecutionEngine::repair_residual(position)`
  - Attempts to close any residual quantities.
- `ExecutionEngine::retry_submit(order)`
  - Retries open leg using retry policy.
- `ExecutionEngine::retry_close(order)`
  - Retries close leg using retry policy.
- `ExecutionEngine::retry_with(action)`
  - Generic retry wrapper for submit/close.

### Mock and paper executors
- `MockOrderExecutor::push_submit_response(qty)`
- `MockOrderExecutor::push_close_response(qty)`
- `MockOrderExecutor::pop_response()`
- `MockOrderExecutor::submit(order)`
- `MockOrderExecutor::close(order)`
- `PaperOrderExecutor::submit(order)`
- `PaperOrderExecutor::close(order)`

---

## src/funding/mod.rs

### Core types
- `FundingRate::validate()`
  - Ensures interval_hours > 0.

### HTTP and sources
- `FundingHttpClient::post(url, body)`
  - Trait: POST JSON for funding endpoints.
- `ReqwestFundingClient::new()`
  - Creates HTTP client.
- `ReqwestFundingClient::post(url, body)`
  - Sends JSON and returns response.
- `HyperliquidFundingSource::new(base_url)`
  - Default Hyperliquid source with interval=1h.
- `HyperliquidFundingSource::with_client(base_url, http)`
  - Injected HTTP client.
- `HyperliquidFundingSource::endpoint_url()`
  - Returns `/info` endpoint.
- `HyperliquidFundingSource::symbol_string(symbol)`
  - Maps to `ETH`/`BTC`.
- `HyperliquidFundingSource::parse_decimal(value)`
  - Parses JSON numeric to Decimal.
- `HyperliquidFundingSource::parse_snapshot(body, timestamp)`
  - Parses `metaAndAssetCtxs` response into rates.
- `FundingSource::fetch_rate(symbol, timestamp)`
  - Trait: single funding rate.
- `FundingSource::fetch_history(symbol, start, end)`
  - Trait: funding rate history.
- `HyperliquidFundingSource::fetch_rate(...)`
  - Uses latest snapshot and selects requested symbol.
- `HyperliquidFundingSource::fetch_history(...)`
  - Returns latest snapshot at end (no history yet).
- `HyperliquidFundingSource::fetch_snapshot(timestamp)`
  - Calls `/info` and parses rates.

### Funding fetcher and mocks
- `FundingFetcher::new(source)`
  - Wraps a funding source.
- `FundingFetcher::fetch_pair_rates(timestamp)`
  - Fetches ETH/BTC rates and validates interval.
- `MockFundingSource::insert_rate(rate)`
- `MockFundingSource::insert_history(symbol, history)`
- `MockFundingSource::insert_error(symbol, timestamp, error)`
- `MockFundingSource::read_rate(symbol, timestamp)`
- `MockFundingSource::fetch_rate(...)`
- `MockFundingSource::fetch_history(...)`
- `ZeroFundingSource::default()`
- `ZeroFundingSource::new(interval_hours)`
- `ZeroFundingSource::fetch_rate(...)`
- `ZeroFundingSource::fetch_history(...)`

### Funding history + controls
- `FundingHistory::new(capacity)`
- `FundingHistory::push(rate)`
- `FundingHistory::window(symbol)`
- `estimate_funding_cost(...)`
  - Computes worst-case funding cost over max hold horizon.
- `apply_funding_controls(...)`
  - Applies FILTER/THRESHOLD/SIZE rules to adjust entry/capital or skip.

---

## src/indicators/mod.rs

### Rolling window helpers
- `RollingWindow::new(capacity)`
- `RollingWindow::push(value)`
- `RollingWindow::len()`
- `RollingWindow::as_slice()`
- `RollingWindow::to_vec()`
- `RollingWindow::mean()`
- `RollingWindow::std()`
- `RollingWindow::quantile(p)`

### Math utilities
- `relative_price(eth, btc)`
  - Computes ETH/BTC ratio.
- `log_return(current, previous)`
  - Computes log return.
- `ewma_std(values, half_life)`
  - Exponential weighted std dev.

### Sigma floor
- `SigmaFloorCalculator::new(config, bars_per_day)`
  - Initializes sigma floor state.
- `SigmaFloorCalculator::update(sigma_t, r_values)`
  - Returns effective sigma based on mode.

### Z-score
- `ZScoreCalculator::new(n_z, sigma_floor, bars_per_day)`
  - Sets rolling window for z-score.
- `ZScoreCalculator::update(r)`
  - Updates rolling stats and returns snapshot.

### Volatility
- `VolatilityCalculator::new(n_vol)`
  - Initializes vol window.
- `VolatilityCalculator::update(eth, btc)`
  - Updates vol snapshot for both legs.

---

## src/signals/mod.rs

- `EntrySignalDetector::new(config)`
  - Uses entry z-score threshold for signal generation.
- `EntrySignalDetector::update(zscore, status)`
  - Produces entry signal when flat/cooldown and zscore crosses entry.
- `ExitSignalDetector::new(strategy, risk)`
  - Initializes TP/SL/time stop thresholds.
- `ExitSignalDetector::evaluate(zscore, status, position, timestamp)`
  - Produces exit signal based on TP/SL/time stop.

---

## src/core/strategy.rs

- `StrategyEngine::new(config, execution)`
  - Creates signal pipeline and state machine.
- `StrategyEngine::state()`
  - Returns current state machine reference.
- `StrategyEngine::apply_state(state)`
  - Hydrates state machine with persisted state.
- `StrategyEngine::process_bar(bar)`
  - Main step: updates indicators, evaluates signals,
    applies funding controls, sizes positions,
    submits open/close orders, and returns outcome.
- `StrategyEngine::build_outcome(...)`
  - Constructs `StrategyOutcome` and `BarLog` from step data.
- `StrategyEngine::limit_price(side, price)`
  - Applies slippage bps to produce limit price.

---

## src/integration/mod.rs

- `max_drawdown()`
  - Returns hard-coded max drawdown gate.
- `min_sharpe()`
  - Returns hard-coded min Sharpe gate.
- `min_win_rate()`
  - Returns hard-coded min win rate gate.
- `min_profit_factor()`
  - Returns hard-coded min profit factor gate.
- `min_trades()`
  - Returns hard-coded minimum trade count gate.
- `papertrading_gate(metrics)`
  - Checks metrics against gate thresholds.
- `api_connectivity_ok(fetcher, timestamp)`
  - Async check: returns true if price fetch succeeds.
- `deployment_ready(config, api_ok)`
  - Combines config validation + API health gates.

---

## src/runtime/mod.rs

- `StateWriter::save(state)`
  - Trait: async persistence hook.
- `StateStoreWriter::new(store)`
  - Wraps `StateStore` for async writes.
- `StateStoreWriter::save(state)`
  - Saves to SQLite state store.
- `LiveRunner::new(engine, prices, funding)`
  - Creates live runner with engine and data sources.
- `LiveRunner::with_clock(now_fn)`
  - Injects time source (testing).
- `LiveRunner::with_state_writer(writer)`
  - Adds persistence hook.
- `LiveRunner::run_once()`
  - Runs one loop iteration at current time.
- `LiveRunner::run_once_at(timestamp)`
  - Runs one loop iteration at a specific time.
- `LiveRunner::run_loop(interval, shutdown_rx)`
  - Runs periodic loop with shutdown signal.

---

## src/state/mod.rs

- `PositionSnapshot::has_residual()`
  - True if any leg has non-zero qty.
- `PositionSnapshot::is_flat()`
  - True if both legs are zero.
- `PositionSnapshot::holding_hours(now)`
  - Returns holding duration in hours.
- `StrategyState::default()`
  - Default FLAT state.
- `StateMachine::new(risk)`
  - Initializes with risk config.
- `StateMachine::state()`
  - Returns current state.
- `StateMachine::hydrate(state)`
  - Validates and sets state.
- `StateMachine::enter(position, now)`
  - Transitions to IN_POSITION.
- `StateMachine::exit(reason, now)`
  - Transitions out with exit reason.
- `StateMachine::update(now)`
  - Updates cooldown timers and transitions.
- `StateStore::new(path)`
  - Opens SQLite store (creates if needed).
- `StateStore::new_in_memory()`
  - In-memory SQLite store.
- `StateStore::init_schema()`
  - Creates schema if missing.
- `StateStore::save(state)`
  - Persists state.
- `StateStore::load()`
  - Loads state if present.
- `recover_state(state, now)`
  - Inspects state for inconsistencies and produces recovery report.

---

## src/logging/mod.rs

### Bar logging
- `BarLog::to_json_value()`
  - Converts bar log to JSON value.
- `LogFormatter::format_json(bar)`
  - Serializes bar log as JSON string.
- `LogFormatter::format_text(bar)`
  - Produces human-readable line.

### Alerting
- `AlertChannel::send(alert)`
  - Trait: sends alerts.
- `InMemoryAlertChannel::alerts()`
  - Returns captured alerts.
- `InMemoryAlertChannel::send(alert)`
  - Stores alerts in memory.
- `AlertDispatcher::new(channels)`
  - Creates dispatcher.
- `AlertDispatcher::send(alert)`
  - Sends to all channels.
- `AlertHttpClient::post(url, payload)`
  - Trait for webhook clients.
- `RetryPolicy::fast()`
  - Minimal retry policy.
- `RetryPolicy::default()`
  - Uses `fast()`.
- `WebhookChannel::new(url, retry, client)`
  - Creates webhook channel.
- `WebhookChannel::send_with_retry(payload)`
  - Sends with retries and backoff.
- `WebhookChannel::send(alert)`
  - Formats and sends alert.
- `EmailTransport::send(subject, body)`
  - Trait for email sending.
- `NoopEmailTransport::send(...)`
  - No-op implementation.
- `EmailChannel::new(transport, throttle_seconds)`
  - Creates email channel with throttling.
- `EmailChannel::send_inner(alert)`
  - Applies throttling and sends.
- `EmailChannel::send(alert)`
  - Trait impl entrypoint.
- `redact_json_value(value)`
  - Redacts sensitive keys in JSON recursively.

### File logging
- `RotationConfig::default()`
  - Default rotation (max bytes, count).
- `FileLogger::new(path, rotation)`
  - Opens file and sets rotation policy.
- `FileLogger::write_line(line)`
  - Writes and rotates if needed.
- `FileLogger::rotate()`
  - Rotates files.
- `rotated_path(path, index)`
  - Helper for rotation filenames.

---

## src/position/mod.rs

- `risk_parity_weights(vol_eth, vol_btc)`
  - Computes inverse-volatility weights.
- `compute_capital(config, equity)`
  - Computes position capital based on mode.
- `SizeConverter::new(constraints, policy)`
  - Initializes converter with rounding rules.
- `SizeConverter::convert_notional(notional, price)`
  - Converts notional to qty and validates min sizes.
- `SizeConverter::round_qty(qty, mode)`
  - Rounds qty per exchange constraints.

---

## src/util/rate_limiter.rs

- `RateLimiter::wait()`
  - Trait: async wait hook.
- `NoopRateLimiter::wait()`
  - No-op implementation.
- `FixedRateLimiter::new(min_interval)`
  - Creates rate limiter with min delay.
- `FixedRateLimiter::disabled()`
  - Returns a disabled limiter.
- `FixedRateLimiter::wait()`
  - Enforces minimum interval between calls.

---

## src/core/mod.rs

- `TradeDirection::is_eth_long()`
  - True if direction is long ETH / short BTC.
- `TradeDirection::is_btc_long()`
  - True if direction is short ETH / long BTC.
