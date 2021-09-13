use std::net::SocketAddr;

use client_protocol::connection;
use tokio::net::TcpStream;
use tokio::time::Duration;
use tokio::time::Instant;
use futures::FutureExt;

use tokio::sync::mpsc;
use tokio::time::timeout_at;
use tracing::{info, info_span};

use crate::server_conn::protocol::ElectionMsg;
use crate::server_conn::protocol::ToRs;
use crate::server_conn::protocol::ToWs;

type Winner = SocketAddr;
#[derive(Debug)]
pub enum ElectionResult {
    WeWon,
    WeLost(Winner),
    Stale,
}

async fn send_hb(addr: SocketAddr, term: u64) -> Option<()> {
    use futures::SinkExt;
    type RsStream = connection::MsgStream<ToWs, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::Election(ElectionMsg::RequestVote(term)))
        .await
        .ok()?;
    Some(())
}

pub async fn maintain_heartbeat(state: &'_ State<'_>) {
    loop {
        let heartbeats = state
            .chart
            .map
            .iter()
            .map(|m| m.value().clone())
            .map(|addr| send_hb(addr, state.term));

        // TODO add timeout
        futures::future::join_all(heartbeats).await;
    }
}


/// future that returns if no heartbeat has been recieved for
async fn monitor_heartbeat(state: &'_ mut State<'_>, rx: &mut mpsc::Receiver<(SocketAddr, ElectionMsg)>) {
    const HB_TIMEOUT: Duration = Duration::from_secs(2);
    let mut hb_deadline = Instant::now() + HB_TIMEOUT;
    loop {
        match timeout_at(hb_deadline, rx.recv()).await {
            Ok(Some((_, ElectionMsg::HeartBeat(term)))) => {
                if term >= state.term {
                    state.term = term;
                    hb_deadline += HB_TIMEOUT;
                }
            }
            Ok(Some(msg)) => todo!("unhandled electionmsg: {:?}", msg),
            Ok(None) => panic!("should never get an empty msg"),
            Err(_) => break,
        }
    }
}

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

async fn request_vote(addr: SocketAddr, term: u64) -> Option<()> {
    use futures::{SinkExt, TryStreamExt};
    type RsStream = connection::MsgStream<ToWs, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::Election(ElectionMsg::RequestVote(term)))
        .await
        .ok()?;

    match stream.try_next().await {
        Ok(Some(ToWs::Election(ElectionMsg::VotedForYou(t)))) if t == term => Some(()),
        _ => None,
    }
}

async fn request_and_count_votes(state: &State<'_>) -> ElectionResult {
    let requests = state
        .chart
        .map
        .iter()
        .map(|m| m.value().clone())
        .map(|addr| request_vote(addr, state.term));

    // TODO add timeout
    let votes: usize = futures::future::join_all(requests)
        .await
        .iter()
        .map(Option::is_some)
        .map(|b| b as usize)
        .sum();

    match state.is_majority(votes) {
        true => ElectionResult::WeWon,
        false => ElectionResult::Stale,
    }
}

async fn monitor_elections(rx: &mut mpsc::Receiver<(SocketAddr, ElectionMsg)>, our_term: u64) -> ElectionResult {
    loop {
        if let Some((source, ElectionMsg::HeartBeat(term))) = rx.recv().await {
            if term > our_term {
                return ElectionResult::WeLost(source);
            }
        }
    }
}

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

pub async fn cycle(mut rx: mpsc::Receiver<(SocketAddr,ElectionMsg)>, state: &'_ mut State<'_>) {

    loop {
        monitor_heartbeat(state, &mut rx).await;

        // TODO get cmd server listener in here
        if let ElectionResult::WeWon = host_election(&mut rx, state).await {
            info!("won election");
            break;
        }
    }
}
