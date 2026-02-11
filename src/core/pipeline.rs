use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::config::Config;
use crate::core::{EntrySignal, ExitSignal};
use crate::indicators::{
    IndicatorError, VolatilityCalculator, VolatilitySnapshot, ZScoreCalculator, ZScoreSnapshot,
    relative_price,
};
use crate::signals::{EntrySignalDetector, ExitSignalDetector};
use crate::state::{PositionSnapshot, StrategyStatus};

#[derive(Debug, Clone)]
pub struct SignalOutput {
    pub r: Decimal,
    pub z_snapshot: ZScoreSnapshot,
    pub vol_snapshot: VolatilitySnapshot,
    pub entry_signal: Option<EntrySignal>,
    pub exit_signal: Option<ExitSignal>,
}

#[derive(Debug, Clone)]
pub struct SignalPipeline {
    zcalc: ZScoreCalculator,
    volcalc: VolatilityCalculator,
    entry_detector: EntrySignalDetector,
    exit_detector: ExitSignalDetector,
}

impl SignalPipeline {
    pub fn new(config: &Config) -> Result<Self, IndicatorError> {
        let zcalc = ZScoreCalculator::new(config.strategy.n_z, config.sigma_floor.clone(), 96)?;
        let volcalc = VolatilityCalculator::new(config.position.n_vol)?;
        Ok(Self {
            zcalc,
            volcalc,
            entry_detector: EntrySignalDetector::new(config.strategy.clone()),
            exit_detector: ExitSignalDetector::new(config.strategy.clone(), config.risk.clone()),
        })
    }

    pub fn update(
        &mut self,
        timestamp: DateTime<Utc>,
        eth_price: Decimal,
        btc_price: Decimal,
        status: StrategyStatus,
        position: Option<&PositionSnapshot>,
    ) -> Result<SignalOutput, IndicatorError> {
        let r = relative_price(eth_price, btc_price)?;
        let z_snapshot = self.zcalc.update(r)?;
        let vol_snapshot = self.volcalc.update(eth_price, btc_price)?;
        let entry_signal = self.entry_detector.update(z_snapshot.zscore, status);
        let exit_signal =
            self.exit_detector
                .evaluate(z_snapshot.zscore, status, position, timestamp);
        Ok(SignalOutput {
            r,
            z_snapshot,
            vol_snapshot,
            entry_signal,
            exit_signal,
        })
    }
}
