use std::path::PathBuf;
use std::str::FromStr;

use alloy_signer_local::PrivateKeySigner;
use anyhow::{Context, anyhow};
use clap::Parser;
use eth_btc_strategy::account::{AccountBalanceSource, HyperliquidAccountSource};
use eth_btc_strategy::config::load_config;

#[derive(Debug, Parser)]
#[command(name = "check-balance")]
struct Args {
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    base_url: Option<String>,
    #[arg(long, value_name = "HEX")]
    private_key: Option<String>,
    #[arg(long, value_name = "ADDRESS")]
    wallet_address: Option<String>,
    #[arg(long, value_name = "ADDRESS")]
    vault_address: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = load_config(args.config.as_deref()).context("load config")?;

    let base_url = args.base_url.unwrap_or_else(|| config.runtime.base_url.clone());
    let private_key = args
        .private_key
        .or_else(|| config.auth.private_key.clone())
        .ok_or_else(|| anyhow!("missing Hyperliquid private key"))?;
    let wallet_address = args.wallet_address.or_else(|| config.auth.wallet_address.clone());
    let vault_address = args.vault_address.or_else(|| config.auth.vault_address.clone());

    let signer = PrivateKeySigner::from_str(private_key.trim_start_matches("0x"))
        .map_err(|err| anyhow!("invalid private key: {err}"))?;
    let signer_wallet = signer.address().to_string();
    let account_wallet = wallet_address
        .clone()
        .or_else(|| vault_address.clone())
        .unwrap_or_else(|| signer_wallet.clone());
    let execution_wallet = vault_address
        .clone()
        .unwrap_or_else(|| signer_wallet.clone());

    let account_source = HyperliquidAccountSource::new(base_url, account_wallet.clone());
    let balance = account_source
        .fetch_available_balance()
        .await
        .map_err(|err| anyhow!("fetch available balance: {err}"))?;

    println!(
        "{}",
        serde_json::json!({
            "signer_wallet": signer_wallet,
            "wallet_address": wallet_address,
            "vault_address": vault_address,
            "account_wallet": account_wallet,
            "execution_wallet": execution_wallet,
            "available_balance": balance,
        })
    );
    Ok(())
}
