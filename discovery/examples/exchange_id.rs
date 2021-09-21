use mac_address::get_mac_address;
use std::env;
use tracing::{info, Level};

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

    let id = id_from_mac();
    let (sock, chart) = discovery::setup(id).await;
    let discover = discovery::cluster(&sock, &chart, cluster_size);
    let maintain = discovery::maintain(&sock, &chart);

    futures::join!(discover, maintain);
}

fn id_from_mac() -> u64 {
    let mac_bytes = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .bytes();

    let mut id = 0u64.to_ne_bytes();
    id[0..6].copy_from_slice(&mac_bytes);
    u64::from_ne_bytes(id)
}
