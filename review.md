# Code Review: ETH/BTC Mean Reversion Strategy

**Review Date**: 2026-01-30
**Reviewer**: Claude Opus 4.5
**Project**: ETH/BTC relative value mean reversion strategy for variational.io
**Language**: Rust (Edition 2024)
**Lines of Code**: ~4,700 (core) + ~2,700 (tests)

---

## Executive Summary

This is a well-structured Rust trading system implementing an ETH/BTC mean reversion strategy. The codebase demonstrates good separation of concerns, comprehensive error handling, and proper use of `rust_decimal` for financial calculations. However, several issues require attention before production deployment.

**Overall Assessment**: **B+** - Production-ready with fixes needed

| Category | Issues Found |
|----------|-------------|
| Critical | 4 |
| High | 8 |
| Medium | 11 |
| Low | 9 |

---

## Critical Issues

### 1. [CRITICAL] Configuration Validation Missing tp_z Bounds Check
**File**: `src/config/mod.rs:453-551`

The `validate()` method checks that `entry_z < sl_z` but does not validate that `tp_z < entry_z`. This could lead to impossible exit conditions where take-profit can never trigger.

```rust
// Current validation (incomplete)
if self.strategy.entry_z >= self.strategy.sl_z {
    return Err(ConfigError::InvalidValue { ... });
}

// Missing validation:
// tp_z should be < entry_z (otherwise TP zone overlaps entry zone)
```

**Impact**: Strategy may never take profit, holding positions indefinitely.

**Recommendation**: Add validation:
```rust
if self.strategy.tp_z >= self.strategy.entry_z {
    return Err(ConfigError::InvalidValue {
        field: "strategy.tp_z",
        message: "must be < entry_z".to_string(),
    });
}
```

---

### 2. [CRITICAL] Hardcoded Funding Interval in StrategyEngine
**File**: `src/core/strategy.rs:119-126`

The funding interval is hardcoded to 8 hours regardless of configuration or actual exchange settings:

```rust
let eth_rate = FundingRate {
    symbol: Symbol::EthPerp,
    rate: funding_eth,
    timestamp: bar.timestamp,
    interval_hours: 8,  // HARDCODED!
};
```

**Impact**: Incorrect funding cost calculations if exchange uses different intervals (e.g., 1h, 4h).

**Recommendation**: Accept `interval_hours` from the `FundingSnapshot` or configuration instead of hardcoding.

---

### 3. [CRITICAL] close_pair Not Truly Atomic
**File**: `src/execution/mod.rs:249-263`

The `close_pair` method closes ETH first, then BTC. If BTC close fails, it returns `PartialFill` but does NOT attempt to rollback/re-open ETH:

```rust
pub async fn close_pair(...) -> Result<(), ExecutionError> {
    let eth_result = self.retry_close(&eth_order).await;
    if eth_result.is_err() {
        return eth_result.map(|_| ());
    }
    let btc_result = self.retry_close(&btc_order).await;
    if let Err(err) = btc_result {
        return Err(ExecutionError::PartialFill(err.to_string()));
        // ETH is closed, BTC still open - UNHEDGED POSITION!
    }
    Ok(())
}
```

**Impact**: Single-leg exposure after partial close failure - violates "never trade single leg" rule.

**Recommendation**: Either:
1. Attempt to re-open ETH position on BTC close failure
2. Alert immediately and retry BTC close aggressively
3. Mark state as requiring manual intervention

---

### 4. [CRITICAL] Backtest Funding Cost Applied Per Bar Instead of Per Trade
**File**: `src/backtest/mod.rs:328-354`

The funding cost is calculated using `max_hold_hours` for every trade, but it's applied using the exit bar's funding rate, not accumulated over the actual holding period:

```rust
let estimate = estimate_funding_cost(
    input.direction,
    input.notional_eth,
    input.notional_btc,
    &eth_rate,
    &btc_rate,
    config.risk.max_hold_hours,  // Uses max, not actual
)
```

**Impact**: Backtest results are inaccurate - trades held for 2 hours are charged the same funding as trades held for 48 hours.

**Recommendation**: Calculate actual holding duration and use actual accumulated funding rates.

---

## High Priority Issues

### 5. [HIGH] BarLog Fields Not Populated
**File**: `src/core/strategy.rs:278-286`

Several `BarLog` fields are always `None` even when the data is available:

```rust
bar_log: BarLog {
    // ...
    w_eth: None,           // Should be weights.w_eth
    w_btc: None,           // Should be weights.w_btc
    notional_eth: None,    // Should be notional_eth
    notional_btc: None,    // Should be notional_btc
    funding_cost_est: None, // Should be estimate.cost_est
    funding_skip: None,     // Should be decision.should_skip
    // ...
}
```

**Impact**: Incomplete logging makes debugging and auditing difficult.

---

### 6. [HIGH] Potential Panic in align_to_bar_close
**File**: `src/data/mod.rs:615-621`

```rust
pub fn align_to_bar_close(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    let seconds = timestamp.timestamp();
    let aligned = seconds - seconds.rem_euclid(900);
    Utc.timestamp_opt(aligned, 0)
        .single()
        .expect("aligned timestamp must be valid")  // PANIC!
}
```

**Impact**: Could panic on edge cases with extreme timestamps.

**Recommendation**: Return `Result<DateTime<Utc>, DataError>` instead.

---

### 7. [HIGH] Inconsistent Error Conversion Patterns
**Files**: Multiple

The codebase uses different error conversion patterns inconsistently:

1. Manual `map_err(|err| SomeError(err.to_string()))`
2. `#[from]` derive macro
3. Explicit `From` implementations

Example inconsistency in `src/runtime/mod.rs`:
```rust
// Both styles used in same file
impl From<DataError> for RunnerError {
    fn from(err: DataError) -> Self {
        RunnerError::Data(err.to_string())
    }
}

// But in run_once_at:
.map_err(|err| RunnerError::Data(err.to_string()))
```

**Impact**: Inconsistent code, harder to maintain.

---

### 8. [HIGH] compute_capital Uses Wrong Default for equity
**File**: `src/core/strategy.rs:104-108`

```rust
let capital = compute_capital(
    &self.config.position,
    self.config.position.c_value.unwrap_or(Decimal::ZERO),  // Wrong!
)
```

For `EquityRatio` mode, this passes `c_value` (which might be None) as equity, not actual account equity.

**Impact**: Position sizing could be completely wrong in EquityRatio mode.

---

### 9. [HIGH] Missing Validation for Quantile Percentile Range
**File**: `src/config/mod.rs:478-483`

```rust
if self.sigma_floor.sigma_floor_quantile_p <= Decimal::ZERO {
    // Only checks > 0, not <= 1
}
```

Quantile percentile should be in range (0, 1], but upper bound is not validated.

**Impact**: Invalid quantile value could cause undefined behavior.

---

### 10. [HIGH] StateStore Not Thread-Safe
**File**: `src/state/mod.rs:176-242`

`StateStore` wraps a `rusqlite::Connection` which is not `Send + Sync`. The `StateStoreWriter` in `src/runtime/mod.rs:29-49` wraps it in `std::sync::Mutex`, but this is error-prone.

```rust
pub struct StateStoreWriter {
    store: std::sync::Mutex<StateStore>,  // Wrapped, but design is fragile
}
```

**Impact**: Potential deadlocks or data races in concurrent scenarios.

**Recommendation**: Use `tokio::sync::Mutex` for async contexts or consider using a connection pool.

---

### 11. [HIGH] Retry Loop Can Exit Without Attempting
**File**: `src/execution/mod.rs:297-319`

```rust
async fn retry_with<F, Fut>(&self, mut action: F) -> Result<Decimal, ExecutionError>
where ...
{
    let mut delay = self.retry.base_delay_ms;
    for attempt in 0..self.retry.max_attempts {
        match action().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if err.is_transient() && attempt + 1 < self.retry.max_attempts {
                    // sleep and continue
                }
                return Err(err);  // Non-transient errors return immediately
            }
        }
    }
    Err(ExecutionError::Transient("retry attempts exhausted".to_string()))
}
```

If `max_attempts = 0`, the loop never executes and returns "retry attempts exhausted" even though no attempts were made.

---

### 12. [HIGH] Entry Signal Crossing Logic First-Bar Edge Case
**File**: `src/signals/mod.rs:35-39`

```rust
let crossed_into_zone = self
    .prev_z
    .map_or(abs_z >= self.entry_z && abs_z < self.sl_z, |prev| {
        prev.abs() < self.entry_z && abs_z >= self.entry_z && abs_z < self.sl_z
    });
```

On the first bar (`prev_z = None`), if Z is already in the entry zone, it triggers entry. This violates the "crossing into zone" requirement.

**Impact**: May enter positions on startup even when Z didn't cross into the zone.

---

## Medium Priority Issues

### 13. [MEDIUM] Significant Code Duplication Between Strategy and Backtest
**Files**: `src/core/strategy.rs`, `src/backtest/mod.rs`

The `StrategyEngine::process_bar` and `BacktestEngine::run` share ~80% similar logic for:
- Z-score calculation
- Volatility calculation
- Entry signal detection
- Exit signal detection
- Position creation

**Recommendation**: Extract common logic into shared helper functions or a `StrategyCore` struct.

---

### 14. [MEDIUM] Missing Live Funding Source Implementation
**File**: `src/funding/mod.rs`

Only `MockFundingSource` and `ZeroFundingSource` are implemented. No production `VariationalFundingSource` exists.

**Impact**: Live trading will always use zero funding rates.

---

### 15. [MEDIUM] Integration Module Too Simplistic
**File**: `src/integration/mod.rs`

Only 13 lines with basic checks:
```rust
pub fn papertrading_gate(metrics: &Metrics) -> bool {
    metrics.max_drawdown <= Decimal::new(15, 2)
}

pub fn deployment_ready(config: &Config) -> bool {
    config.validate().is_ok()
}
```

**Missing**:
- Sharpe ratio threshold
- Win rate threshold
- Minimum number of trades
- Profit factor check
- API connectivity verification

---

### 16. [MEDIUM] Sharpe Ratio Calculation Issues
**File**: `src/backtest/mod.rs:439-475`

1. Uses sample variance formula but divides by N instead of N-1
2. Risk-free rate handling is complex and potentially incorrect
3. Periods per year calculation assumes uniform time intervals

---

### 17. [MEDIUM] No Rate Limiting for API Calls
**Files**: `src/data/mod.rs`, `src/execution/mod.rs`

No built-in rate limiting for API requests. If the strategy loop runs faster than expected, it could trigger rate limits.

---

### 18. [MEDIUM] Missing Position Limits Validation
**File**: `src/core/strategy.rs`

No validation against exchange position limits or account margin. The strategy could attempt to open positions larger than allowed.

---

### 19. [MEDIUM] Email Transport Trait Has No Implementation
**File**: `src/logging/mod.rs:232-235`

```rust
pub trait EmailTransport: Send + Sync {
    async fn send(&self, subject: &str, body: &str) -> Result<(), AlertError>;
}
```

The trait is defined but no concrete implementation exists (no SMTP client).

---

### 20. [MEDIUM] Decimal::ln() Can Return None
**File**: `src/indicators/mod.rs:95-111`

```rust
pub fn relative_price(eth: Decimal, btc: Decimal) -> Result<Decimal, IndicatorError> {
    if eth <= Decimal::ZERO || btc <= Decimal::ZERO {
        return Err(...);
    }
    Ok(eth.ln() - btc.ln())  // ln() returns Option<Decimal>!
}
```

`Decimal::ln()` returns `Option<Decimal>`, but the code assumes it always succeeds.

---

### 21. [MEDIUM] ewma_std Uses Floating Point
**File**: `src/indicators/mod.rs:113-128`

```rust
pub fn ewma_std(values: &[Decimal], half_life: u32) -> Option<Decimal> {
    // ...
    let decay = Decimal::new(5, 1).powf(1.0 / half_life as f64);  // f64!
    // ...
}
```

Using `f64` in EWMA calculation introduces floating-point imprecision in what should be a high-precision financial calculation.

---

### 22. [MEDIUM] No Graceful Degradation on Partial Data
**File**: `src/data/mod.rs:315-354`

If one price (mid/mark/close) is missing, `effective_price` falls back. But if ALL prices are missing, `fetch_pair_prices` fails entirely instead of gracefully degrading or using last known price.

---

### 23. [MEDIUM] CLI Missing Backtest Subcommand
**File**: `src/cli.rs`

The CLI only supports live trading mode. No way to run backtests from the command line.

---

## Low Priority Issues

### 24. [LOW] Unused `#[allow(unused_crate_dependencies)]`
**Files**: `src/lib.rs:1`, `src/main.rs:1`

This attribute suppresses warnings about unused dependencies, which could mask actual issues.

---

### 25. [LOW] `rust_decimal_macros` Only in dev-dependencies
**File**: `Cargo.toml:30`

The `dec!` macro is very useful for tests but could also be useful in production code for config defaults.

---

### 26. [LOW] Missing `#[derive(Default)]` on Some Structs
**Files**: Various

`RetryConfig`, `RotationConfig`, etc. lack `Default` derives but have `::fast()` or similar constructors.

---

### 27. [LOW] Inconsistent Visibility Modifiers
**Files**: Various

Mix of `pub`, `pub(crate)`, and private items without clear pattern.

---

### 28. [LOW] No Benchmarks Configured
**File**: `Cargo.toml`

`criterion` is in dev-dependencies but no `[[bench]]` entries are defined.

---

### 29. [LOW] `to_vec()` Creates Unnecessary Allocations
**Files**: `src/indicators/mod.rs:52-54`, `src/data/mod.rs:476-478`

```rust
fn to_vec(&self) -> Vec<Decimal> {
    self.values.iter().cloned().collect()
}
```

Could return `&[Decimal]` or an iterator instead.

---

### 30. [LOW] Missing `Debug` on Some Types
**File**: `src/core/strategy.rs:43`

`StrategyEngine` does not derive `Debug`, making debugging harder.

---

### 31. [LOW] Symbol::all() Returns Array, Not Slice
**File**: `src/config/mod.rs:33-35`

```rust
pub fn all() -> [Symbol; 2] {
    [Symbol::EthPerp, Symbol::BtcPerp]
}
```

Should return `&'static [Symbol]` to avoid copying.

---

### 32. [LOW] Test Files Use `#[cfg(test)]` Inconsistently
**Files**: `tests/` directory

Some test modules are in `tests/` directory (integration tests), but unit tests could be colocated with source using `#[cfg(test)]` modules.

---

## Security Observations

### Positive Findings
1. API keys read from environment variables, not hardcoded
2. `redact_json_value()` function properly redacts sensitive fields
3. No SQL injection risk (parameterized queries in SQLite)
4. HTTPS enforced via `rustls-tls`

### Areas of Concern
1. API key could be passed via CLI `--api-key` flag (visible in process list)
2. SQLite state file has no encryption at rest
3. No signature verification for API responses

---

## Performance Observations

### Positive Findings
1. Async/await used throughout for I/O
2. `VecDeque` used for efficient rolling windows
3. `rust_decimal` provides arbitrary precision without runtime allocation

### Areas of Concern
1. `to_vec()` calls create unnecessary allocations in hot paths
2. No caching for repeated calculations
3. File logger does synchronous I/O
4. No connection pooling for HTTP client

---

## Recommendations Summary

### Immediate (Before Production)
1. Fix tp_z validation
2. Fix close_pair atomicity
3. Fix hardcoded funding interval
4. Fix backtest funding calculation
5. Populate all BarLog fields
6. Handle first-bar entry signal edge case

### Short-Term (Within 2 Weeks)
1. Implement production FundingSource
2. Add rate limiting
3. Improve integration gate checks
4. Fix Sharpe ratio calculation
5. Add position limit validation

### Medium-Term (Within 1 Month)
1. Refactor to reduce code duplication
2. Add CLI backtest subcommand
3. Implement email transport
4. Add metrics/monitoring
5. Improve error consistency

---

## Test Coverage Analysis

| Module | Test Files | Coverage Estimate |
|--------|-----------|-------------------|
| config | 2 | 70% |
| data | 3 | 65% |
| indicators | 4 | 80% |
| signals | 2 | 75% |
| position | 2 | 70% |
| funding | 3 | 60% |
| state | 4 | 75% |
| execution | 5 | 70% |
| logging | 7 | 65% |
| backtest | 5 | 60% |
| core | 1 | 50% |
| integration | 10 | 40% |
| runtime | 1 | 30% |
| cli | 1 | 20% |

**Overall Estimated Coverage**: ~60%

### Missing Test Scenarios
1. Concurrent access to StateStore
2. Network failure recovery
3. Extreme market conditions (price gaps, zero liquidity)
4. Configuration hot-reload
5. Memory pressure / OOM conditions

---

## Conclusion

The ETH/BTC mean reversion strategy codebase is well-architected and demonstrates strong Rust fundamentals. The modular design, comprehensive error types, and test coverage provide a solid foundation.

However, the **4 critical issues** must be addressed before production deployment:
1. Configuration validation gap (tp_z)
2. Hardcoded funding interval
3. Non-atomic close_pair operation
4. Inaccurate backtest funding

After addressing these issues and the high-priority items, the system should be suitable for paper trading, with live trading following successful paper validation.

---

*End of Review*
