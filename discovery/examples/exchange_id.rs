use std::env;
use tracing::{info, Level};
use mac_address::get_mac_address;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_max_level(Level::TRACE)
        .init();

    let cluster_size: u16 = env::args()
        .skip(1)
        .next()
        .expect("have to pass at least one arg")
        .parse()
        .expect("pass id as u16");

    let id = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .to_string();

    let (sock, chart) = discovery::setup(id).await;
    let discover = discovery::cluster(sock.clone(), &chart, cluster_size);
    let maintain = discovery::maintain(sock, &chart);

    futures::join!(discover, maintain);
}
