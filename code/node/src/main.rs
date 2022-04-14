use node::{Config, run};
use clap::Parser;
use color_eyre::eyre::Result;

mod util;

#[tokio::main]
async fn main() -> Result<()> {
    let conf = Config::parse();

    util::setup_tracing(conf.id.to_string(), &conf.endpoint, conf.run);
    util::setup_errors();

    run(conf).await
}
