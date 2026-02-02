use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use rust_decimal::Decimal;

use crate::config::Symbol;
use crate::execution::OrderSide;

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
    #[arg(long, value_name = "URL")]
    pub base_url: Option<String>,
    #[arg(long, value_name = "TOKEN")]
    pub api_key: Option<String>,
    #[arg(long, value_name = "HEX")]
    pub private_key: Option<String>,
    #[arg(long, value_name = "ADDRESS")]
    pub vault_address: Option<String>,
    #[arg(long, value_name = "SECONDS")]
    pub interval_secs: Option<u64>,
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
    Download(DownloadArgs),
    OrderTest(OrderTestArgs),
}

#[derive(Debug, Args)]
pub struct BacktestArgs {
    #[arg(long, value_name = "PATH")]
    pub bars: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub db: Option<PathBuf>,
    #[arg(long, value_name = "RFC3339")]
    pub start: Option<String>,
    #[arg(long, value_name = "RFC3339")]
    pub end: Option<String>,
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    #[arg(long, value_name = "RFC3339")]
    pub start: String,
    #[arg(long, value_name = "RFC3339")]
    pub end: String,
    #[arg(long, value_name = "PATH")]
    pub output: PathBuf,
}

#[derive(Debug, Args)]
pub struct OrderTestArgs {
    #[arg(long, value_name = "SYMBOL")]
    pub symbol: Symbol,
    #[arg(long, value_name = "SIDE")]
    pub side: OrderSide,
    #[arg(long, value_name = "QTY")]
    pub qty: Decimal,
    #[arg(long, value_name = "PRICE")]
    pub limit_price: Decimal,
    #[arg(long)]
    pub reduce_only: bool,
    #[arg(long)]
    pub dry_run: bool,
}
