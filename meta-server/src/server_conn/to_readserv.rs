use client_protocol::connection;
use discovery::Chart;
use futures::future::join_all;
use futures::{SinkExt, TryStreamExt};
use tracing::{trace, warn};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::consensus::{State, HB_TIMEOUT};
use crate::server_conn::protocol::{Change, FromRS, ToRs};

type RsStream = connection::MsgStream<FromRS, ToRs>;

#[derive(Clone, Debug)]
pub struct ReadServers(Arc<Mutex<Inner>>);

impl ReadServers {
    pub fn new(chart: Chart, port: u16) -> Self {
        Self(Arc::new(Mutex::new(Inner::new(chart, port))))
    }
    #[tracing::instrument]
    pub async fn publish(&self, state: &State, change: Change) -> PubResult {
        let msg = ToRs::DirectoryChange(state.term(), state.increase_change_idx(), change);
        let reached = self.0.lock().await.send_to_readservers(msg).await as u16;
        tracing::info!("reached {} servers", reached);

        let majority = state.config.cluster_size / 2;
        if reached == state.config.cluster_size {
            PubResult::ReachedAll
        } else if reached > majority {
            PubResult::ReachedMajority
        } else {
            PubResult::ReachedMinority
        }
    }
}

#[derive(Debug)]
struct Inner {
    port: u16,
    conns: HashMap<IpAddr, RsStream>,
    chart: Chart,
}

pub enum PubResult {
    ReachedAll,
    ReachedMajority,
    ReachedMinority,
}

#[tracing::instrument]
async fn send_confirm(msg: ToRs, conn: &mut RsStream) -> Option<()> {
    conn.send(msg).await.ok()?;
    match conn.try_next().await.ok()? {
        Some(FromRS::Awk) => Some(()),
        None => {warn!("got empty response"); None}
        Some(other) => {warn!("got invalid response from peer: {:?}", other); None}
    }
}

#[tracing::instrument]
async fn send(msg: ToRs, ip: IpAddr, conn: &mut RsStream) -> Result<IpAddr, IpAddr> {
    match timeout(HB_TIMEOUT, send_confirm(msg, conn)).await {
        Err(_) => Err(ip),
        Ok(None) => Err(ip),
        Ok(Some(_)) => Ok(ip),
    }
}

#[tracing::instrument]
async fn conn_and_send(msg: ToRs, ip: IpAddr, port: u16) -> Result<(IpAddr, RsStream), ()> {
    let addr = SocketAddr::from((ip, port));
    let stream = TcpStream::connect(addr).await.map_err(|_| ())?;
    let mut conn: RsStream = connection::wrap(stream);
    match timeout(HB_TIMEOUT, send_confirm(msg, &mut conn)).await {
        Err(_) => Err(()),
        Ok(None) => Err(()),
        Ok(Some(_)) => Ok((ip, conn)),
    }
}

impl Inner {
    pub fn new(chart: Chart, port: u16) -> Self {
        Self {
            port,
            conns: HashMap::new(),
            chart,
        }
    }

    #[tracing::instrument]
    async fn send_to_readservers(&mut self, msg: ToRs) -> usize {
        let conn_ips: HashSet<_> = self.conns.keys().cloned().collect();

        let jobs = self
            .conns
            .iter_mut()
            .map(|(ip, conn)| send(msg.clone(), *ip, conn));

        let results = join_all(jobs).await.into_iter();
        for failed in results.filter_map(Result::err) {
            self.conns.remove(&failed);
        }

        let untried = self
            .chart
            .adresses()
            .into_iter()
            .map(|addr| addr.ip())
            .filter(|addr| !conn_ips.contains(&addr));
        trace!("publising to unconnected servers: {:?}", untried);

        let jobs = untried.map(|ip| conn_and_send(msg.clone(), ip, self.port));
        let new_ok_conns = join_all(jobs).await.into_iter().filter_map(Result::ok);
        self.conns.extend(new_ok_conns);
        self.conns.len()
    }
}
