use node::{Config, run};
use node::util::prefix::*;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Config::parse();
    run(args).await
}
