use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};

use client_protocol::connection;
use discovery::Chart;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::{self, timeout_at, Duration, Instant};

use tracing::{info, trace, warn};

use super::{State, HB_TIMEOUT};
use crate::server_conn::protocol::{FromRS, ToRs};

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
            Err(_timeout) => {
                warn!("heartbeat timed out");
                return;
            }
            Ok(_) => {
                let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
                hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
                trace!("hb timeout in {} ms", hb_deadline.saturating_duration_since(Instant::now()).as_millis()); 
            }
        }
    }
}

#[derive(Default, Debug)]
struct VoteCount {
    majority: u16,
    count: AtomicU16,
    notify: Notify,
}

impl VoteCount {
    pub fn new(cluster_size: u16) -> Self {
        Self {
            majority: (cluster_size as f32 * 0.5).ceil() as u16,
            count: AtomicU16::new(1), // we vote for ourself
            notify: Notify::new(),
        }
    }
    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.notify.notify_one();
    }
    pub fn is_majority(&self) -> bool {
        self.count.load(Ordering::Relaxed) >= self.majority
    }
    pub async fn await_majority(&self) {
        while !self.is_majority() {
            self.notify.notified().await;
        }
    }
}

#[tracing::instrument(level = "debug")]
async fn request_and_register(
    addr: SocketAddr,
    term: u64,
    change_idx: u64,
    our_id: u64,
    count: &VoteCount,
) -> Option<()> {
    use futures::{SinkExt, TryStreamExt};
    type RsStream = connection::MsgStream<FromRS, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream
        .send(ToRs::RequestVote(term, change_idx, our_id))
        .await
        .ok()?;

    if let Ok(Some(FromRS::VotedForYou(t))) = stream.try_next().await {
        if t == term {
            count.increment();
        }
    }
    Some(())
}

#[tracing::instrument(level = "debug", skip(state, chart))]
async fn request_and_count_votes(port: u16, state: &State, chart: &Chart) -> ElectionResult {
    let count = VoteCount::new(state.config.cluster_size);
    let term = state.term();
    let our_id = chart.our_id();
    let requests = chart
        .adresses()
        .into_iter()
        .map(|addr| request_and_register(addr, term, state.change_idx(), our_id, &count));

    let geather_votes = futures::future::join_all(requests);
    let timeout = time::sleep(HB_TIMEOUT.mul_f32(1.1));
    let majority = count.await_majority();
    println!("select starts");
    let res = tokio::select! {
        _ = timeout => ElectionResult::Stale,
        _ = majority => ElectionResult::WeWon,
        _ = geather_votes => match count.is_majority() {
            true => ElectionResult::WeWon,
            false => ElectionResult::Stale,
        }
    };
    println!("select completes");
    return res;
}

#[tracing::instrument(level = "debug", skip(state,chart))]
async fn host_election(port: u16, state: &State, chart: &Chart) -> ElectionResult {
    info!("hosting leader election");
    state.set_candidate();
    state.increase_term();
    tokio::select! {
        res = request_and_count_votes(port, &state, chart) => res,
        _ = state.got_valid_hb.notified() => {
            state.set_follower();
            ElectionResult::WeLost
        },
    }
}

pub async fn cycle(port: u16, state: &State, chart: &Chart) {
    loop {
        monitor_heartbeat(state).await;

        match host_election(port, state, chart).await {
            ElectionResult::Stale => info!("stale election"),
            ElectionResult::WeLost => info!("lost election"),
            ElectionResult::WeWon => {
                info!("won election");
                break;
            }
        }
    }
}
