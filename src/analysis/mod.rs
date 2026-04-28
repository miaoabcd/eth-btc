use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::config::Symbol;

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("csv header missing")]
    MissingHeader,
    #[error("csv row {row} has {actual} columns, expected at least {expected}")]
    InvalidColumnCount {
        row: usize,
        actual: usize,
        expected: usize,
    },
    #[error("csv row {row} has invalid timestamp {value}: {message}")]
    InvalidTimestamp {
        row: usize,
        value: String,
        message: String,
    },
    #[error("csv row {row} has invalid symbol {value}")]
    InvalidSymbol { row: usize, value: String },
    #[error("csv row {row} has invalid direction {value}")]
    InvalidDirection { row: usize, value: String },
    #[error("csv row {row} has invalid decimal in {field}: {value}")]
    InvalidDecimal {
        row: usize,
        field: &'static str,
        value: String,
    },
    #[error("stats log line {line}: {message}")]
    InvalidStatsLog { line: usize, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CycleKind {
    Paired,
    SingleLeg,
    Unclassified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradeDirection {
    LongEthShortBtc,
    ShortEthLongBtc,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeCycle {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub row_count: usize,
    pub kind: CycleKind,
    pub direction: Option<TradeDirection>,
    pub closed: bool,
    pub net_pnl: Decimal,
    pub fees: Decimal,
    pub gross_pnl: Decimal,
    pub open_notional: Decimal,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CycleSummary {
    pub cycles: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakeven: usize,
    pub net_pnl: Decimal,
    pub fees: Decimal,
    pub gross_pnl: Decimal,
    pub open_notional: Decimal,
    pub win_rate: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub fee_bps: Option<Decimal>,
    pub gross_edge_bps: Option<Decimal>,
    pub net_edge_bps: Option<Decimal>,
    #[serde(skip)]
    positive_pnl: Decimal,
    #[serde(skip)]
    negative_pnl_abs: Decimal,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TradeHistorySummary {
    pub total: CycleSummary,
    pub paired: CycleSummary,
    pub single_leg: CycleSummary,
    pub unclassified: CycleSummary,
    pub directions: BTreeMap<TradeDirection, CycleSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeAttributionReport {
    pub summary: TradeHistorySummary,
    pub cycles: Vec<TradeCycle>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayStrategyConfig {
    pub name: String,
    pub entry_z: Decimal,
    pub tp_z: Decimal,
    pub sl_z: Decimal,
    pub cooldown_recovery: bool,
    pub cooldown_recovery_bars: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsReplayReport {
    pub rows: usize,
    pub strategies: Vec<StatsReplaySummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReplayDirectionSummary {
    pub trades: usize,
    pub total_net_bps: Decimal,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StatsReplaySummary {
    pub name: String,
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakeven: usize,
    pub total_net_bps: Decimal,
    pub avg_net_bps: Option<Decimal>,
    pub win_rate: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub entry_sources: BTreeMap<String, usize>,
    pub exit_reasons: BTreeMap<String, usize>,
    pub directions: BTreeMap<TradeDirection, ReplayDirectionSummary>,
    #[serde(skip)]
    positive_bps: Decimal,
    #[serde(skip)]
    negative_bps_abs: Decimal,
}

#[derive(Debug, Clone)]
struct TradeHistoryRow {
    timestamp: DateTime<Utc>,
    coin: Symbol,
    action: TradeAction,
    size: Decimal,
    notional: Decimal,
    fee: Decimal,
    closed_pnl: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayState {
    Flat,
    Cooldown,
    Other,
}

#[derive(Debug, Clone)]
struct StatsReplayRow {
    timestamp: DateTime<Utc>,
    eth_price: Decimal,
    btc_price: Decimal,
    zscore: Decimal,
    w_eth: Decimal,
    w_btc: Decimal,
    state: ReplayState,
}

#[derive(Debug, Clone)]
struct ReplayOpenPosition {
    entry_index: usize,
    entry: StatsReplayRow,
    direction: TradeDirection,
    source: &'static str,
}

#[derive(Debug, Clone)]
struct ReplayTrade {
    direction: TradeDirection,
    source: &'static str,
    exit_reason: &'static str,
    net_bps: Decimal,
}

#[derive(Debug, Clone, Copy)]
enum TradeAction {
    OpenLong,
    CloseLong,
    OpenShort,
    CloseShort,
}

impl TradeAction {
    fn signed_delta(self, size: Decimal) -> Decimal {
        match self {
            TradeAction::OpenLong | TradeAction::CloseShort => size,
            TradeAction::CloseLong | TradeAction::OpenShort => -size,
        }
    }

    fn is_open(self) -> bool {
        matches!(self, TradeAction::OpenLong | TradeAction::OpenShort)
    }
}

pub fn analyze_trade_history_csv(content: &str) -> Result<Vec<TradeCycle>, AnalysisError> {
    analyze_trade_history_csv_since(content, None)
}

pub fn analyze_trade_history_csv_since(
    content: &str,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<TradeCycle>, AnalysisError> {
    let rows = parse_trade_history_csv(content)?;
    Ok(reconstruct_cycles(rows.into_iter().filter(|row| {
        since.map(|since| row.timestamp >= since).unwrap_or(true)
    })))
}

pub fn build_trade_attribution_report(cycles: Vec<TradeCycle>) -> TradeAttributionReport {
    let summary = summarize_cycles(&cycles);
    TradeAttributionReport { summary, cycles }
}

pub fn summarize_cycles(cycles: &[TradeCycle]) -> TradeHistorySummary {
    let mut summary = TradeHistorySummary::default();
    for cycle in cycles {
        summary.total.add(cycle);
        match cycle.kind {
            CycleKind::Paired => summary.paired.add(cycle),
            CycleKind::SingleLeg => summary.single_leg.add(cycle),
            CycleKind::Unclassified => summary.unclassified.add(cycle),
        }
        if let Some(direction) = cycle.direction {
            summary.directions.entry(direction).or_default().add(cycle);
        }
    }
    summary.total.finalize();
    summary.paired.finalize();
    summary.single_leg.finalize();
    summary.unclassified.finalize();
    for direction in summary.directions.values_mut() {
        direction.finalize();
    }
    summary
}

pub fn format_report_text(report: &TradeAttributionReport) -> String {
    let mut output = String::new();
    output.push_str("trade history attribution\n");
    output.push_str(&format_summary_line("total", &report.summary.total));
    output.push_str(&format_summary_line("paired", &report.summary.paired));
    output.push_str(&format_summary_line(
        "single_leg",
        &report.summary.single_leg,
    ));
    if report.summary.unclassified.cycles > 0 {
        output.push_str(&format_summary_line(
            "unclassified",
            &report.summary.unclassified,
        ));
    }
    for (direction, summary) in &report.summary.directions {
        output.push_str(&format_summary_line(&format!("{direction:?}"), summary));
    }
    output
}

pub fn default_replay_strategy_configs() -> Vec<ReplayStrategyConfig> {
    vec![
        ReplayStrategyConfig {
            name: "cross_ez_1.4".to_string(),
            entry_z: Decimal::new(14, 1),
            tp_z: Decimal::new(45, 2),
            sl_z: Decimal::new(35, 1),
            cooldown_recovery: false,
            cooldown_recovery_bars: 0,
        },
        ReplayStrategyConfig {
            name: "cross_ez_1.2".to_string(),
            entry_z: Decimal::new(12, 1),
            tp_z: Decimal::new(45, 2),
            sl_z: Decimal::new(35, 1),
            cooldown_recovery: false,
            cooldown_recovery_bars: 0,
        },
        ReplayStrategyConfig {
            name: "cooldown_recovery_ez_1.4".to_string(),
            entry_z: Decimal::new(14, 1),
            tp_z: Decimal::new(45, 2),
            sl_z: Decimal::new(35, 1),
            cooldown_recovery: true,
            cooldown_recovery_bars: 4,
        },
        ReplayStrategyConfig {
            name: "cooldown_recovery_ez_1.6".to_string(),
            entry_z: Decimal::new(16, 1),
            tp_z: Decimal::new(45, 2),
            sl_z: Decimal::new(35, 1),
            cooldown_recovery: true,
            cooldown_recovery_bars: 4,
        },
    ]
}

pub fn replay_stats_log(
    content: &str,
    since: Option<DateTime<Utc>>,
    configs: &[ReplayStrategyConfig],
) -> Result<StatsReplayReport, AnalysisError> {
    let rows = parse_stats_replay_rows(content, since)?;
    let strategies = configs
        .iter()
        .map(|config| replay_strategy(&rows, config))
        .collect();
    Ok(StatsReplayReport {
        rows: rows.len(),
        strategies,
    })
}

pub fn format_stats_replay_text(report: &StatsReplayReport) -> String {
    let mut output = String::new();
    output.push_str("stats replay candidates\n");
    output.push_str(
        "name trades wins losses net_bps avg_bps win_rate profit_factor sources exits directions\n",
    );
    for strategy in &report.strategies {
        output.push_str(&format!(
            "{} {} {} {} {} {} {} {} {} {} {}\n",
            strategy.name,
            strategy.trades,
            strategy.wins,
            strategy.losses,
            strategy.total_net_bps,
            fmt_optional(strategy.avg_net_bps),
            fmt_optional(strategy.win_rate),
            fmt_optional(strategy.profit_factor),
            format_count_map(&strategy.entry_sources),
            format_count_map(&strategy.exit_reasons),
            format_replay_directions(&strategy.directions),
        ));
    }
    output
}

impl CycleSummary {
    fn add(&mut self, cycle: &TradeCycle) {
        self.cycles += 1;
        if cycle.net_pnl > Decimal::ZERO {
            self.wins += 1;
            self.positive_pnl += cycle.net_pnl;
        } else if cycle.net_pnl < Decimal::ZERO {
            self.losses += 1;
            self.negative_pnl_abs += cycle.net_pnl.abs();
        } else {
            self.breakeven += 1;
        }
        self.net_pnl += cycle.net_pnl;
        self.fees += cycle.fees;
        self.gross_pnl += cycle.gross_pnl;
        self.open_notional += cycle.open_notional;
    }

    fn finalize(&mut self) {
        if self.cycles > 0 {
            self.win_rate = Some(Decimal::from(self.wins) / Decimal::from(self.cycles));
        }
        if self.open_notional > Decimal::ZERO {
            let bps = Decimal::from(10_000u32);
            self.fee_bps = Some(self.fees / self.open_notional * bps);
            self.gross_edge_bps = Some(self.gross_pnl / self.open_notional * bps);
            self.net_edge_bps = Some(self.net_pnl / self.open_notional * bps);
        }
        if self.negative_pnl_abs > Decimal::ZERO {
            self.profit_factor = Some(self.positive_pnl / self.negative_pnl_abs);
        }
    }
}

fn parse_trade_history_csv(content: &str) -> Result<Vec<TradeHistoryRow>, AnalysisError> {
    let mut lines = content.lines();
    lines.next().ok_or(AnalysisError::MissingHeader)?;
    lines
        .enumerate()
        .filter_map(|(idx, line)| {
            if line.trim().is_empty() {
                None
            } else {
                Some(parse_trade_history_row(idx + 2, line))
            }
        })
        .collect()
}

fn parse_trade_history_row(
    row_number: usize,
    line: &str,
) -> Result<TradeHistoryRow, AnalysisError> {
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 8 {
        return Err(AnalysisError::InvalidColumnCount {
            row: row_number,
            actual: fields.len(),
            expected: 8,
        });
    }
    let timestamp = parse_timestamp(row_number, fields[0])?;
    let coin = parse_symbol(row_number, fields[1])?;
    let action = parse_action(row_number, fields[2])?;
    let size = parse_decimal(row_number, "sz", fields[4])?;
    let notional = parse_decimal(row_number, "ntl", fields[5])?;
    let fee = parse_decimal(row_number, "fee", fields[6])?;
    let closed_pnl = parse_decimal(row_number, "closedPnl", fields[7])?;

    Ok(TradeHistoryRow {
        timestamp,
        coin,
        action,
        size,
        notional,
        fee,
        closed_pnl,
    })
}

fn parse_timestamp(row: usize, value: &str) -> Result<DateTime<Utc>, AnalysisError> {
    let timestamp = NaiveDateTime::parse_from_str(value, "%d/%m/%Y - %H:%M:%S").map_err(|err| {
        AnalysisError::InvalidTimestamp {
            row,
            value: value.to_string(),
            message: err.to_string(),
        }
    })?;
    Ok(DateTime::from_naive_utc_and_offset(timestamp, Utc))
}

fn parse_symbol(row: usize, value: &str) -> Result<Symbol, AnalysisError> {
    match value.trim().to_uppercase().as_str() {
        "ETH" | "ETH-PERP" | "ETH_PERP" => Ok(Symbol::EthPerp),
        "BTC" | "BTC-PERP" | "BTC_PERP" => Ok(Symbol::BtcPerp),
        _ => Err(AnalysisError::InvalidSymbol {
            row,
            value: value.to_string(),
        }),
    }
}

fn parse_action(row: usize, value: &str) -> Result<TradeAction, AnalysisError> {
    match value.trim().to_uppercase().as_str() {
        "OPEN LONG" => Ok(TradeAction::OpenLong),
        "CLOSE LONG" => Ok(TradeAction::CloseLong),
        "OPEN SHORT" => Ok(TradeAction::OpenShort),
        "CLOSE SHORT" => Ok(TradeAction::CloseShort),
        _ => Err(AnalysisError::InvalidDirection {
            row,
            value: value.to_string(),
        }),
    }
}

fn parse_decimal(row: usize, field: &'static str, value: &str) -> Result<Decimal, AnalysisError> {
    Decimal::from_str(value).map_err(|_| AnalysisError::InvalidDecimal {
        row,
        field,
        value: value.to_string(),
    })
}

fn reconstruct_cycles(rows: impl IntoIterator<Item = TradeHistoryRow>) -> Vec<TradeCycle> {
    let mut cycles = Vec::new();
    let mut current = Vec::new();
    let mut eth_qty = Decimal::ZERO;
    let mut btc_qty = Decimal::ZERO;

    for row in rows {
        let delta = row.action.signed_delta(row.size);
        match row.coin {
            Symbol::EthPerp => eth_qty += delta,
            Symbol::BtcPerp => btc_qty += delta,
        }
        current.push(row);
        if eth_qty == Decimal::ZERO && btc_qty == Decimal::ZERO && !current.is_empty() {
            cycles.push(build_cycle(&current, true));
            current.clear();
        }
    }

    if !current.is_empty() {
        cycles.push(build_cycle(&current, false));
    }

    cycles
}

fn build_cycle(rows: &[TradeHistoryRow], closed: bool) -> TradeCycle {
    let start_time = rows
        .first()
        .map(|row| row.timestamp)
        .unwrap_or_else(Utc::now);
    let end_time = rows.last().map(|row| row.timestamp).unwrap_or(start_time);
    let mut eth_qty = Decimal::ZERO;
    let mut btc_qty = Decimal::ZERO;
    let mut saw_eth = false;
    let mut saw_btc = false;
    let mut direction = None;
    let mut net_pnl = Decimal::ZERO;
    let mut fees = Decimal::ZERO;
    let mut open_notional = Decimal::ZERO;

    for row in rows {
        saw_eth |= row.coin == Symbol::EthPerp;
        saw_btc |= row.coin == Symbol::BtcPerp;
        net_pnl += row.closed_pnl;
        fees += row.fee;
        if row.action.is_open() {
            open_notional += row.notional;
        }
        let delta = row.action.signed_delta(row.size);
        match row.coin {
            Symbol::EthPerp => eth_qty += delta,
            Symbol::BtcPerp => btc_qty += delta,
        }
        if direction.is_none() {
            direction = detect_direction(eth_qty, btc_qty);
        }
    }

    let kind = if direction.is_some() {
        CycleKind::Paired
    } else if saw_eth ^ saw_btc {
        CycleKind::SingleLeg
    } else {
        CycleKind::Unclassified
    };

    TradeCycle {
        start_time,
        end_time,
        row_count: rows.len(),
        kind,
        direction,
        closed,
        net_pnl,
        fees,
        gross_pnl: net_pnl + fees,
        open_notional,
    }
}

fn detect_direction(eth_qty: Decimal, btc_qty: Decimal) -> Option<TradeDirection> {
    if eth_qty > Decimal::ZERO && btc_qty < Decimal::ZERO {
        Some(TradeDirection::LongEthShortBtc)
    } else if eth_qty < Decimal::ZERO && btc_qty > Decimal::ZERO {
        Some(TradeDirection::ShortEthLongBtc)
    } else {
        None
    }
}

fn format_summary_line(label: &str, summary: &CycleSummary) -> String {
    format!(
        "{label}: cycles={} wins={} losses={} net_pnl={} fees={} gross_pnl={} open_notional={} win_rate={} profit_factor={} fee_bps={} gross_edge_bps={} net_edge_bps={}\n",
        summary.cycles,
        summary.wins,
        summary.losses,
        summary.net_pnl,
        summary.fees,
        summary.gross_pnl,
        summary.open_notional,
        fmt_optional(summary.win_rate),
        fmt_optional(summary.profit_factor),
        fmt_optional(summary.fee_bps),
        fmt_optional(summary.gross_edge_bps),
        fmt_optional(summary.net_edge_bps),
    )
}

fn fmt_optional(value: Option<Decimal>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_count_map(map: &BTreeMap<String, usize>) -> String {
    if map.is_empty() {
        return "none".to_string();
    }
    map.iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_replay_directions(map: &BTreeMap<TradeDirection, ReplayDirectionSummary>) -> String {
    if map.is_empty() {
        return "none".to_string();
    }
    map.iter()
        .map(|(direction, summary)| {
            format!(
                "{direction:?}:{}trades/{}bps",
                summary.trades, summary.total_net_bps
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

const REPLAY_MAX_HOLD_BARS: usize = 48 * 4;
const REPLAY_COOLDOWN_BARS: usize = 24 * 4;

fn replay_cost_bps() -> Decimal {
    Decimal::new(898, 2)
}

fn parse_stats_replay_rows(
    content: &str,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<StatsReplayRow>, AnalysisError> {
    let mut rows_by_timestamp = BTreeMap::new();
    for (idx, line) in content.lines().enumerate() {
        let line_number = idx + 1;
        if line.trim().is_empty() {
            continue;
        }
        let payload: Value =
            serde_json::from_str(line).map_err(|err| AnalysisError::InvalidStatsLog {
                line: line_number,
                message: err.to_string(),
            })?;
        let Some(row) = parse_stats_replay_row(line_number, &payload)? else {
            continue;
        };
        if since.map(|since| row.timestamp >= since).unwrap_or(true) {
            rows_by_timestamp.insert(row.timestamp, row);
        }
    }
    Ok(rows_by_timestamp.into_values().collect())
}

fn parse_stats_replay_row(
    line: usize,
    payload: &Value,
) -> Result<Option<StatsReplayRow>, AnalysisError> {
    let timestamp = payload
        .get("timestamp")
        .and_then(Value::as_str)
        .ok_or_else(|| AnalysisError::InvalidStatsLog {
            line,
            message: "timestamp missing".to_string(),
        })
        .and_then(|value| parse_rfc3339_stats_timestamp(line, value))?;
    let eth_price = match optional_decimal_field(payload, "eth_price", line)? {
        Some(value) => value,
        None => return Ok(None),
    };
    let btc_price = match optional_decimal_field(payload, "btc_price", line)? {
        Some(value) => value,
        None => return Ok(None),
    };
    let zscore = match optional_decimal_field(payload, "zscore", line)? {
        Some(value) => value,
        None => return Ok(None),
    };
    let w_eth =
        optional_decimal_field(payload, "w_eth", line)?.unwrap_or_else(|| Decimal::new(5, 1));
    let w_btc =
        optional_decimal_field(payload, "w_btc", line)?.unwrap_or_else(|| Decimal::new(5, 1));
    let state = parse_replay_state(payload.get("state"));

    Ok(Some(StatsReplayRow {
        timestamp,
        eth_price,
        btc_price,
        zscore,
        w_eth,
        w_btc,
        state,
    }))
}

fn parse_rfc3339_stats_timestamp(line: usize, value: &str) -> Result<DateTime<Utc>, AnalysisError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| AnalysisError::InvalidStatsLog {
            line,
            message: format!("invalid timestamp {value}: {err}"),
        })
}

fn optional_decimal_field(
    payload: &Value,
    field: &'static str,
    line: usize,
) -> Result<Option<Decimal>, AnalysisError> {
    let Some(value) = payload.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let raw = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        other => {
            return Err(AnalysisError::InvalidStatsLog {
                line,
                message: format!("{field} has unsupported value {other}"),
            });
        }
    };
    Decimal::from_str(&raw)
        .map(Some)
        .map_err(|err| AnalysisError::InvalidStatsLog {
            line,
            message: format!("invalid decimal {field}={raw}: {err}"),
        })
}

fn parse_replay_state(value: Option<&Value>) -> ReplayState {
    match value.and_then(Value::as_str) {
        Some("Flat") => ReplayState::Flat,
        Some("Cooldown") => ReplayState::Cooldown,
        _ => ReplayState::Other,
    }
}

fn replay_strategy(rows: &[StatsReplayRow], config: &ReplayStrategyConfig) -> StatsReplaySummary {
    let mut position: Option<ReplayOpenPosition> = None;
    let mut trades = Vec::new();
    let mut prev_z = None;
    let mut prev_state = None;
    let mut recovery_age = None;
    let mut cooldown_until = None;

    for (idx, row) in rows.iter().enumerate() {
        let mut just_exited_stop = false;
        if let Some(open) = position.as_ref() {
            if let Some(reason) = replay_exit_reason(open, row, idx, config) {
                trades.push(ReplayTrade {
                    direction: open.direction,
                    source: open.source,
                    exit_reason: reason,
                    net_bps: replay_trade_net_bps(open, row),
                });
                position = None;
                if reason == "SL" {
                    cooldown_until = Some(idx + REPLAY_COOLDOWN_BARS);
                    just_exited_stop = true;
                }
            }
        }

        let mut simulated_cooldown_released = false;
        let cooldown_active = if let Some(until) = cooldown_until {
            if idx < until {
                true
            } else {
                cooldown_until = None;
                simulated_cooldown_released = true;
                false
            }
        } else {
            false
        };

        let abs_z = row.zscore.abs();
        let in_entry_zone = abs_z >= config.entry_z && abs_z < config.sl_z;
        let crossed = prev_z.is_some_and(|prev: Decimal| {
            prev.abs() < config.entry_z && abs_z >= config.entry_z && abs_z < config.sl_z
        });
        let actual_cooldown_released =
            prev_state == Some(ReplayState::Cooldown) && row.state == ReplayState::Flat;
        let cooldown_released = actual_cooldown_released || simulated_cooldown_released;
        let next_recovery_age = if in_entry_zone {
            if cooldown_released {
                Some(1)
            } else {
                recovery_age.map(|age: u32| age.saturating_add(1))
            }
        } else {
            None
        };

        if position.is_none() && !cooldown_active && !just_exited_stop {
            let source = if crossed {
                Some("cross")
            } else if config.cooldown_recovery
                && in_entry_zone
                && next_recovery_age.is_some_and(|age| age <= config.cooldown_recovery_bars)
                && prev_z.is_some_and(|prev| abs_z <= prev.abs())
            {
                Some("cooldown_recovery")
            } else {
                None
            };
            if let Some(source) = source {
                position = Some(ReplayOpenPosition {
                    entry_index: idx,
                    entry: row.clone(),
                    direction: if row.zscore >= Decimal::ZERO {
                        TradeDirection::ShortEthLongBtc
                    } else {
                        TradeDirection::LongEthShortBtc
                    },
                    source,
                });
            }
        }

        prev_z = Some(row.zscore);
        prev_state = Some(row.state);
        recovery_age = next_recovery_age;
    }

    summarize_replay_trades(config.name.clone(), &trades)
}

fn replay_exit_reason(
    open: &ReplayOpenPosition,
    row: &StatsReplayRow,
    idx: usize,
    config: &ReplayStrategyConfig,
) -> Option<&'static str> {
    let abs_z = row.zscore.abs();
    if abs_z <= config.tp_z {
        Some("TP")
    } else if abs_z >= config.sl_z {
        Some("SL")
    } else if idx.saturating_sub(open.entry_index) >= REPLAY_MAX_HOLD_BARS {
        Some("TIME")
    } else {
        None
    }
}

fn replay_trade_net_bps(open: &ReplayOpenPosition, exit: &StatsReplayRow) -> Decimal {
    let eth_return = exit.eth_price / open.entry.eth_price - Decimal::ONE;
    let btc_return = exit.btc_price / open.entry.btc_price - Decimal::ONE;
    let gross = match open.direction {
        TradeDirection::LongEthShortBtc => {
            open.entry.w_eth * eth_return - open.entry.w_btc * btc_return
        }
        TradeDirection::ShortEthLongBtc => {
            -open.entry.w_eth * eth_return + open.entry.w_btc * btc_return
        }
    };
    gross * Decimal::from(10_000u32) - replay_cost_bps()
}

fn summarize_replay_trades(name: String, trades: &[ReplayTrade]) -> StatsReplaySummary {
    let mut summary = StatsReplaySummary {
        name,
        ..Default::default()
    };
    for trade in trades {
        summary.trades += 1;
        summary.total_net_bps += trade.net_bps;
        if trade.net_bps > Decimal::ZERO {
            summary.wins += 1;
            summary.positive_bps += trade.net_bps;
        } else if trade.net_bps < Decimal::ZERO {
            summary.losses += 1;
            summary.negative_bps_abs += trade.net_bps.abs();
        } else {
            summary.breakeven += 1;
        }
        *summary
            .entry_sources
            .entry(trade.source.to_string())
            .or_insert(0) += 1;
        *summary
            .exit_reasons
            .entry(trade.exit_reason.to_string())
            .or_insert(0) += 1;
        let direction = summary.directions.entry(trade.direction).or_default();
        direction.trades += 1;
        direction.total_net_bps += trade.net_bps;
    }
    if summary.trades > 0 {
        summary.avg_net_bps = Some(summary.total_net_bps / Decimal::from(summary.trades));
        summary.win_rate = Some(Decimal::from(summary.wins) / Decimal::from(summary.trades));
    }
    if summary.negative_bps_abs > Decimal::ZERO {
        summary.profit_factor = Some(summary.positive_bps / summary.negative_bps_abs);
    }
    summary
}
