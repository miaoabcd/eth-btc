use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "eth-btc-strategy",
    about = "ETH/BTC mean reversion strategy runner"
)]
pub struct Cli {
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub state_path: Option<PathBuf>,
    #[arg(long, default_value = "https://api.hyperliquid.xyz")]
    pub base_url: String,
    #[arg(long, value_name = "TOKEN")]
    pub api_key: Option<String>,
    #[arg(long, value_name = "HEX")]
    pub private_key: Option<String>,
    #[arg(long, value_name = "ADDRESS")]
    pub vault_address: Option<String>,
    #[arg(long, default_value_t = 900)]
    pub interval_secs: u64,
    #[arg(long)]
    pub once: bool,
    #[arg(long)]
    pub paper: bool,
    #[arg(long)]
    pub disable_funding: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Backtest(BacktestArgs),
}

#[derive(Debug, Args)]
pub struct BacktestArgs {
    #[arg(long, value_name = "PATH")]
    pub bars: PathBuf,
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
}
