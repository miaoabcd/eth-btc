use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RiskConfig;
use crate::core::{ExitReason, TradeDirection};

#[derive(Debug, Error)]
pub enum StateError {
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyStatus {
    Flat,
    InPosition,
    Cooldown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionLeg {
    pub qty: Decimal,
    pub avg_price: Decimal,
    pub notional: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionSnapshot {
    pub direction: TradeDirection,
    pub entry_time: DateTime<Utc>,
    pub eth: PositionLeg,
    pub btc: PositionLeg,
}

impl PositionSnapshot {
    pub fn has_residual(&self) -> bool {
        let eth_zero = self.eth.qty == Decimal::ZERO;
        let btc_zero = self.btc.qty == Decimal::ZERO;
        (eth_zero && !btc_zero) || (!eth_zero && btc_zero)
    }

    pub fn is_flat(&self) -> bool {
        self.eth.qty == Decimal::ZERO && self.btc.qty == Decimal::ZERO
    }

    pub fn holding_hours(&self, now: DateTime<Utc>) -> i64 {
        (now - self.entry_time).num_hours()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyState {
    pub status: StrategyStatus,
    pub position: Option<PositionSnapshot>,
    pub cooldown_until: Option<DateTime<Utc>>,
}

impl Default for StrategyState {
    fn default() -> Self {
        Self {
            status: StrategyStatus::Flat,
            position: None,
            cooldown_until: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateMachine {
    state: StrategyState,
    risk: RiskConfig,
}

impl StateMachine {
    pub fn new(risk: RiskConfig) -> Self {
        Self {
            state: StrategyState::default(),
            risk,
        }
    }

    pub fn state(&self) -> &StrategyState {
        &self.state
    }

    pub fn force_flat(&mut self) {
        self.state.status = StrategyStatus::Flat;
        self.state.position = None;
        self.state.cooldown_until = None;
    }

    pub fn hydrate(&mut self, state: StrategyState) -> Result<(), StateError> {
        match state.status {
            StrategyStatus::Flat => {
                if state.position.is_some() {
                    return Err(StateError::InvalidTransition(
                        "flat state cannot contain position".to_string(),
                    ));
                }
            }
            StrategyStatus::InPosition => {
                if state.position.is_none() {
                    return Err(StateError::InvalidTransition(
                        "in-position state missing position".to_string(),
                    ));
                }
            }
            StrategyStatus::Cooldown => {
                if state.cooldown_until.is_none() {
                    return Err(StateError::InvalidTransition(
                        "cooldown state missing cooldown_until".to_string(),
                    ));
                }
            }
        }
        self.state = state;
        Ok(())
    }

    pub fn enter(
        &mut self,
        position: PositionSnapshot,
        now: DateTime<Utc>,
    ) -> Result<(), StateError> {
        if self.state.status != StrategyStatus::Flat {
            return Err(StateError::InvalidTransition(
                "cannot enter unless flat".to_string(),
            ));
        }
        if let Some(cooldown_until) = self.state.cooldown_until
            && now < cooldown_until
        {
            return Err(StateError::InvalidTransition(
                "cannot enter during cooldown".to_string(),
            ));
        }
        self.state.status = StrategyStatus::InPosition;
        self.state.position = Some(position);
        self.state.cooldown_until = None;
        Ok(())
    }

    pub fn exit(&mut self, reason: ExitReason, now: DateTime<Utc>) -> Result<(), StateError> {
        if self.state.status != StrategyStatus::InPosition {
            return Err(StateError::InvalidTransition(
                "cannot exit unless in position".to_string(),
            ));
        }
        match reason {
            ExitReason::StopLoss => {
                let cooldown_until = now + Duration::hours(self.risk.cooldown_hours as i64);
                self.state.status = StrategyStatus::Cooldown;
                self.state.cooldown_until = Some(cooldown_until);
            }
            ExitReason::TakeProfit | ExitReason::TimeStop => {
                self.state.status = StrategyStatus::Flat;
                self.state.cooldown_until = None;
            }
        }
        self.state.position = None;
        Ok(())
    }

    pub fn update(&mut self, now: DateTime<Utc>) {
        if self.state.status == StrategyStatus::Cooldown
            && let Some(cooldown_until) = self.state.cooldown_until
            && now >= cooldown_until
        {
            self.state.status = StrategyStatus::Flat;
            self.state.cooldown_until = None;
        }
    }
}

#[derive(Debug)]
pub struct StateStore {
    conn: rusqlite::Connection,
}

impl StateStore {
    pub fn new(path: &str) -> Result<Self, StateError> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn new_in_memory() -> Result<Self, StateError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), StateError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS strategy_state (\n                    id INTEGER PRIMARY KEY CHECK (id = 1),\n                    state_json TEXT NOT NULL,\n                    updated_at TEXT NOT NULL\n                )",
                [],
            )
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        Ok(())
    }

    pub fn save(&self, state: &StrategyState) -> Result<(), StateError> {
        let payload = serde_json::to_string(state)
            .map_err(|err| StateError::Serialization(err.to_string()))?;
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO strategy_state (id, state_json, updated_at) VALUES (1, ?, ?)",
                [&payload, &now],
            )
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        Ok(())
    }

    pub fn load(&self) -> Result<Option<StrategyState>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state_json FROM strategy_state WHERE id = 1")
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        let mut rows = stmt
            .query([])
            .map_err(|err| StateError::Persistence(err.to_string()))?;
        if let Some(row) = rows
            .next()
            .map_err(|err| StateError::Persistence(err.to_string()))?
        {
            let state_json: String = row
                .get(0)
                .map_err(|err| StateError::Persistence(err.to_string()))?;
            let state = serde_json::from_str(&state_json)
                .map_err(|err| StateError::Serialization(err.to_string()))?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    RepairResidual,
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub state: StrategyState,
    pub actions: Vec<RecoveryAction>,
    pub alerts: Vec<String>,
}

pub fn recover_state(mut state: StrategyState, now: DateTime<Utc>) -> RecoveryReport {
    let mut actions = Vec::new();
    let mut alerts = Vec::new();

    if state.status == StrategyStatus::Cooldown
        && let Some(cooldown_until) = state.cooldown_until
        && now >= cooldown_until
    {
        state.status = StrategyStatus::Flat;
        state.cooldown_until = None;
    }

    if state.status == StrategyStatus::InPosition {
        match &state.position {
            None => {
                alerts.push("missing position while in-position".to_string());
                state.status = StrategyStatus::Flat;
                state.position = None;
            }
            Some(position) => {
                if position.has_residual() {
                    actions.push(RecoveryAction::RepairResidual);
                    alerts.push("residual leg detected on recovery".to_string());
                }
            }
        }
    }

    RecoveryReport {
        state,
        actions,
        alerts,
    }
}
