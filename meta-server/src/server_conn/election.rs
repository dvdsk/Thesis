use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};

use client_protocol::connection;
use futures::FutureExt;
use tokio::net::TcpStream;
use tokio::time;
use tokio::time::timeout;
use tokio::time::Duration;
use tokio::time::Instant;

use tokio::sync::mpsc;
use tokio::time::timeout_at;
use tracing::info;

use crate::server_conn::protocol::ElectionMsg;
use crate::server_conn::protocol::ToRs;
use crate::server_conn::protocol::FromRS;

type Winner = SocketAddr;
#[derive(Debug)]
pub enum ElectionResult {
    WeWon,
    WeLost(Winner),
    Stale,
}

async fn send_hb(addr: SocketAddr, term: u64) -> Option<()> {
    use futures::SinkExt;
    type RsStream = connection::MsgStream<FromRS, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::Election(ElectionMsg::RequestVote(term)))
        .await
        .ok()?;
    Some(())
}

const HB_TIMEOUT: Duration = Duration::from_secs(2);
pub async fn maintain_heartbeat(state: &'_ State<'_>) {
    loop {
        let heartbeats = state
            .chart
            .map
            .iter()
            .map(|m| m.value().clone())
            .map(|addr| send_hb(addr, state.term));

        // TODO add timeout
        let send_all = futures::future::join_all(heartbeats);
        let _ = timeout(Duration::from_millis(500), send_all).await;
        time::sleep(HB_TIMEOUT / 2).await;
    }
}

/// future that returns if no heartbeat has been recieved for
async fn monitor_heartbeat(
    state: &'_ mut State<'_>,
    rx: &mut mpsc::Receiver<(SocketAddr, ElectionMsg)>,
) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();

    let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
    let mut hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
    loop {
        match timeout_at(hb_deadline, rx.recv()).await {
            Ok(Some((_, ElectionMsg::HeartBeat(term)))) => {
                if term >= state.term {
                    state.term = term;
                    let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
                    hb_deadline += HB_TIMEOUT + random_dur;
                }
            }
            Ok(Some(msg)) => todo!("unhandled electionmsg: {:?}", msg),
            Ok(None) => panic!("should never get an empty msg"),
            Err(_) => break,
        }
    }
}

#[derive(Debug)]
pub struct State<'a> {
    term: u64,
    cluster_size: u16,
    pub chart: &'a discovery::Chart,
}

impl<'a> State<'a> {
    pub fn new(cluster_size: u16, chart: &'a discovery::Chart) -> Self {
        Self {
            term: 0,
            cluster_size,
            chart,
        }
    }
    fn is_majority(&self, votes: usize) -> bool {
        let cluster_majority = (self.cluster_size as f32 * 0.5).ceil() as usize;
        votes > cluster_majority
    }
}

#[tracing::instrument]
async fn request_and_count(addr: SocketAddr, term: u64, count: &AtomicU16) -> Option<()> {
    use futures::{SinkExt, TryStreamExt};
    type RsStream = connection::MsgStream<FromRS, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::Election(ElectionMsg::RequestVote(term)))
        .await
        .ok()?;

    if let Ok(Some(FromRS::Election(ElectionMsg::VotedForYou(t)))) = stream.try_next().await {
        if t == term {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }
    Some(())
}

// TODO rewrite, as soon as we have enough votes continue and declear win
#[tracing::instrument]
async fn request_and_count_votes(state: &State<'_>) -> ElectionResult {
    let count = AtomicU16::new(1); // we vote for ourself
    let requests = state
        .chart
        .map
        .iter()
        .map(|m| m.value().clone())
        .map(|addr| request_and_count(addr, state.term, &count));

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

#[tracing::instrument]
async fn monitor_elections(
    rx: &mut mpsc::Receiver<(SocketAddr, ElectionMsg)>,
    our_term: u64,
) -> ElectionResult {
    loop {
        if let Some((source, ElectionMsg::HeartBeat(term))) = rx.recv().await {
            if term > our_term {
                return ElectionResult::WeLost(source);
            }
        }
    }
}

#[tracing::instrument]
async fn host_election(
    rx: &mut mpsc::Receiver<(SocketAddr, ElectionMsg)>,
    state: &'_ mut State<'_>,
) -> ElectionResult {
    info!("hosting leader election");
    state.term += 1;
    futures::select! {
        res = request_and_count_votes(&state).fuse() => res,
        res = monitor_elections(rx, state.term).fuse() => res,
    }
}

#[tracing::instrument]
pub async fn cycle(mut rx: mpsc::Receiver<(SocketAddr, ElectionMsg)>, state: &'_ mut State<'_>) {
    loop {
        monitor_heartbeat(state, &mut rx).await;

        // TODO get cmd server listener in here
        if let ElectionResult::WeWon = host_election(&mut rx, state).await {
            info!("won election");
            break;
        }
    }
}
