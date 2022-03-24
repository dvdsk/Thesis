use std::net::TcpListener;
use std::env;
use tracing::Level;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_max_level(Level::TRACE)
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

fn id_from_mac() -> u64 {
    let mac_bytes = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .bytes();

    let mut id = 0u64.to_ne_bytes();
    id[0..6].copy_from_slice(&mac_bytes);
    u64::from_ne_bytes(id)
}
