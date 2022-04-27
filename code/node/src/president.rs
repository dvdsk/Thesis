use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use futures::{SinkExt, TryStreamExt, pin_mut};
use color_eyre::Result;

use instance_chart::Chart as mChart;
type Chart = mChart<3, u16>;

use serde::{Serialize, Deserialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::info;
use protocol::connection;
mod raft;
pub use raft::{Log, Order};

struct Subject {
    connection: TcpStream,
    /// index of the next log entry to send to this subject
    next_idx: u32,
    /// index of highest log entry known to be replicated on server    
    match_idx: u32,
}

async fn manage_subject(address: SocketAddr, rx: broadcast::Receiver<()>, last_log_idx: u32) {
    use std::io::ErrorKind;
    let next_idx = last_log_idx;
    let match_idx = 0;

    let stream = loop {
        match TcpStream::connect(address).await {
            Ok(stream) => break stream,
            Err(e) => match e.kind() {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted => (),
                _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
            },
        }
        sleep(Duration::from_millis(100)).await;
    };
}

/// look for new subjects in the chart and register them
async fn instruct_subjects(chart: &mut Chart, orders: broadcast::Sender<()>, state: raft::State) {
    let mut subjects = JoinSet::new();
    let mut add_subject = |addr| {
        let rx = orders.subscribe();
        let manage = manage_subject(addr, rx, state.last_applied() + 1);
        subjects.spawn(manage);
    };

    let mut notify = chart.notify();
    let mut adresses: HashSet<_> = chart.nth_addr_vec::<0>().into_iter().collect();
    for addr in adresses.iter().cloned() {
        add_subject(addr)
    }

    while let Ok((_id, addr)) = notify.recv_nth_addr::<0>().await {
        let is_new = adresses.insert(addr);
        if is_new {
            add_subject(addr)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg {
    ClientReq(protocol::Request),
    // LoadReport,
    // MinisterReq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
}

async fn handle_conn(stream: TcpStream, log: LogWriter) {
    use Msg::*;
    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(ClientReq(req)) => todo!(),
        };
    }
}

async fn handle_messages(listener: &mut TcpListener, log: LogWriter) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone());
        request_handlers.spawn(handle);
    }
}

#[derive(Debug, Clone)]
struct LogWriter {
    state: raft::State,
    broadcast: broadcast::Sender<()>,
}

pub(super) async fn work(
    log: &mut Log,
    chart: &mut Chart,
    listener: &mut TcpListener,
) -> crate::Role {
    info!("started work as president: {}", chart.our_id());
    let Log { orders, state, .. } = log;
    let (broadcast, _) = broadcast::channel(16);
    let log_writer = LogWriter {
        state: state.clone(),
        broadcast: broadcast.clone(),
    };

    tokio::select! {
        () = instruct_subjects(chart, broadcast.clone(), state.clone()) => unreachable!(),
        () = handle_messages(listener, log_writer) => unreachable!(),
        usurper = orders.recv() => match usurper {
            Some(Order::ResignPres) => crate::Role::Idle,
            Some(_other) => unreachable!("The president should never recieve
                                         an order expect resign, recieved: {_other:?}"),
            None => panic!("channel was dropped,
                           this means another thread panicked. Joining the panic"),
        },
    }
}
