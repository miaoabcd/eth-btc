use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::config::{RiskConfig, StaleCrossConfig, StrategyConfig};
use crate::core::{EntrySignal, ExitReason, ExitSignal, TradeDirection};
use crate::state::{PositionSnapshot, StrategyStatus};

#[derive(Debug, Clone)]
pub struct EntrySignalDetector {
    entry_z: Decimal,
    sl_z: Decimal,
    stale_cross: StaleCrossConfig,
    prev_z: Option<Decimal>,
    last_status: Option<StrategyStatus>,
    cooldown_recovery_age: Option<u32>,
}

impl EntrySignalDetector {
    pub fn new(config: StrategyConfig) -> Self {
        Self::with_stale_cross(config, StaleCrossConfig::default())
    }

    pub fn with_stale_cross(config: StrategyConfig, stale_cross: StaleCrossConfig) -> Self {
        Self {
            entry_z: config.entry_z,
            sl_z: config.sl_z,
            stale_cross,
            prev_z: None,
            last_status: None,
            cooldown_recovery_age: None,
        }
    }

    pub fn update(
        &mut self,
        zscore: Option<Decimal>,
        state: StrategyStatus,
    ) -> Option<EntrySignal> {
        let Some(zscore) = zscore else {
            self.prev_z = None;
            self.last_status = Some(state);
            self.cooldown_recovery_age = None;
            return None;
        };

        let abs_z = zscore.abs();
        let prev_z = self.prev_z;
        let cooldown_released =
            self.last_status == Some(StrategyStatus::Cooldown) && state == StrategyStatus::Flat;
        let crossed_into_zone = self.prev_z.is_some_and(|prev| {
            prev.abs() < self.entry_z && abs_z >= self.entry_z && abs_z < self.sl_z
        });
        let in_entry_zone = abs_z >= self.entry_z && abs_z < self.sl_z;
        let next_recovery_age = if state == StrategyStatus::Flat && in_entry_zone {
            if cooldown_released {
                Some(1)
            } else {
                self.cooldown_recovery_age.map(|age| age.saturating_add(1))
            }
        } else {
            None
        };
        let stale_cross_recovery = self.stale_cross.enabled
            && !crossed_into_zone
            && in_entry_zone
            && state == StrategyStatus::Flat
            && prev_z.is_some()
            && next_recovery_age.is_some_and(|age| age <= self.stale_cross.max_age_bars)
            && (!self.stale_cross.require_reverting
                || prev_z.is_some_and(|prev| abs_z <= prev.abs()));

        self.prev_z = Some(zscore);
        self.last_status = Some(state);
        self.cooldown_recovery_age = next_recovery_age;

        if state != StrategyStatus::Flat || (!crossed_into_zone && !stale_cross_recovery) {
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
