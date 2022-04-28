use color_eyre::Result;
use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::{timeout_at, Instant};
use tracing::{info, warn};

use crate::president::Chart;

use super::{state, Msg, Reply};

const HB_TIMEOUT: Duration = Duration::from_millis(100);

pub(super) async fn president_died(heartbeat: &Notify) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();

    loop {
        let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
        let hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
        match timeout_at(hb_deadline, heartbeat.notified()).await {
            Err(elapsed) => {
                warn!("heartbeat timed out, elapsed without hb: {:?}", elapsed);
                return;
            }
            Ok(_) => {
                info!(
                    "hb timeout in {} ms",
                    hb_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                );
            }
        }
    }
}

async fn request_vote(addr: SocketAddr, vote_req: state::RequestVote) -> Result<()> {
    let stream = TcpStream::connect(addr).await?;
    let mut stream: connection::MsgStream<Reply, Msg> = connection::wrap(stream);
    stream.send(Msg::RequestVote(vote_req)).await?;
    loop {
        match stream.try_next().await? {
            None => continue,
            Some(_reply) => return Ok(()),
        }
    }
}

/// only returns when this node has been elected
/// election timeout is implemented by selecting on this
/// with a timeout
pub(super) async fn run_for_office(chart: &Chart, cluster_size: u16, campaign: state::RequestVote) {
    let mut requests: JoinSet<_> = chart
        .nth_addr_vec::<0>()
        .into_iter()
        .map(|addr| request_vote(addr, campaign.clone()))
        .fold(JoinSet::new(), |mut set, fut| {
            set.spawn(fut);
            set
        });

    let majority = (f32::from(cluster_size) * 0.5).ceil() as usize;
    let mut votes = 0;
    info!(votes, majority);
    while let Some(res) = requests
        .join_one()
        .await
        .expect("request vote task panicked")
    {
        match res {
            Ok(_) => votes += 1,
            Err(_) => continue,
        }
        if dbg!(votes) >= dbg!(majority) {
            return;
        }
    }
}
