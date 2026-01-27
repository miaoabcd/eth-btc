use std::path::PathBuf;

use clap::Parser;

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
    #[arg(long, default_value = "https://api.variational.io")]
    pub base_url: String,
    #[arg(long, value_name = "TOKEN")]
    pub api_key: Option<String>,
    #[arg(long, default_value_t = 900)]
    pub interval_secs: u64,
    #[arg(long)]
    pub once: bool,
    #[arg(long)]
    pub paper: bool,
    #[arg(long)]
    pub disable_funding: bool,
}
