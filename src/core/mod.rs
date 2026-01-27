use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

pub mod strategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    LongEthShortBtc,
    ShortEthLongBtc,
}

impl TradeDirection {
    pub fn is_eth_long(&self) -> bool {
        matches!(self, TradeDirection::LongEthShortBtc)
    }

    pub fn is_btc_long(&self) -> bool {
        matches!(self, TradeDirection::ShortEthLongBtc)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitReason {
    TakeProfit,
    StopLoss,
    TimeStop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntrySignal {
    pub direction: TradeDirection,
    pub zscore: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitSignal {
    pub reason: ExitReason,
    pub zscore: Decimal,
}
