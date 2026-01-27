use clap::Parser;

use eth_btc_strategy::cli::Cli;

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
    assert_eq!(cli.interval_secs, 60);
    assert!(cli.paper);
    assert!(cli.disable_funding);
    assert_eq!(cli.base_url, "http://localhost");
    assert_eq!(cli.api_key.as_deref(), Some("token"));
    assert_eq!(cli.config.unwrap().to_str().unwrap(), "config.toml");
    assert_eq!(cli.state_path.unwrap().to_str().unwrap(), "state.sqlite");
}
