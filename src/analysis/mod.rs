use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
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

#[derive(Debug, Clone, Serialize)]
pub struct RegimeStudyConfig {
    pub lookback_bars: usize,
    pub max_half_life_bars: f64,
    pub entry_z: Decimal,
    pub tp_z: Decimal,
    pub sl_z: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeStudyReport {
    pub rows: usize,
    pub evaluated_rows: usize,
    pub lookback_bars: usize,
    pub max_half_life_bars: f64,
    pub median_beta: Option<f64>,
    pub median_fixed_half_life_bars: Option<f64>,
    pub median_residual_half_life_bars: Option<f64>,
    pub fixed_regime_counts: BTreeMap<String, usize>,
    pub residual_regime_counts: BTreeMap<String, usize>,
    pub candidates: Vec<StatsReplaySummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeSweepConfig {
    pub lookback_bars: Vec<usize>,
    pub entry_z_values: Vec<Decimal>,
    pub max_half_life_bars: Vec<f64>,
    pub tp_z: Decimal,
    pub sl_z: Decimal,
    pub min_trades: usize,
    pub top_n: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeSweepReport {
    pub rows: usize,
    pub runs: usize,
    pub min_trades: usize,
    pub top_n: usize,
    pub top_candidates: Vec<RegimeSweepCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeSweepCandidate {
    pub name: String,
    pub lookback_bars: usize,
    pub entry_z: Decimal,
    pub max_half_life_bars: Option<f64>,
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub total_net_bps: Decimal,
    pub avg_net_bps: Option<Decimal>,
    pub win_rate: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub exit_reasons: BTreeMap<String, usize>,
    pub directions: BTreeMap<TradeDirection, ReplayDirectionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FundingCarryReplayConfig {
    pub entry_z: Decimal,
    pub tp_z: Decimal,
    pub sl_z: Decimal,
    pub min_net_edge_bps: Decimal,
    pub max_hold_hours: u32,
    pub funding_interval_hours: u32,
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
    funding_eth: Option<Decimal>,
    funding_btc: Option<Decimal>,
    state: ReplayState,
}

#[derive(Debug, Clone)]
struct RegimeStatsRow {
    eth_price: Decimal,
    btc_price: Decimal,
    log_eth: f64,
    log_btc: f64,
    fixed_spread: f64,
    zscore: Decimal,
    w_eth: Decimal,
    w_btc: Decimal,
}

#[derive(Debug, Clone)]
struct RegimeEvaluatedRow {
    source: RegimeStatsRow,
    beta: Option<f64>,
    fixed_half_life: Option<f64>,
    residual_zscore: Option<Decimal>,
    residual_half_life: Option<f64>,
    residual_w_eth: Option<Decimal>,
    residual_w_btc: Option<Decimal>,
    fixed_regime: String,
    residual_regime: String,
}

#[derive(Debug, Clone)]
struct FilteredReplayRow {
    eth_price: Decimal,
    btc_price: Decimal,
    zscore: Decimal,
    w_eth: Decimal,
    w_btc: Decimal,
    entry_allowed: bool,
}

#[derive(Debug, Clone)]
struct FilteredOpenPosition {
    entry_index: usize,
    entry: FilteredReplayRow,
    direction: TradeDirection,
    source: &'static str,
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

pub fn replay_funding_carry_stats_log(
    content: &str,
    since: Option<DateTime<Utc>>,
    config: &FundingCarryReplayConfig,
) -> Result<StatsReplayReport, AnalysisError> {
    let rows = parse_stats_replay_rows(content, since)?;
    let price_only_config = ReplayStrategyConfig {
        name: "funding_price_only".to_string(),
        entry_z: config.entry_z,
        tp_z: config.tp_z,
        sl_z: config.sl_z,
        cooldown_recovery: false,
        cooldown_recovery_bars: 0,
    };
    let strategies = vec![
        replay_strategy(&rows, &price_only_config),
        replay_funding_carry_strategy("funding_signed_carry", &rows, config, false),
        replay_funding_carry_strategy("funding_carry_gate", &rows, config, true),
    ];
    Ok(StatsReplayReport {
        rows: rows.len(),
        strategies,
    })
}

pub fn format_funding_carry_replay_text(report: &StatsReplayReport) -> String {
    let mut output = String::new();
    output.push_str("funding carry replay\n");
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

pub fn study_residual_regimes(
    content: &str,
    since: Option<DateTime<Utc>>,
    config: &RegimeStudyConfig,
) -> Result<RegimeStudyReport, AnalysisError> {
    let rows = parse_stats_regime_rows(content, since)?;
    let evaluated = evaluate_regime_rows(&rows, config.lookback_bars);
    let fixed_rows = build_fixed_regime_replay_rows(&evaluated, false, config);
    let fixed_half_life_rows = build_fixed_regime_replay_rows(&evaluated, true, config);
    let residual_rows = build_residual_regime_replay_rows(&evaluated, false, config);
    let residual_half_life_rows = build_residual_regime_replay_rows(&evaluated, true, config);
    let fixed_regime_counts = count_regimes(&evaluated, |row| row.fixed_regime.as_str());
    let residual_regime_counts = count_regimes(&evaluated, |row| row.residual_regime.as_str());
    let candidates = vec![
        replay_filtered_strategy(
            "fixed_spread_baseline".to_string(),
            &fixed_rows,
            config.entry_z,
            config.tp_z,
            config.sl_z,
        ),
        replay_filtered_strategy(
            "fixed_spread_half_life".to_string(),
            &fixed_half_life_rows,
            config.entry_z,
            config.tp_z,
            config.sl_z,
        ),
        replay_filtered_strategy(
            "rolling_beta_residual".to_string(),
            &residual_rows,
            config.entry_z,
            config.tp_z,
            config.sl_z,
        ),
        replay_filtered_strategy(
            "rolling_beta_residual_half_life".to_string(),
            &residual_half_life_rows,
            config.entry_z,
            config.tp_z,
            config.sl_z,
        ),
    ];

    Ok(RegimeStudyReport {
        rows: rows.len(),
        evaluated_rows: evaluated.len(),
        lookback_bars: config.lookback_bars,
        max_half_life_bars: config.max_half_life_bars,
        median_beta: median(evaluated.iter().filter_map(|row| row.beta).collect()),
        median_fixed_half_life_bars: median(
            evaluated
                .iter()
                .filter_map(|row| row.fixed_half_life)
                .collect(),
        ),
        median_residual_half_life_bars: median(
            evaluated
                .iter()
                .filter_map(|row| row.residual_half_life)
                .collect(),
        ),
        fixed_regime_counts,
        residual_regime_counts,
        candidates,
    })
}

pub fn format_regime_study_text(report: &RegimeStudyReport) -> String {
    let mut output = String::new();
    output.push_str("residual regime study\n");
    output.push_str(&format!(
        "rows={} evaluated_rows={} lookback_bars={} max_half_life_bars={} median_beta={} median_fixed_half_life_bars={} median_residual_half_life_bars={} fixed_regimes={} residual_regimes={}\n",
        report.rows,
        report.evaluated_rows,
        report.lookback_bars,
        report.max_half_life_bars,
        fmt_optional_f64(report.median_beta),
        fmt_optional_f64(report.median_fixed_half_life_bars),
        fmt_optional_f64(report.median_residual_half_life_bars),
        format_count_map(&report.fixed_regime_counts),
        format_count_map(&report.residual_regime_counts),
    ));
    output.push_str(
        "name trades wins losses net_bps avg_bps win_rate profit_factor sources exits directions\n",
    );
    for candidate in &report.candidates {
        output.push_str(&format!(
            "{} {} {} {} {} {} {} {} {} {} {}\n",
            candidate.name,
            candidate.trades,
            candidate.wins,
            candidate.losses,
            candidate.total_net_bps,
            fmt_optional(candidate.avg_net_bps),
            fmt_optional(candidate.win_rate),
            fmt_optional(candidate.profit_factor),
            format_count_map(&candidate.entry_sources),
            format_count_map(&candidate.exit_reasons),
            format_replay_directions(&candidate.directions),
        ));
    }
    output
}

pub fn sweep_residual_regime_parameters(
    content: &str,
    since: Option<DateTime<Utc>>,
    config: &RegimeSweepConfig,
) -> Result<RegimeSweepReport, AnalysisError> {
    let rows = parse_stats_regime_rows(content, since)?;
    let mut candidates = Vec::new();
    let mut runs = 0;

    for &lookback_bars in &config.lookback_bars {
        let evaluated = evaluate_regime_rows(&rows, lookback_bars);
        for &entry_z in &config.entry_z_values {
            let base_config = RegimeStudyConfig {
                lookback_bars,
                max_half_life_bars: f64::INFINITY,
                entry_z,
                tp_z: config.tp_z,
                sl_z: config.sl_z,
            };
            let fixed_rows = build_fixed_regime_replay_rows(&evaluated, false, &base_config);
            let residual_rows = build_residual_regime_replay_rows(&evaluated, false, &base_config);
            runs += 2;
            push_sweep_candidate(
                &mut candidates,
                replay_filtered_strategy(
                    "fixed_spread_baseline".to_string(),
                    &fixed_rows,
                    entry_z,
                    config.tp_z,
                    config.sl_z,
                ),
                lookback_bars,
                entry_z,
                None,
                config.min_trades,
            );
            push_sweep_candidate(
                &mut candidates,
                replay_filtered_strategy(
                    "rolling_beta_residual".to_string(),
                    &residual_rows,
                    entry_z,
                    config.tp_z,
                    config.sl_z,
                ),
                lookback_bars,
                entry_z,
                None,
                config.min_trades,
            );

            for &max_half_life_bars in &config.max_half_life_bars {
                let half_life_config = RegimeStudyConfig {
                    lookback_bars,
                    max_half_life_bars,
                    entry_z,
                    tp_z: config.tp_z,
                    sl_z: config.sl_z,
                };
                let fixed_half_life_rows =
                    build_fixed_regime_replay_rows(&evaluated, true, &half_life_config);
                let residual_half_life_rows =
                    build_residual_regime_replay_rows(&evaluated, true, &half_life_config);
                runs += 2;
                push_sweep_candidate(
                    &mut candidates,
                    replay_filtered_strategy(
                        "fixed_spread_half_life".to_string(),
                        &fixed_half_life_rows,
                        entry_z,
                        config.tp_z,
                        config.sl_z,
                    ),
                    lookback_bars,
                    entry_z,
                    Some(max_half_life_bars),
                    config.min_trades,
                );
                push_sweep_candidate(
                    &mut candidates,
                    replay_filtered_strategy(
                        "rolling_beta_residual_half_life".to_string(),
                        &residual_half_life_rows,
                        entry_z,
                        config.tp_z,
                        config.sl_z,
                    ),
                    lookback_bars,
                    entry_z,
                    Some(max_half_life_bars),
                    config.min_trades,
                );
            }
        }
    }

    candidates.sort_by(|left, right| {
        right
            .total_net_bps
            .cmp(&left.total_net_bps)
            .then_with(|| right.trades.cmp(&left.trades))
            .then_with(|| left.name.cmp(&right.name))
    });
    candidates.truncate(config.top_n);

    Ok(RegimeSweepReport {
        rows: rows.len(),
        runs,
        min_trades: config.min_trades,
        top_n: config.top_n,
        top_candidates: candidates,
    })
}

pub fn format_regime_sweep_text(report: &RegimeSweepReport) -> String {
    let mut output = String::new();
    output.push_str("regime parameter sweep\n");
    output.push_str(&format!(
        "rows={} runs={} min_trades={} top_n={}\n",
        report.rows, report.runs, report.min_trades, report.top_n
    ));
    output.push_str(
        "name lookback entry_z max_half_life trades wins losses net_bps avg_bps win_rate profit_factor exits directions\n",
    );
    for candidate in &report.top_candidates {
        output.push_str(&format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {}\n",
            candidate.name,
            candidate.lookback_bars,
            candidate.entry_z,
            fmt_optional_f64(candidate.max_half_life_bars),
            candidate.trades,
            candidate.wins,
            candidate.losses,
            candidate.total_net_bps,
            fmt_optional(candidate.avg_net_bps),
            fmt_optional(candidate.win_rate),
            fmt_optional(candidate.profit_factor),
            format_count_map(&candidate.exit_reasons),
            format_replay_directions(&candidate.directions),
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

fn fmt_optional_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.4}"))
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

fn count_regimes(
    rows: &[RegimeEvaluatedRow],
    selector: impl Fn(&RegimeEvaluatedRow) -> &str,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(selector(row).to_string()).or_insert(0) += 1;
    }
    counts
}

fn push_sweep_candidate(
    candidates: &mut Vec<RegimeSweepCandidate>,
    summary: StatsReplaySummary,
    lookback_bars: usize,
    entry_z: Decimal,
    max_half_life_bars: Option<f64>,
    min_trades: usize,
) {
    if summary.trades < min_trades {
        return;
    }
    candidates.push(RegimeSweepCandidate {
        name: summary.name,
        lookback_bars,
        entry_z,
        max_half_life_bars,
        trades: summary.trades,
        wins: summary.wins,
        losses: summary.losses,
        total_net_bps: summary.total_net_bps,
        avg_net_bps: summary.avg_net_bps,
        win_rate: summary.win_rate,
        profit_factor: summary.profit_factor,
        exit_reasons: summary.exit_reasons,
        directions: summary.directions,
    });
}

fn decimal_to_positive_f64(
    value: Decimal,
    field: &'static str,
    line: usize,
) -> Result<f64, AnalysisError> {
    let value = value
        .to_f64()
        .ok_or_else(|| AnalysisError::InvalidStatsLog {
            line,
            message: format!("{field} cannot be represented as f64"),
        })?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(AnalysisError::InvalidStatsLog {
            line,
            message: format!("{field} must be positive, got {value}"),
        })
    }
}

fn decimal_from_f64_for_analysis(value: f64) -> Option<Decimal> {
    if value.is_finite() {
        Decimal::from_f64(value)
    } else {
        None
    }
}

fn standard_score(value: f64, sample: &[f64]) -> Option<f64> {
    let mean = mean(sample)?;
    let variance = sample
        .iter()
        .map(|item| {
            let diff = item - mean;
            diff * diff
        })
        .sum::<f64>()
        / sample.len() as f64;
    let stddev = variance.sqrt();
    if stddev.is_finite() && stddev > 1e-12 {
        Some((value - mean) / stddev)
    } else {
        None
    }
}

fn estimate_half_life_bars(series: &[f64]) -> Option<f64> {
    if series.len() < 3 {
        return None;
    }
    let lagged = series[..series.len() - 1].to_vec();
    let deltas = series
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<Vec<_>>();
    let (_, slope) = ols_alpha_beta(&lagged, &deltas)?;
    if slope < 0.0 {
        let half_life = -std::f64::consts::LN_2 / slope;
        if half_life.is_finite() && half_life > 0.0 {
            return Some(half_life);
        }
    }
    None
}

fn ols_alpha_beta(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let x_mean = mean(x)?;
    let y_mean = mean(y)?;
    let mut cov = 0.0;
    let mut var = 0.0;
    for (x_value, y_value) in x.iter().zip(y.iter()) {
        let x_diff = x_value - x_mean;
        cov += x_diff * (y_value - y_mean);
        var += x_diff * x_diff;
    }
    if !var.is_finite() || var <= 1e-18 {
        return None;
    }
    let beta = cov / var;
    let alpha = y_mean - beta * x_mean;
    if alpha.is_finite() && beta.is_finite() {
        Some((alpha, beta))
    } else {
        None
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    mean.is_finite().then_some(mean)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Some(values[middle])
    }
}

fn classify_half_life_regime(half_life: Option<f64>) -> String {
    match half_life {
        Some(value) if value <= 16.0 => "fast".to_string(),
        Some(value) if value <= 48.0 => "medium".to_string(),
        Some(value) if value <= 96.0 => "slow".to_string(),
        Some(_) => "too_slow".to_string(),
        None => "non_reverting".to_string(),
    }
}

fn half_life_allowed(half_life: Option<f64>, max_half_life_bars: f64) -> bool {
    half_life.is_some_and(|value| value <= max_half_life_bars)
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
    let funding_eth = optional_decimal_field(payload, "funding_eth", line)?;
    let funding_btc = optional_decimal_field(payload, "funding_btc", line)?;
    let state = parse_replay_state(payload.get("state"));

    Ok(Some(StatsReplayRow {
        timestamp,
        eth_price,
        btc_price,
        zscore,
        w_eth,
        w_btc,
        funding_eth,
        funding_btc,
        state,
    }))
}

fn parse_stats_regime_rows(
    content: &str,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<RegimeStatsRow>, AnalysisError> {
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
        if since.map(|since| row.timestamp < since).unwrap_or(false) {
            continue;
        }
        let eth_price = decimal_to_positive_f64(row.eth_price, "eth_price", line_number)?;
        let btc_price = decimal_to_positive_f64(row.btc_price, "btc_price", line_number)?;
        let log_eth = eth_price.ln();
        let log_btc = btc_price.ln();
        rows_by_timestamp.insert(
            row.timestamp,
            RegimeStatsRow {
                eth_price: row.eth_price,
                btc_price: row.btc_price,
                log_eth,
                log_btc,
                fixed_spread: log_eth - log_btc,
                zscore: row.zscore,
                w_eth: row.w_eth,
                w_btc: row.w_btc,
            },
        );
    }
    Ok(rows_by_timestamp.into_values().collect())
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

fn evaluate_regime_rows(rows: &[RegimeStatsRow], lookback_bars: usize) -> Vec<RegimeEvaluatedRow> {
    if lookback_bars < 3 || rows.len() <= lookback_bars {
        return Vec::new();
    }
    let mut evaluated = Vec::with_capacity(rows.len().saturating_sub(lookback_bars));
    for idx in lookback_bars..rows.len() {
        let window = &rows[idx - lookback_bars..idx];
        let source = rows[idx].clone();
        let fixed_series = window
            .iter()
            .map(|row| row.fixed_spread)
            .collect::<Vec<_>>();
        let fixed_half_life = estimate_half_life_bars(&fixed_series);
        let regression = ols_alpha_beta(
            &window.iter().map(|row| row.log_btc).collect::<Vec<_>>(),
            &window.iter().map(|row| row.log_eth).collect::<Vec<_>>(),
        );
        let (beta, residual_zscore, residual_half_life, residual_w_eth, residual_w_btc) =
            if let Some((alpha, beta)) = regression {
                let residuals = window
                    .iter()
                    .map(|row| row.log_eth - (alpha + beta * row.log_btc))
                    .collect::<Vec<_>>();
                let current_residual = source.log_eth - (alpha + beta * source.log_btc);
                let residual_zscore = standard_score(current_residual, &residuals)
                    .and_then(decimal_from_f64_for_analysis);
                let residual_half_life = estimate_half_life_bars(&residuals);
                let hedge_weight = beta.abs();
                let (residual_w_eth, residual_w_btc) =
                    if hedge_weight.is_finite() && hedge_weight > 0.0 {
                        (
                            decimal_from_f64_for_analysis(1.0 / (1.0 + hedge_weight)),
                            decimal_from_f64_for_analysis(hedge_weight / (1.0 + hedge_weight)),
                        )
                    } else {
                        (None, None)
                    };
                (
                    Some(beta),
                    residual_zscore,
                    residual_half_life,
                    residual_w_eth,
                    residual_w_btc,
                )
            } else {
                (None, None, None, None, None)
            };
        let fixed_regime = classify_half_life_regime(fixed_half_life);
        let residual_regime = classify_half_life_regime(residual_half_life);
        evaluated.push(RegimeEvaluatedRow {
            source,
            beta,
            fixed_half_life,
            residual_zscore,
            residual_half_life,
            residual_w_eth,
            residual_w_btc,
            fixed_regime,
            residual_regime,
        });
    }
    evaluated
}

fn build_fixed_regime_replay_rows(
    rows: &[RegimeEvaluatedRow],
    use_half_life_filter: bool,
    config: &RegimeStudyConfig,
) -> Vec<FilteredReplayRow> {
    rows.iter()
        .map(|row| FilteredReplayRow {
            eth_price: row.source.eth_price,
            btc_price: row.source.btc_price,
            zscore: row.source.zscore,
            w_eth: row.source.w_eth,
            w_btc: row.source.w_btc,
            entry_allowed: !use_half_life_filter
                || half_life_allowed(row.fixed_half_life, config.max_half_life_bars),
        })
        .collect()
}

fn build_residual_regime_replay_rows(
    rows: &[RegimeEvaluatedRow],
    use_half_life_filter: bool,
    config: &RegimeStudyConfig,
) -> Vec<FilteredReplayRow> {
    rows.iter()
        .filter_map(|row| {
            let zscore = row.residual_zscore?;
            let w_eth = row.residual_w_eth?;
            let w_btc = row.residual_w_btc?;
            Some(FilteredReplayRow {
                eth_price: row.source.eth_price,
                btc_price: row.source.btc_price,
                zscore,
                w_eth,
                w_btc,
                entry_allowed: !use_half_life_filter
                    || half_life_allowed(row.residual_half_life, config.max_half_life_bars),
            })
        })
        .collect()
}

fn replay_filtered_strategy(
    name: String,
    rows: &[FilteredReplayRow],
    entry_z: Decimal,
    tp_z: Decimal,
    sl_z: Decimal,
) -> StatsReplaySummary {
    let mut position: Option<FilteredOpenPosition> = None;
    let mut trades = Vec::new();
    let mut prev_z = None;
    let mut cooldown_until = None;

    for (idx, row) in rows.iter().enumerate() {
        let mut just_exited_stop = false;
        if let Some(open) = position.as_ref()
            && let Some(reason) = filtered_replay_exit_reason(open, row, idx, tp_z, sl_z)
        {
            trades.push(ReplayTrade {
                direction: open.direction,
                source: open.source,
                exit_reason: reason,
                net_bps: filtered_replay_trade_net_bps(open, row),
            });
            position = None;
            if reason == "SL" {
                cooldown_until = Some(idx + REPLAY_COOLDOWN_BARS);
                just_exited_stop = true;
            }
        }

        let cooldown_active = if let Some(until) = cooldown_until {
            if idx < until {
                true
            } else {
                cooldown_until = None;
                false
            }
        } else {
            false
        };
        let abs_z = row.zscore.abs();
        let crossed = prev_z
            .is_some_and(|prev: Decimal| prev.abs() < entry_z && abs_z >= entry_z && abs_z < sl_z);
        if position.is_none()
            && !cooldown_active
            && !just_exited_stop
            && row.entry_allowed
            && crossed
        {
            position = Some(FilteredOpenPosition {
                entry_index: idx,
                entry: row.clone(),
                direction: if row.zscore >= Decimal::ZERO {
                    TradeDirection::ShortEthLongBtc
                } else {
                    TradeDirection::LongEthShortBtc
                },
                source: "cross",
            });
        }

        prev_z = Some(row.zscore);
    }

    summarize_replay_trades(name, &trades)
}

fn filtered_replay_exit_reason(
    open: &FilteredOpenPosition,
    row: &FilteredReplayRow,
    idx: usize,
    tp_z: Decimal,
    sl_z: Decimal,
) -> Option<&'static str> {
    let abs_z = row.zscore.abs();
    if abs_z <= tp_z {
        Some("TP")
    } else if abs_z >= sl_z {
        Some("SL")
    } else if idx.saturating_sub(open.entry_index) >= REPLAY_MAX_HOLD_BARS {
        Some("TIME")
    } else {
        None
    }
}

fn filtered_replay_trade_net_bps(open: &FilteredOpenPosition, exit: &FilteredReplayRow) -> Decimal {
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

fn replay_funding_carry_strategy(
    name: &str,
    rows: &[StatsReplayRow],
    config: &FundingCarryReplayConfig,
    enforce_gate: bool,
) -> StatsReplaySummary {
    let mut position: Option<ReplayOpenPosition> = None;
    let mut trades = Vec::new();
    let mut prev_z = None;
    let mut cooldown_until = None;

    for (idx, row) in rows.iter().enumerate() {
        let mut just_exited_stop = false;
        if let Some(open) = position.as_ref()
            && let Some(reason) = replay_exit_reason(
                open,
                row,
                idx,
                &ReplayStrategyConfig {
                    name: name.to_string(),
                    entry_z: config.entry_z,
                    tp_z: config.tp_z,
                    sl_z: config.sl_z,
                    cooldown_recovery: false,
                    cooldown_recovery_bars: 0,
                },
            )
        {
            trades.push(ReplayTrade {
                direction: open.direction,
                source: open.source,
                exit_reason: reason,
                net_bps: replay_trade_net_bps(open, row)
                    + funding_carry_bps(
                        &open.entry,
                        open.direction,
                        idx.saturating_sub(open.entry_index),
                        config.funding_interval_hours,
                    ),
            });
            position = None;
            if reason == "SL" {
                cooldown_until = Some(idx + REPLAY_COOLDOWN_BARS);
                just_exited_stop = true;
            }
        }

        let cooldown_active = if let Some(until) = cooldown_until {
            if idx < until {
                true
            } else {
                cooldown_until = None;
                false
            }
        } else {
            false
        };
        let abs_z = row.zscore.abs();
        let crossed = prev_z.is_some_and(|prev: Decimal| {
            prev.abs() < config.entry_z && abs_z >= config.entry_z && abs_z < config.sl_z
        });
        if position.is_none() && !cooldown_active && !just_exited_stop && crossed {
            let direction = if row.zscore >= Decimal::ZERO {
                TradeDirection::ShortEthLongBtc
            } else {
                TradeDirection::LongEthShortBtc
            };
            let carry_gate_pass = !enforce_gate
                || funding_carry_bps_for_hours(
                    row,
                    direction,
                    config.max_hold_hours,
                    config.funding_interval_hours,
                ) >= config.min_net_edge_bps;
            if carry_gate_pass {
                position = Some(ReplayOpenPosition {
                    entry_index: idx,
                    entry: row.clone(),
                    direction,
                    source: if enforce_gate {
                        "funding_carry_gate"
                    } else {
                        "cross"
                    },
                });
            }
        }

        prev_z = Some(row.zscore);
    }

    summarize_replay_trades(name.to_string(), &trades)
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

fn funding_carry_bps(
    entry: &StatsReplayRow,
    direction: TradeDirection,
    holding_bars: usize,
    funding_interval_hours: u32,
) -> Decimal {
    if funding_interval_hours == 0 {
        return Decimal::ZERO;
    }
    let holding_minutes = Decimal::from(holding_bars as u64) * Decimal::from(15u32);
    let holding_hours = holding_minutes / Decimal::from(60u32);
    funding_carry_bps_for_decimal_hours(entry, direction, holding_hours, funding_interval_hours)
}

fn funding_carry_bps_for_hours(
    entry: &StatsReplayRow,
    direction: TradeDirection,
    holding_hours: u32,
    funding_interval_hours: u32,
) -> Decimal {
    funding_carry_bps_for_decimal_hours(
        entry,
        direction,
        Decimal::from(holding_hours),
        funding_interval_hours,
    )
}

fn funding_carry_bps_for_decimal_hours(
    entry: &StatsReplayRow,
    direction: TradeDirection,
    holding_hours: Decimal,
    funding_interval_hours: u32,
) -> Decimal {
    if funding_interval_hours == 0 {
        return Decimal::ZERO;
    }
    let Some(funding_eth) = entry.funding_eth else {
        return Decimal::ZERO;
    };
    let Some(funding_btc) = entry.funding_btc else {
        return Decimal::ZERO;
    };
    let per_interval_cost = match direction {
        TradeDirection::LongEthShortBtc => funding_eth * entry.w_eth - funding_btc * entry.w_btc,
        TradeDirection::ShortEthLongBtc => -funding_eth * entry.w_eth + funding_btc * entry.w_btc,
    };
    let intervals = holding_hours / Decimal::from(funding_interval_hours);
    -per_interval_cost * intervals * Decimal::from(10_000u32)
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
