use clap::Parser;
use rust_decimal_macros::dec;

use eth_btc_strategy::cli::{Cli, Command};
use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::execution::OrderSide;

#[test]
fn cli_parses_runtime_flags() {
    let cli = Cli::try_parse_from([
        "bin",
        "--once",
        "--interval-secs",
        "60",
        "--paper",
        "--disable-funding",
        "--base-url",
        "http://localhost",
        "--api-key",
        "token",
        "--config",
        "config.toml",
        "--state-path",
        "state.sqlite",
    ])
    .unwrap();

    assert!(cli.once);
    assert_eq!(cli.interval_secs, Some(60));
    assert!(cli.paper);
    assert!(cli.disable_funding);
    assert_eq!(cli.base_url.as_deref(), Some("http://localhost"));
    assert_eq!(cli.api_key.as_deref(), Some("token"));
    assert_eq!(cli.config.unwrap().to_str().unwrap(), "config.toml");
    assert_eq!(cli.state_path.unwrap().to_str().unwrap(), "state.sqlite");
    assert!(cli.command.is_none());
}

#[test]
fn cli_parses_backtest_subcommand() {
    let cli = Cli::try_parse_from([
        "bin",
        "backtest",
        "--bars",
        "bars.json",
        "--output-dir",
        "out",
    ])
    .unwrap();

    match cli.command {
        Some(Command::Backtest(args)) => {
            assert_eq!(args.bars.to_str().unwrap(), "bars.json");
            assert_eq!(args.output_dir.unwrap().to_str().unwrap(), "out");
        }
        other => panic!("unexpected command {other:?}"),
    }
}

#[test]
fn cli_parses_download_subcommand() {
    let cli = Cli::try_parse_from([
        "bin",
        "download",
        "--start",
        "2024-01-01T00:00:00Z",
        "--end",
        "2024-01-01T01:00:00Z",
        "--output",
        "bars.json",
    ])
    .unwrap();

    match cli.command {
        Some(Command::Download(args)) => {
            assert_eq!(args.start, "2024-01-01T00:00:00Z");
            assert_eq!(args.end, "2024-01-01T01:00:00Z");
            assert_eq!(args.output.to_str().unwrap(), "bars.json");
        }
        other => panic!("unexpected command {other:?}"),
    }
}

#[test]
fn cli_parses_order_test_subcommand() {
    let cli = Cli::try_parse_from([
        "bin",
        "order-test",
        "--symbol",
        "ETH-PERP",
        "--side",
        "BUY",
        "--qty",
        "0.01",
        "--limit-price",
        "1000",
        "--reduce-only",
    ])
    .unwrap();

    match cli.command {
        Some(Command::OrderTest(args)) => {
            assert_eq!(args.symbol, Symbol::EthPerp);
            assert_eq!(args.side, OrderSide::Buy);
            assert_eq!(args.qty, dec!(0.01));
            assert_eq!(args.limit_price, dec!(1000));
            assert!(args.reduce_only);
        }
        other => panic!("unexpected command {other:?}"),
    }
}
