# Strategy Background Context

## Assumptions

- ETH and BTC are highly correlated long term, but short-term dislocations are common; the strategy targets the mean reversion of the ETH/BTC ratio (features.md section 1.1).
- Signals use relative price only: `ln(ETH) - ln(BTC)` (no directional market view).
- All trading is dual-leg and hedged; single-leg exposure is not allowed.

## Traceability

- [indicators.relative.price](../ai/tasks/indicators/relative.price.md)
- [execution.order.submit](../ai/tasks/execution/order.submit.md)
- [execution.close.atomic](../ai/tasks/execution/close.atomic.md)
- [execution.residual.repair](../ai/tasks/execution/residual.repair.md)
