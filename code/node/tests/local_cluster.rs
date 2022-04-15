use std::fs;
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
    fs::create_dir(temp)
        .to_ok_if(|e| e.kind() == AlreadyExists)
        .unwrap();
    temp
}

#[tokio::test]
async fn main() -> Result<()> {
    start_jeager::start_if_not_running(&runtime_dir());

    let config = Config {
        id: 0,
        endpoint: "127.0.0.1:8888".to_owned(),
        run: 0,
        port: 0,
        cluster_size: 3,
        database: PathBuf::from("changed in loop"),
    };

    let temp_dir = Temp::new_dir().unwrap();
    (0..config.cluster_size)
        .map(|i| Config {
            id: i.into(),
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
