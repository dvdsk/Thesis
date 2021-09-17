use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};

use client_protocol::connection;
use discovery::Chart;
use futures::FutureExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio::time::Duration;
use tokio::time::Instant;

use tokio::time::timeout_at;
use tracing::info;

use super::{State, HB_TIMEOUT};
use crate::server_conn::protocol::FromRS;
use crate::server_conn::protocol::ToRs;

#[derive(Debug)]
pub enum ElectionResult {
    WeWon,
    WeLost,
    Stale,
}

/// future that returns if no heartbeat has been recieved for
async fn monitor_heartbeat(state: &State) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();

    let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
    let mut hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
    loop {
        match timeout_at(hb_deadline, state.got_valid_hb.notified()).await {
            Err(_timeout) => return,
            Ok(_) => {
                let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
                hb_deadline += HB_TIMEOUT + random_dur;
            }
        }
    }
}

async fn request_and_count(
    addr: SocketAddr,
    term: u64,
    change_idx: u64,
    count: &AtomicU16,
) -> Option<()> {
    use futures::{SinkExt, TryStreamExt};
    type RsStream = connection::MsgStream<FromRS, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::RequestVote(term, change_idx))
        .await
        .ok()?;

    if let Ok(Some(FromRS::VotedForYou(t))) = stream.try_next().await {
        if t == term {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }
    Some(())
}

// TODO rewrite, as soon as we have enough votes continue and declear win
async fn request_and_count_votes(state: &State, chart: &Chart) -> ElectionResult {
    let count = AtomicU16::new(1); // we vote for ourself
    let term = state.term();
    let requests = chart
        .map
        .iter()
        .map(|m| m.value().clone())
        .map(|addr| request_and_count(addr, term, state.change_idx(), &count));

    let geather_votes = futures::future::join_all(requests);
    let _ = timeout(Duration::from_millis(500), geather_votes).await;

    match state.is_majority(count.into_inner() as usize) {
        true => {
            info!("won election");
            ElectionResult::WeWon
        }
        false => {
            info!("stale election");
            ElectionResult::Stale
        }
    }
}

async fn host_election(state: &State, chart: &Chart) -> ElectionResult {
    info!("hosting leader election");
    state.increase_term();
    state.set_candidate();
    futures::select! {
        res = request_and_count_votes(&state, chart).fuse() => res,
        _ = state.got_valid_hb.notified().fuse() => {
            state.set_follower();
            ElectionResult::WeLost
        },
    }
}

pub async fn cycle(state: &State, chart: &Chart) {
    loop {
        monitor_heartbeat(state).await;

        // TODO get cmd server listener in here
        if let ElectionResult::WeWon = host_election(state, chart).await {
            info!("won election");
            break;
        }
    }
}
