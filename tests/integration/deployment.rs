use eth_btc_strategy::config::Config;
use eth_btc_strategy::integration::deployment_ready;

#[test]
fn deployment_ready_requires_valid_config() {
    let config = Config::default();
    assert!(deployment_ready(&config, true));
    assert!(!deployment_ready(&config, false));
}
