use futures::{SinkExt, TryStreamExt};
use protocol::connection::MsgStream;
use protocol::{connection, Request, Response};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::ops::Range;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};
use tracing::warn;

use protocol::AccessKey;
use crate::raft::subjects::{Source, SourceNotify};
use crate::raft::{CONN_RETRY_PERIOD, HB_TIMEOUT};

#[derive(Debug, Clone)]
enum LockReq {
    Lock {
        path: PathBuf,
        range: Range<u64>,
        key: AccessKey,
    },
    Unlock {
        key: AccessKey,
    },
}

impl LockReq {
    fn to_request(self) -> Request {
        match self {
            Self::Lock { path, range, key } => Request::Lock { path, range, key },
            Self::Unlock { key } => Request::Unlock { key },
        }
    }
}

async fn connect(address: &SocketAddr) -> MsgStream<Response, Request> {
    use std::io::ErrorKind;
    loop {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                let stream = connection::wrap(stream);
                break stream;
            }
            Err(e) => match e.kind() {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted => {
                    sleep(CONN_RETRY_PERIOD).await;
                    continue;
                }
                _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
            },
        }
    }
}

type AckSender = mpsc::Sender<Result<(), ()>>;
async fn keep_client_up_to_date(
    broadcast: &mut broadcast::Receiver<(AckSender, LockReq)>,
    stream: &mut MsgStream<Response, Request>,
) {
    loop {
        let (ack_tx, lock_req) = broadcast.recv().await.unwrap();
        let lock_req = lock_req.to_request();

        if let Err(e) = stream.send(lock_req).await {
            warn!("did not send to host, error: {e:?}");
            ack_tx.try_send(Result::Err(())).unwrap();
            break;
        }

        match timeout(HB_TIMEOUT, stream.try_next()).await {
            Err(..) => (), // clerk must be down, we no longer contact it
            Ok(Ok(Some(Response::Done))) => return ack_tx.try_send(Result::Ok(())).unwrap(),
            Ok(Ok(_)) => unreachable!("incorrect reply from clerk"),
            Ok(Err(e)) => (),
        }

        ack_tx.try_send(Result::Err(())).unwrap();
    }
    // clerk must be down
}

async fn send_initial_locks(
    stream: &mut MsgStream<Response, Request>,
    list: Vec<LockReq>,
) -> Result<(), ()> {
    for lock in list {
        stream.send(Request::UnlockAll);
        stream.send(lock.to_request()).await;
        match timeout(HB_TIMEOUT, stream.try_next()).await {
            Err(..) => return Err(()), // clerk must be down, we no longer contact it
            Ok(Ok(Some(Response::Done))) => (),
            Ok(Ok(_)) => unreachable!("incorrect reply from clerk"),
            Ok(Err(e)) => return Err(()),
        }
    }
    Ok(())
}

async fn manage_clerks_locks(
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<(AckSender, LockReq)>,
    lock_list: Vec<LockReq>,
) {
    let mut stream = connect(&address).await;
    if send_initial_locks(&mut stream, lock_list).await.is_ok() {
        keep_client_up_to_date(&mut broadcast, &mut stream).await;
    }
}

struct Locks(HashMap<AccessKey, (PathBuf, Range<u64>)>);

impl Locks {
    fn new() -> Self {
        Self(HashMap::new())
    }

    /// list of lockreq::lock
    fn list(&self) -> Vec<LockReq> {
        self.0
            .iter()
            .map(|(key, (path, range))| (key.clone(), path.clone(), range.clone()))
            .map(|(key, path, range)| LockReq::Lock { path, range, key })
            .collect()
    }

    fn apply(&mut self, req: LockReq) {
        match req {
            LockReq::Unlock { key } => self.0.remove(&key),
            LockReq::Lock { path, range, key } => self.0.insert(key, (path, range)),
        };
    }
}

type LockManagerMsg = (LockReq, oneshot::Sender<Result<(), FanOutError>>);

#[derive(Debug, Clone)]
pub struct LockManager(mpsc::Sender<LockManagerMsg>);

impl LockManager {
    pub fn new() -> (Self, mpsc::Receiver<LockManagerMsg>) {
        let (tx, rx) = mpsc::channel(16);
        (Self(tx), rx)
    }

    pub async fn lock(
        &mut self,
        path: PathBuf,
        range: Range<u64>,
        key: AccessKey,
    ) -> Result<(), FanOutError> {
        let (tx, res) = oneshot::channel();
        self.0.try_send((LockReq::Lock { path, range, key }, tx));
        res.await.unwrap()
    }

    pub async fn unlock(&mut self, key: AccessKey) -> Result<(), FanOutError> {
        let (tx, res) = oneshot::channel();
        self.0.try_send((LockReq::Unlock { key }, tx));
        res.await.unwrap()
    }
}

/// look for new subjects in the chart and register them
pub async fn maintain_file_locks(
    mut members: super::clerks::Map,
    mut lock_req: mpsc::Receiver<(LockReq, oneshot::Sender<Result<(), FanOutError>>)>,
) {
    let mut locks = Locks::new();
    let (mut lock_broadcast, _) = broadcast::channel(16);
    let broadcast_subscriber = lock_broadcast.clone();

    let mut clerks = JoinSet::new();
    let mut add_clerk = |addr, locks: &Locks| {
        let broadcast_rx = broadcast_subscriber.subscribe();

        let manage = manage_clerks_locks(addr, broadcast_rx, locks.list());
        clerks.build_task().name("manage_clerk").spawn(manage);
    };

    let mut notify = members.notify();
    let mut adresses: HashSet<_> = members.adresses().into_iter().collect();
    for (_, addr) in adresses.iter().cloned() {
        add_clerk(addr, &locks)
    }

    loop {
        tokio::select! {
            recoverd = notify.recv_new() => {
                let (id, addr) = recoverd.unwrap();
                let is_new = adresses.insert((id, addr));
                if is_new {
                    add_clerk(addr, &locks);
                }
            },
            req = lock_req.recv() => {
                let (req, ack) = req.unwrap();
                locks.apply(req.clone());
                let res = fan_out_lock_req(&mut lock_broadcast, req).await;
                ack.send(res).unwrap();
            },
        };
    }
}

#[derive(Debug)]
enum FanOutError {
    NoSubscribedNodes,
    ClerkDidNotRespond,
}

async fn fan_out_lock_req(
    lock_broadcast: &mut broadcast::Sender<(AckSender, LockReq)>,
    req: LockReq,
) -> Result<(), FanOutError> {
    use FanOutError::*;
    let n = lock_broadcast.receiver_count();
    let (tx, mut rx) = mpsc::channel(16);
    let msg = (tx, req);

    lock_broadcast.send(msg).map_err(|_| NoSubscribedNodes)?;
    for _ in 0..n {
        let _clerk_informed = rx.recv().await.ok_or(ClerkDidNotRespond)?;
    }

    Ok(())
}
