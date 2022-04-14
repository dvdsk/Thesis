use color_eyre::eyre::Result;
use node::Config;
use tokio::task::JoinSet;

mod util;

#[tokio::test]
async fn main() -> Result<()> {
    start_jeager::start_if_not_running();

    let config = Config {
        id: 0,
        endpoint: "127.0.0.1:8888".to_owned(),
        run: 0,
        port: 0,
        cluster_size: 3,
    };

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
