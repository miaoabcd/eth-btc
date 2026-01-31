use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::Config;
use eth_btc_strategy::core::pipeline::SignalPipeline;
use eth_btc_strategy::state::StrategyStatus;

#[test]
fn pipeline_emits_entry_on_crossing() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);

    let mut pipeline = SignalPipeline::new(&config).expect("pipeline");

    for offset in [0, 900, 1800] {
        let output = pipeline
            .update(
                Utc.timestamp_opt(offset, 0).unwrap(),
                dec!(100),
                dec!(100),
                StrategyStatus::Flat,
                None,
            )
            .unwrap();
        assert!(output.entry_signal.is_none());
    }

    let output = pipeline
        .update(
            Utc.timestamp_opt(2700, 0).unwrap(),
            dec!(271.8281828),
            dec!(100),
            StrategyStatus::Flat,
            None,
        )
        .unwrap();
    assert!(output.entry_signal.is_some());
}
