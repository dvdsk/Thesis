use crate::server_conn::protocol::{FromRS, ToRs};
use client_protocol::connection;
use discovery::Chart;
use tracing::info;
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

/// heartbeats are send every HB_TIMEOUT/2 seconds, 200ms seems to be the lowest
pub const HB_TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Clone, Debug)]
pub struct HbControl(flume::Sender<Instant>);

impl HbControl {
    pub fn new() -> (Self, flume::Receiver<Instant>) {
        // randevous channel
        let (tx, rx) = flume::bounded(0);
        (Self(tx), rx)
    }
    pub async fn delay(&mut self) {
        let next_hb = Instant::now() + HB_TIMEOUT.mul_f32(1.2);
        self.0.send_async(next_hb).await.unwrap();
    }
}

pub async fn maintain_heartbeat(state: Arc<State>, chart: Chart, mut rx: flume::Receiver<Instant>) {
    loop {
        let term = state.term();
        let change_idx = state.change_idx();
        let heartbeats = chart
            .adresses()
            .into_iter()
            .map(|addr| send_hb(addr, term, change_idx));
        let send_all = futures::future::join_all(heartbeats);
        let next_hb = Instant::now() + HB_TIMEOUT / 2;
        let _ = timeout_at(next_hb, send_all).await;
        sleep_prolongable(next_hb, &mut rx).await;
    }
}

async fn sleep_prolongable(mut next_hb: Instant, rx: &mut flume::Receiver<Instant>) {
    loop {
        next_hb = tokio::select! {
            biased; // always first poll the recv future
            new = rx.recv_async() => {
                info!("hb delayed");
                new.unwrap()
            }
            _ = sleep_until(next_hb) => {
                return 
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
