use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::config::{RiskConfig, StrategyConfig};
use crate::core::{EntrySignal, ExitReason, ExitSignal, TradeDirection};
use crate::state::{PositionSnapshot, StrategyStatus};

#[derive(Debug, Clone)]
pub struct EntrySignalDetector {
    entry_z: Decimal,
    sl_z: Decimal,
    prev_z: Option<Decimal>,
}

impl EntrySignalDetector {
    pub fn new(config: StrategyConfig) -> Self {
        Self {
            entry_z: config.entry_z,
            sl_z: config.sl_z,
            prev_z: None,
        }
    }

    pub fn update(
        &mut self,
        zscore: Option<Decimal>,
        state: StrategyStatus,
    ) -> Option<EntrySignal> {
        let Some(zscore) = zscore else {
            self.prev_z = None;
            return None;
        };

        let abs_z = zscore.abs();
        let crossed_into_zone = self
            .prev_z
            .map_or(abs_z >= self.entry_z && abs_z < self.sl_z, |prev| {
                prev.abs() < self.entry_z && abs_z >= self.entry_z && abs_z < self.sl_z
            });

        self.prev_z = Some(zscore);

        if !crossed_into_zone || state != StrategyStatus::Flat {
            return None;
        }

        let direction = if zscore >= self.entry_z {
            TradeDirection::ShortEthLongBtc
        } else {
            TradeDirection::LongEthShortBtc
        };
        Some(EntrySignal { direction, zscore })
    }
}

#[derive(Debug, Clone)]
pub struct ExitSignalDetector {
    tp_z: Decimal,
    sl_z: Decimal,
    max_hold_hours: u32,
    confirm_bars_tp: u32,
    tp_count: u32,
}

impl ExitSignalDetector {
    pub fn new(strategy: StrategyConfig, risk: RiskConfig) -> Self {
        Self {
            tp_z: strategy.tp_z,
            sl_z: strategy.sl_z,
            max_hold_hours: risk.max_hold_hours,
            confirm_bars_tp: risk.confirm_bars_tp,
            tp_count: 0,
        }
    }

    pub fn evaluate(
        &mut self,
        zscore: Option<Decimal>,
        state: StrategyStatus,
        position: Option<&PositionSnapshot>,
        now: DateTime<Utc>,
    ) -> Option<ExitSignal> {
        if state != StrategyStatus::InPosition {
            self.tp_count = 0;
            return None;
        }

        let position = position?;
        let Some(zscore) = zscore else {
            self.tp_count = 0;
            return None;
        };
        let abs_z = zscore.abs();

        if abs_z >= self.sl_z {
            self.tp_count = 0;
            return Some(ExitSignal {
                reason: ExitReason::StopLoss,
                zscore,
            });
        }

        if abs_z <= self.tp_z {
            if self.confirm_bars_tp == 0 {
                self.tp_count = 0;
                return Some(ExitSignal {
                    reason: ExitReason::TakeProfit,
                    zscore,
                });
            }
            self.tp_count += 1;
            if self.tp_count >= self.confirm_bars_tp {
                self.tp_count = 0;
                return Some(ExitSignal {
                    reason: ExitReason::TakeProfit,
                    zscore,
                });
            }
        } else {
            self.tp_count = 0;
        }

        if position.holding_hours(now) >= self.max_hold_hours as i64 {
            return Some(ExitSignal {
                reason: ExitReason::TimeStop,
                zscore,
            });
        }

        None
    }
}
