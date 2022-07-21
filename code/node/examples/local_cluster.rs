use std::net::{Ipv4Addr, IpAddr};
use std::path::PathBuf;

use mktemp::Temp;
use node::Config;
use node::util::runtime_dir;
use tokio::task::JoinSet;
use tracing::Instrument;

use node::util;

#[tokio::main]
async fn main() {
    util::setup_errors();
    start_jeager::start_if_not_running(runtime_dir()).await;

    let config = Config {
        id: 0,
        endpoint: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        run: util::run_number(&runtime_dir()),
        local_instances: true,
        pres_port: None,
        minister_port: None,
        client_port: None,
        cluster_size: 4,
        database: PathBuf::from("changed in loop"),
        partitions: Vec::new(),
    };

    util::setup_tracing("test".into(), config.endpoint, config.run);
    let temp_dir = Temp::new_dir().unwrap();
    (0..config.cluster_size)
        .map(|i| Config {
            id: i.into(),
            database: temp_dir.join(format!("{i}.db")),
            ..config.clone()
        })
        .map(node::run)
        .map(|fut| fut.instrument(tracing::info_span!("test")))
        .fold(JoinSet::new(), |mut set, fut| {
            set.build_task().name("node").spawn(fut);
            set
        })
        .join_one()
        .await
        .unwrap()
        .expect("joinset has content")
}
