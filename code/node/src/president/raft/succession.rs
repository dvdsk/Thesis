use color_eyre::Result;
use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::{timeout_at, Instant};
use tracing::{debug, instrument, trace, warn};

use super::HB_TIMEOUT;
use super::{Chart, State};

use super::state::vote;
use super::{Msg, Reply};

#[instrument(skip(state))]
pub(super) async fn president_died(state: &State) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();
    let heartbeat = state.heartbeat();

    loop {
        let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
        let hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
        match timeout_at(hb_deadline, heartbeat.notified()).await {
            Err(_) => {
                let election_office = state.election_office.lock().unwrap();
                let data = election_office.data();
                warn!("heartbeat timed out, term is now: {}", data.term());
                return;
            }
            Ok(_) => {
                trace!(
                    "hb timeout in {} ms",
                    hb_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                );
            }
        }
    }
}

#[instrument(level = "debug", ret)]
async fn request_vote(
    addr: SocketAddr,
    vote_req: vote::RequestVote,
    id: instance_chart::Id,
) -> Result<()> {
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

pub enum ElectionResult {
    Won,
    Lost,
}

/// only returns when this node has been elected
/// election timeout is implemented by selecting on this
/// with a timeout
#[instrument(skip_all, fields(id = chart.our_id()))]
pub(super) async fn run_for_office(
    chart: &Chart,
    cluster_size: u16,
    campaign: vote::RequestVote,
) -> ElectionResult {
    let mut requests: JoinSet<_> = chart
        .nth_addr_vec::<0>()
        .into_iter()
        .map(|(_, addr)| addr)
        .map(|addr| request_vote(addr, campaign.clone(), chart.our_id()))
        .fold(JoinSet::new(), |mut set, fut| {
            set.spawn(fut);
            set
        });

    debug!("waiting for vote to complete");
    let majority = (f32::from(cluster_size) * 0.5 + 0.001).ceil() as usize;
    let mut votes = 1; // vote for ourself

    for to_count in (0..cluster_size).rev() {
        match requests
            .join_one()
            .await
            .expect("request vote task panicked")
        {
            Some(Ok(_)) => votes += 1,
            Some(Err(_)) => (),
            None => break, // no more req_vote tasks left
        }

        if to_count + votes < majority as u16 {
            break;
        }

        if votes >= majority as u16 {
            debug!("Got majority ({majority}) with {votes} votes");
            return ElectionResult::Won;
        }
    }
    ElectionResult::Lost
}
