use std::fs;
use std::net::{Ipv4Addr, IpAddr};
use std::path::PathBuf;

use color_eyre::eyre::Result;
use mktemp::Temp;
use node::Config;
use result_tools::*;
use tokio::task::JoinSet;

mod util;

fn runtime_dir() -> PathBuf {
    use std::io::ErrorKind::AlreadyExists;
    let temp = std::env::temp_dir().join("thesis");
    fs::create_dir(&temp)
        .to_ok_if(|e| e.kind() == AlreadyExists)
        .unwrap();
    temp
}

async fn test_run(config: Config) {
    node::run(config).await;
}

#[tokio::test]
async fn main() -> Result<()> {
    let jeager = start_jeager::start_if_not_running(runtime_dir());
    let _ = tokio::spawn(jeager);

    let config = Config {
        id: 0,
        endpoint: IpAddr::V4(Ipv4Addr::LOCALHOST),
        run: 0,
        port: 0,
        cluster_size: 3,
        database: PathBuf::from("changed in loop"),
    };

    util::setup_tracing("0".into(), config.endpoint, 0);
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
        .unwrap();

    Ok(())
}
