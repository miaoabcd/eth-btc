# Strategy Goals and Measurable Checks

Reference: features.md section 1.2.

## Goals

1. Capture ETH/BTC mean reversion opportunities.
2. Maintain hedged exposure with dual-leg positions only.
3. Enforce risk controls for extreme moves and overstays.

## Mapping to Modules

- Mean reversion capture: signals, indicators, backtest.
- Hedged exposure: position, execution, state.
- Risk controls: state, funding, execution.

## Measurable Checks

- Entry only on Z crossing into the entry zone; exits on TP/SL/TIME_STOP (signals).
- Both legs always open/close together; residual detection triggers repair (execution, state).
- Cooldown enforced after SL; max-hold exit enforced at 48h (state).
- Funding filter/threshold/size adjustments applied before entry (funding).

## Traceability

- [signals.entry.detect](../ai/tasks/signals/entry.detect.md)
- [signals.exit.detect](../ai/tasks/signals/exit.detect.md)
- [state.machine.implement](../ai/tasks/state/machine.implement.md)
- [execution.order.submit](../ai/tasks/execution/order.submit.md)
- [execution.close.atomic](../ai/tasks/execution/close.atomic.md)
- [funding.filter.apply](../ai/tasks/funding/filter.apply.md)
