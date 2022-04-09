use discovery::ChartBuilder;
use multicast_discovery as discovery;
use std::env;
use std::net::TcpListener;

use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let filter = EnvFilter::from_default_env();
    // let console_layer = console_subscriber::spawn();
    let fmt_layer = fmt::layer().with_target(false).pretty();

    tracing_subscriber::registry()
        // .with(console_layer)
        .with(filter)
        .with(fmt_layer)
        .init();

    let mut args = env::args().skip(1);
    let cluster_size: u16 = args
        .next()
        .expect("have to pass at least two args")
        .parse()
        .expect("pass cluster size as u16");
    let id = args
        .next()
        .expect("pass the id as second argument")
        .parse()
        .expect("pass id as u64");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 0);

    let chart = ChartBuilder::new().with_id(id).build().unwrap();
    let discover = discovery::found_majority(chart.clone(), cluster_size);
    let discover = tokio::spawn(discover);
    let maintain = discovery::maintain(chart.clone());
    let maintain = tokio::spawn(maintain);

    let (e1, e2) = futures::join!(discover, maintain);
    e1.unwrap();
    e2.unwrap();
}
