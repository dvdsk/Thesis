use node::{Config, run};
use clap::Parser;

mod util;

#[tokio::main]
async fn main() {
    let conf = Config::parse();

    util::setup_tracing(conf.id.to_string(), conf.endpoint, conf.run);
    util::setup_errors();

    run(conf).await
}
