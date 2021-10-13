use crate::server_conn::protocol::{FromRS, ToRs};
use client_protocol::connection;
use discovery::Chart;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep_until, timeout_at};
use tokio::net::TcpStream;

pub mod election;
mod replicate;
pub use replicate::update;
mod state;
pub use state::State;

pub const HB_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Clone, Debug)]
pub struct HbControl(flume::Sender<Instant>);

impl HbControl {
    pub fn new() -> (Self, flume::Receiver<Instant>) {
        // randevous channel
        let (tx, rx) = flume::bounded(0);
        (Self(tx), rx)
    }
    pub async fn delay(&mut self) {
        let next_hb = Instant::now() + HB_TIMEOUT / 2;
        self.0.send_async(next_hb).await.unwrap();
    }
}

pub async fn maintain_heartbeat(state: Arc<State>, chart: Chart, mut rx: flume::Receiver<Instant>) {
    let mut next_hb = Instant::now() + HB_TIMEOUT / 2;
    loop {
        let term = state.increase_term();
        let heartbeats = chart
            .adresses()
            .into_iter()
            .map(|addr| send_hb(addr, term, state.change_idx()));

        let send_all = futures::future::join_all(heartbeats);
        let _ = timeout_at(next_hb, send_all).await;
        next_hb = sleep_prolongable(next_hb, &mut rx).await;
    }
}

async fn sleep_prolongable(mut next_hb: Instant, rx: &mut flume::Receiver<Instant>) -> Instant {
    loop {
        next_hb = tokio::select! {
            biased; // always first poll the recv future
            new = rx.recv_async() => new.unwrap(),
            _ = sleep_until(next_hb) => {
                return next_hb + HB_TIMEOUT / 2; 
            }
        };
    }
}

#[tracing::instrument(level = "debug")]
async fn send_hb(addr: SocketAddr, term: u64, change_idx: u64) -> Option<()> {
    use futures::SinkExt;
    type RsStream = connection::MsgStream<FromRS, ToRs>;

    let socket = TcpStream::connect(addr).await.ok()?;
    let mut stream: RsStream = connection::wrap(socket);
    stream.send(ToRs::HeartBeat(term, change_idx)).await.ok()?;
    Some(())
}
