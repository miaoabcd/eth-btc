use rust_decimal_macros::dec;

use eth_btc_strategy::analysis::{
    CycleKind, FundingCarryReplayConfig, RegimeStudyConfig, RegimeSweepConfig,
    ReplayStrategyConfig, TradeDirection, analyze_trade_history_csv,
    format_funding_carry_replay_text, format_regime_study_text, format_regime_sweep_text,
    format_stats_replay_text, replay_funding_carry_stats_log, replay_stats_log,
    study_residual_regimes, summarize_cycles, sweep_residual_regime_parameters,
};

#[test]
fn trade_history_analysis_classifies_paired_and_single_leg_cycles() {
    let csv = "\
time,coin,dir,px,sz,ntl,fee,closedPnl
08/03/2026 - 11:15:00,ETH,Open Long,2000,0.1,200,0.02,-0.02
08/03/2026 - 11:15:01,BTC,Open Short,100000,0.002,200,0.02,-0.02
08/03/2026 - 12:00:00,ETH,Close Long,2010,0.1,201,0.02,0.98
08/03/2026 - 12:00:01,BTC,Close Short,99500,0.002,199,0.02,0.98
09/03/2026 - 01:00:00,ETH,Open Short,2100,0.1,210,0.021,-0.021
09/03/2026 - 02:00:00,ETH,Close Short,2110,0.1,211,0.021,-1.021
";

    let cycles = analyze_trade_history_csv(csv).unwrap();

    assert_eq!(cycles.len(), 2);
    assert_eq!(cycles[0].kind, CycleKind::Paired);
    assert_eq!(cycles[0].direction, Some(TradeDirection::LongEthShortBtc));
    assert_eq!(cycles[0].row_count, 4);
    assert_eq!(cycles[0].net_pnl, dec!(1.92));
    assert_eq!(cycles[0].fees, dec!(0.08));
    assert_eq!(cycles[0].gross_pnl, dec!(2.00));
    assert_eq!(cycles[0].open_notional, dec!(400));

    assert_eq!(cycles[1].kind, CycleKind::SingleLeg);
    assert_eq!(cycles[1].direction, None);
    assert_eq!(cycles[1].net_pnl, dec!(-1.042));
    assert_eq!(cycles[1].fees, dec!(0.042));
}

#[test]
fn stats_replay_compares_crossing_and_cooldown_recovery_candidates() {
    let stats = "\
{\"timestamp\":\"2026-04-24T09:45:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"-1.7\",\"sigma_eff\":\"0.01\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Cooldown\"}
{\"timestamp\":\"2026-04-24T10:00:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"-1.6\",\"sigma_eff\":\"0.01\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:15:00Z\",\"eth_price\":\"101\",\"btc_price\":\"100\",\"zscore\":\"-0.4\",\"sigma_eff\":\"0.01\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:30:00Z\",\"eth_price\":\"101\",\"btc_price\":\"100\",\"zscore\":\"0.2\",\"sigma_eff\":\"0.01\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
";
    let configs = vec![
        ReplayStrategyConfig {
            name: "cross".to_string(),
            entry_z: dec!(1.4),
            tp_z: dec!(0.45),
            sl_z: dec!(3.5),
            cooldown_recovery: false,
            cooldown_recovery_bars: 0,
        },
        ReplayStrategyConfig {
            name: "cooldown_recovery".to_string(),
            entry_z: dec!(1.4),
            tp_z: dec!(0.45),
            sl_z: dec!(3.5),
            cooldown_recovery: true,
            cooldown_recovery_bars: 4,
        },
    ];

    let report = replay_stats_log(stats, None, &configs).unwrap();

    assert_eq!(report.rows, 4);
    assert_eq!(report.strategies.len(), 2);
    assert_eq!(report.strategies[0].trades, 0);
    assert_eq!(report.strategies[1].trades, 1);
    assert_eq!(report.strategies[1].wins, 1);
    assert_eq!(
        report.strategies[1].entry_sources.get("cooldown_recovery"),
        Some(&1)
    );
    assert!(report.strategies[1].total_net_bps > dec!(0));

    let text = format_stats_replay_text(&report);
    assert!(text.contains("stats replay candidates"));
    assert!(text.contains("cooldown_recovery"));
}

#[test]
fn residual_regime_study_reports_half_life_and_rolling_beta_candidates() {
    let stats = "\
{\"timestamp\":\"2026-04-24T09:00:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:15:00Z\",\"eth_price\":\"101\",\"btc_price\":\"100.8\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:30:00Z\",\"eth_price\":\"99\",\"btc_price\":\"99.4\",\"zscore\":\"-0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:45:00Z\",\"eth_price\":\"100.5\",\"btc_price\":\"100.2\",\"zscore\":\"0.3\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:00:00Z\",\"eth_price\":\"101.0\",\"btc_price\":\"100.5\",\"zscore\":\"0.8\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:15:00Z\",\"eth_price\":\"102.0\",\"btc_price\":\"100.7\",\"zscore\":\"1.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:30:00Z\",\"eth_price\":\"101.0\",\"btc_price\":\"101.0\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
";
    let config = RegimeStudyConfig {
        lookback_bars: 4,
        max_half_life_bars: 16.0,
        entry_z: dec!(1.0),
        tp_z: dec!(0.45),
        sl_z: dec!(3.5),
    };

    let report = study_residual_regimes(stats, None, &config).unwrap();

    assert_eq!(report.rows, 7);
    assert_eq!(report.evaluated_rows, 3);
    assert_eq!(report.candidates.len(), 4);
    assert!(report.fixed_regime_counts.values().sum::<usize>() > 0);
    assert!(report.residual_regime_counts.values().sum::<usize>() > 0);
    assert!(
        report
            .candidates
            .iter()
            .any(|candidate| candidate.name == "rolling_beta_residual_half_life")
    );

    let text = format_regime_study_text(&report);
    assert!(text.contains("residual regime study"));
    assert!(text.contains("rolling_beta_residual_half_life"));
    assert!(text.contains("fixed_regimes="));
}

#[test]
fn residual_regime_sweep_ranks_parameter_candidates() {
    let stats = "\
{\"timestamp\":\"2026-04-24T09:00:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:15:00Z\",\"eth_price\":\"101\",\"btc_price\":\"100.8\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:30:00Z\",\"eth_price\":\"99\",\"btc_price\":\"99.4\",\"zscore\":\"-0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:45:00Z\",\"eth_price\":\"100.5\",\"btc_price\":\"100.2\",\"zscore\":\"0.3\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:00:00Z\",\"eth_price\":\"101.0\",\"btc_price\":\"100.5\",\"zscore\":\"0.8\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:15:00Z\",\"eth_price\":\"102.0\",\"btc_price\":\"100.7\",\"zscore\":\"1.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:30:00Z\",\"eth_price\":\"101.0\",\"btc_price\":\"101.0\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:45:00Z\",\"eth_price\":\"100.8\",\"btc_price\":\"100.9\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T11:00:00Z\",\"eth_price\":\"103.0\",\"btc_price\":\"101.0\",\"zscore\":\"1.6\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T11:15:00Z\",\"eth_price\":\"101.0\",\"btc_price\":\"101.2\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"state\":\"Flat\"}
";
    let config = RegimeSweepConfig {
        lookback_bars: vec![4, 5],
        entry_z_values: vec![dec!(1.0), dec!(1.5)],
        max_half_life_bars: vec![8.0, 16.0],
        tp_z: dec!(0.45),
        sl_z: dec!(3.5),
        min_trades: 1,
        top_n: 6,
    };

    let report = sweep_residual_regime_parameters(stats, None, &config).unwrap();

    assert_eq!(report.rows, 10);
    assert!(report.runs >= 8);
    assert!(!report.top_candidates.is_empty());
    assert!(report.top_candidates.len() <= 6);
    assert!(
        report
            .top_candidates
            .iter()
            .any(|candidate| candidate.entry_z == dec!(1.5))
    );

    let text = format_regime_sweep_text(&report);
    assert!(text.contains("regime parameter sweep"));
    assert!(text.contains("entry_z"));
    assert!(text.contains("fixed_spread"));
}

#[test]
fn funding_carry_replay_adds_signed_carry_and_gates_entries() {
    let stats = "\
{\"timestamp\":\"2026-04-24T09:00:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"funding_eth\":\"0\",\"funding_btc\":\"0\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:15:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"funding_eth\":\"0\",\"funding_btc\":\"0\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:30:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"1.5\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"funding_eth\":\"0.0001\",\"funding_btc\":\"0\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T09:45:00Z\",\"eth_price\":\"99\",\"btc_price\":\"100\",\"zscore\":\"0.2\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"funding_eth\":\"0.0001\",\"funding_btc\":\"0\",\"state\":\"Flat\"}
{\"timestamp\":\"2026-04-24T10:00:00Z\",\"eth_price\":\"100\",\"btc_price\":\"100\",\"zscore\":\"0.1\",\"w_eth\":\"0.5\",\"w_btc\":\"0.5\",\"funding_eth\":\"0.0001\",\"funding_btc\":\"0\",\"state\":\"Flat\"}
";
    let config = FundingCarryReplayConfig {
        entry_z: dec!(1.4),
        tp_z: dec!(0.45),
        sl_z: dec!(3.5),
        min_net_edge_bps: dec!(0),
        max_hold_hours: 48,
        funding_interval_hours: 1,
    };

    let report = replay_funding_carry_stats_log(stats, None, &config).unwrap();

    assert_eq!(report.rows, 5);
    assert_eq!(report.strategies.len(), 3);
    let price_only = report
        .strategies
        .iter()
        .find(|candidate| candidate.name == "funding_price_only")
        .unwrap();
    let with_carry = report
        .strategies
        .iter()
        .find(|candidate| candidate.name == "funding_signed_carry")
        .unwrap();
    assert_eq!(price_only.trades, 1);
    assert_eq!(with_carry.trades, 1);
    assert!(with_carry.total_net_bps > price_only.total_net_bps);

    let text = format_funding_carry_replay_text(&report);
    assert!(text.contains("funding carry replay"));
    assert!(text.contains("funding_signed_carry"));
}

#[test]
fn trade_history_summary_reports_direction_and_fee_edges() {
    let csv = "\
time,coin,dir,px,sz,ntl,fee,closedPnl
08/03/2026 - 11:15:00,ETH,Open Long,2000,0.1,200,0.02,-0.02
08/03/2026 - 11:15:01,BTC,Open Short,100000,0.002,200,0.02,-0.02
08/03/2026 - 12:00:00,ETH,Close Long,2010,0.1,201,0.02,0.98
08/03/2026 - 12:00:01,BTC,Close Short,99500,0.002,199,0.02,0.98
09/03/2026 - 01:00:00,ETH,Open Short,2100,0.1,210,0.021,-0.021
09/03/2026 - 01:00:01,BTC,Open Long,99000,0.002,198,0.0198,-0.0198
09/03/2026 - 02:00:00,ETH,Close Short,2110,0.1,211,0.021,-1.021
09/03/2026 - 02:00:01,BTC,Close Long,99100,0.002,198.2,0.01982,0.18018
10/03/2026 - 01:00:00,ETH,Open Short,2100,0.1,210,0.021,-0.021
10/03/2026 - 02:00:00,ETH,Close Short,2110,0.1,211,0.021,-1.021
";

    let cycles = analyze_trade_history_csv(csv).unwrap();
    let summary = summarize_cycles(&cycles);

    assert_eq!(summary.total.cycles, 3);
    assert_eq!(summary.paired.cycles, 2);
    assert_eq!(summary.single_leg.cycles, 1);
    assert_eq!(summary.paired.wins, 1);
    assert_eq!(summary.paired.losses, 1);
    assert_eq!(summary.single_leg.losses, 1);
    assert_eq!(
        summary
            .directions
            .get(&TradeDirection::LongEthShortBtc)
            .unwrap()
            .cycles,
        1
    );
    assert_eq!(
        summary
            .directions
            .get(&TradeDirection::ShortEthLongBtc)
            .unwrap()
            .cycles,
        1
    );
    assert!(summary.paired.fee_bps.unwrap() > dec!(0));
    assert!(summary.paired.net_edge_bps.unwrap() > dec!(0));
}
