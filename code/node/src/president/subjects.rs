use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::{Chart, raft};


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
pub async fn instruct(chart: &mut Chart, orders: broadcast::Sender<()>, state: raft::State) {
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
