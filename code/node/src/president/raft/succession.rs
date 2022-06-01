use color_eyre::Result;
use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::{timeout_at, Instant};
use tracing::{debug, instrument, trace, warn};

use crate::Term;

use super::HB_TIMEOUT;
use super::{Chart, State};

use super::state::vote;
use super::{Msg, Reply};

#[instrument(skip_all)]
pub(super) async fn president_died(state: &State) {
    let heartbeat = state.heartbeat();

    loop {
        let hb_deadline = Instant::now() + HB_TIMEOUT;
        match timeout_at(hb_deadline, heartbeat.notified()).await {
            Err(_) => {
                let election_office = state.election_office.lock().await;
                let data = election_office.data();
                warn!("heartbeat for term {} timed out", data.term());
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

#[instrument(level = "trace", ret)]
async fn request_vote(
    addr: SocketAddr,
    vote_req: vote::RequestVote,
    voter_id: instance_chart::Id,
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
/// with a timeout. This returns as soon as a majority is reached
#[instrument(skip_all, fields(id = chart.our_id(), term))]
pub(super) async fn run_for_office(
    state: &State,
    chart: &Chart,
    cluster_size: u16,
    term: Term,
) -> ElectionResult {

    let meta = state.last_log_meta();
    let campaign = vote::RequestVote {
        term,
        candidate_id: chart.our_id(),
        last_log_term: meta.term,
        last_log_idx: meta.idx,
    };

    let mut requests: JoinSet<_> = chart
        .nth_addr_vec::<0>()
        .into_iter() 
        .map(|(voter_id, addr)| request_vote(addr, campaign.clone(), voter_id))
        .fold(JoinSet::new(), |mut set, fut| {
            set.build_task().name("request_vote").spawn(fut);
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
