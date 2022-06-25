use color_eyre::eyre::eyre;
use color_eyre::Help;
use futures::{SinkExt, TryStreamExt};
use protocol::connection::MsgStream;
use protocol::{connection, Request, Response};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};
use tracing::{error, instrument, warn};

use crate::raft::{CONN_RETRY_PERIOD, HB_TIMEOUT};
use crate::redirectory::ClientAddr;
use crate::Id;
use protocol::AccessKey;

use super::clerks;

#[derive(Debug, Clone)]
pub enum LockReq {
    Lock {
        path: PathBuf,
        range: Range<u64>,
        key: AccessKey,
    },
    Unlock {
        key: AccessKey,
        path: PathBuf,
    },
}

impl LockReq {
    fn into_request(self) -> Request {
        match self {
            Self::Lock { path, range, key } => Request::Lock { path, range, key },
            Self::Unlock { path, key } => Request::Unlock { path, key },
        }
    }
}

#[instrument]
async fn connect(address: &ClientAddr) -> MsgStream<Response, Request> {
    use std::io::ErrorKind;
    loop {
        match TcpStream::connect(address.as_untyped()).await {
            Ok(stream) => {
                let stream = connection::wrap(stream);
                break stream;
            }
            Err(e) => match e.kind() {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted => {
                    warn!("Can not connect to clerk");
                    sleep(CONN_RETRY_PERIOD).await;
                    continue;
                }
                _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
            },
        }
    }
}

type AckSender = mpsc::Sender<Result<(), ()>>;
#[instrument(skip_all)]
async fn keep_client_up_to_date(
    broadcast: &mut broadcast::Receiver<(AckSender, LockReq)>,
    stream: &mut MsgStream<Response, Request>,
) {
    loop {
        let (ack_tx, lock_req) = broadcast.recv().await.unwrap();
        let lock_req = lock_req.into_request();

        if let Err(e) = stream.send(lock_req).await {
            warn!("did not send to host, error: {e:?}");
            ack_tx.try_send(Result::Err(())).unwrap();
            break;
        }

        match timeout(HB_TIMEOUT, stream.try_next()).await {
            Err(..) => (), // clerk must be down, we no longer contact it
            Ok(Ok(Some(Response::Done))) => {
                ack_tx.try_send(Result::Ok(())).unwrap();
                continue;
            }
            Ok(Ok(Some(r))) => unreachable!("incorrect reply from clerk: {r:?}"),
            Ok(Ok(None)) => warn!("lost connection to client"),
            Ok(Err(e)) => warn!("error in transport: {e:?}"),
        }

        ack_tx.try_send(Result::Err(())).unwrap();
        return;
    }
    // clerk must be down
}

#[instrument(skip(stream), err)]
async fn send_initial_locks(
    stream: &mut MsgStream<Response, Request>,
    list: Vec<LockReq>,
) -> color_eyre::Result<()> {
    stream.send(Request::UnlockAll).await?;
    for lock in list {
        dbg!("sending lock: {lock:?}");
        stream.send(lock.into_request()).await?;

        match timeout(HB_TIMEOUT, stream.try_next()).await {
            Err(..) => return Err(eyre!("timeout recieving response")), // clerk must be down, we no longer contact it
            Ok(Ok(Some(Response::Done))) => (),
            Ok(Ok(_)) => unreachable!("incorrect reply from clerk"),
            Ok(Err(e)) => {
                return Err(eyre!("transport error, recieving response")).with_error(|| e)
            }
        }
    }
    Ok(())
}

#[instrument(skip(broadcast, lock_list))]
async fn manage_clerks_locks(
    address: ClientAddr,
    mut broadcast: broadcast::Receiver<(AckSender, LockReq)>,
    lock_list: Vec<LockReq>,
) {
    let mut stream = connect(&address).await;
    send_initial_locks(&mut stream, lock_list).await.unwrap();
    keep_client_up_to_date(&mut broadcast, &mut stream).await;
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
            .map(|(key, (path, range))| (*key, path.clone(), range.clone()))
            .map(|(key, path, range)| LockReq::Lock { path, range, key })
            .collect()
    }

    fn apply(&mut self, req: LockReq) {
        match req {
            LockReq::Unlock { key, .. } => self.0.remove(&key),
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
        self.0
            .try_send((LockReq::Lock { path, range, key }, tx))
            .unwrap();
        res.await.unwrap()
    }

    pub fn unlock(&mut self, path: PathBuf, key: AccessKey) {
        let (tx, _) = oneshot::channel();
        // we do not wait for a result, failure in unlock means failure
        // of clerks, replacement clecks will not hold the lock
        self.0
            .try_send((LockReq::Unlock { path, key }, tx))
            .unwrap();
    }
}

/// look for new subjects in the chart and register them
#[instrument(skip_all)]
pub async fn maintain_file_locks(
    mut members: clerks::ClientAddrMap,
    mut lock_req: mpsc::Receiver<(LockReq, oneshot::Sender<Result<(), FanOutError>>)>,
) -> ! {
    let mut locks = Locks::new();
    let (mut lock_broadcast, _) = broadcast::channel(16);
    let broadcast_subscriber = lock_broadcast.clone();

    let mut clerks = JoinSet::new(); // TODO refactor some things out
    let add_clerk = |addr, locks: &Locks, clerks: &mut JoinSet<_>| {
        let broadcast_rx = broadcast_subscriber.subscribe();

        let manage = manage_clerks_locks(addr, broadcast_rx, locks.list());
        clerks.build_task().name("manage_clerk").spawn(manage);
    };

    let mut notify = members.notify();
    let mut adresses: HashSet<(Id, ClientAddr)> = members
        .adresses()
        .into_iter()
        .map(|(id, addr)| (id, addr))
        .collect();

    for (_, addr) in adresses.iter().cloned() {
        add_clerk(addr, &locks, &mut clerks)
    }

    loop {
        if clerks.is_empty() {
            let (id, addr) = notify.recv().await.unwrap();
            let is_new = adresses.insert((id, addr));
            if is_new {
                add_clerk(addr, &locks, &mut clerks);
            }
        }

        tokio::select! {
            res = clerks.join_one() => {
                // TODO deregister the clerk/report clerk dead to president
                // (not in current design)
                match res.unwrap() {
                    Err(e) => error!("error while managering clerk: {e:?}"),
                    Ok(_) => warn!("disconnected from clerk"),
                }

            },
            recoverd = notify.recv() => {
                let (id, addr) = recoverd.unwrap();
                let is_new = adresses.insert((id, addr));
                if is_new {
                    add_clerk(addr, &locks, &mut clerks);
                }
            },
            req = lock_req.recv() => {
                let (req, ack) = req.unwrap();
                locks.apply(req.clone());
                match &req {
                    LockReq::Lock {..} => {
                        let res = fan_out_lock_req(&mut lock_broadcast, req).await;
                        ack.send(res).unwrap();
                    }
                    LockReq::Unlock {..} => {
                        fan_out_lock_req(&mut lock_broadcast, req).await.unwrap();
                        // minsters do not wait for an ack on unlocks
                    },
                }
            },
        }
    }
}

#[derive(Debug)]
pub enum FanOutError {
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
