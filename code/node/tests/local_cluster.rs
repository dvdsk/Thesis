use std::net::{Ipv4Addr, IpAddr};
use std::path::PathBuf;

use color_eyre::eyre::Result;
use mktemp::Temp;
use node::Config;
use node::util::runtime_dir;
use tokio::task::JoinSet;

mod util;

#[tokio::test]
async fn main() -> Result<()> {
    util::setup_errors();
    start_jeager::start_if_not_running(runtime_dir()).await;

    let config = Config {
        id: 0,
        endpoint: IpAddr::V4(Ipv4Addr::LOCALHOST),
        run: util::run_number(&runtime_dir()),
        local_instances: true,
        pres_port: None,
        node_port: None,
        req_port: None,
        cluster_size: 3,
        database: PathBuf::from("changed in loop"),
    };

    util::setup_tracing("test".into(), config.endpoint, config.run);
    let temp_dir = Temp::new_dir().unwrap();
    (0..config.cluster_size)
        .map(|i| Config {
            id: i.into(),
            database: temp_dir.join(format!("{i}.db")),
            ..config.clone()
        })
        .map(|config| node::run(config))
        .fold(JoinSet::new(), |mut set, fut| {
            set.spawn(fut);
            set
        })
        .join_one()
        .await
        .unwrap()
        .expect("joinset has content")
}
