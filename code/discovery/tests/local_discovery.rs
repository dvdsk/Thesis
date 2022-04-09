use std::net::UdpSocket;
use discovery::ChartBuilder;
use multicast_discovery as discovery;
use tracing::{debug, info};

fn setup_tracing() {
    use tracing_subscriber::{filter, prelude::*};

    let filter_modules = filter::filter_fn(|metadata| {
        if let Some(module) = metadata.module_path() {
            !module.contains("tarp")
        } else {
            true
        }
    });
    let fmt = tracing_subscriber::fmt::layer()
        .pretty()
        .with_test_writer();

    let _ignore_err = tracing_subscriber::registry()
        .with(fmt)
        .with(filter::LevelFilter::INFO)
        .with(filter_modules)
        .try_init();
}

#[tokio::test(flavor = "current_thread")]
async fn local_discovery() {
    setup_tracing();

    let cluster_size: u16 = 5;
    let handles: Vec<_> = (0..cluster_size).map(|id| {
        tokio::spawn(node(id.into(), cluster_size))
    }).collect();

    for h in handles {
        h.await.unwrap();
    }
}

async fn node(id: u64, cluster_size: u16) {
    let reserv_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserv_socket.local_addr().unwrap().port();
    assert_ne!(port, 0);

    let chart = ChartBuilder::new().with_id(id).build().unwrap();
    let maintain = discovery::maintain(chart.clone());
    let _ = tokio::spawn(maintain);

    discovery::found_everyone(chart.clone(), cluster_size).await;
    debug!("discovery complete: {chart:?}");
}
