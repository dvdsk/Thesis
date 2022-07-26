use color_eyre::eyre::eyre;
use color_eyre::Help;
use futures::{SinkExt, TryStreamExt};
use protocol::connection::MsgStream;
use protocol::{connection, Request, Response};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, instrument, warn};

use crate::raft::{CONN_RETRY_PERIOD, HB_TIMEOUT};
use crate::redirectory::ClientAddr;
use crate::Id;
use protocol::AccessKey;

use super::clerks;

type ManagerAck = oneshot::Sender<Result<(), FanOutError>>;

#[derive(Debug, Clone)]
pub(super) struct LockReq {
    path: PathBuf,
    range: Range<u64>,
    key: AccessKey,
}

#[derive(Debug, Clone)]
pub(super) struct UnlockReq {
    key: AccessKey,
    path: PathBuf,
}

impl LockReq {
    fn into_request(self) -> Request {
        Request::Lock {
            path: self.path,
            range: self.range,
            key: self.key,
        }
    }
}
impl UnlockReq {
    fn into_request(self) -> Request {
        Request::Unlock {
            path: self.path,
            key: self.key,
        }
    }
}

#[instrument]
async fn connect(address: &ClientAddr) -> MsgStream<Response, Request> {
    use std::io::ErrorKind;
    loop {
        match TcpStream::connect(address.as_untyped()).await {
            Ok(stream) => {
                stream.set_nodelay(true).unwrap();
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
    broadcast: &mut broadcast::Receiver<(AckSender, Request)>,
    stream: &mut MsgStream<Response, Request>,
) {
    loop {
        let (ack_tx, lock_req) = broadcast.recv().await.unwrap();

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
    mut broadcast: broadcast::Receiver<(AckSender, Request)>,
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
            .map(|(key, path, range)| LockReq { path, range, key })
            .collect()
    }

    #[instrument(level = "debug", skip(self))]
    fn lock(&mut self, req: LockReq) {
        debug!("locking: {req:?}");
        let existing = self.0.insert(req.key, (req.path, req.range));
        assert_eq!(
            existing, None,
            "locks are unique, this should have been enforced 
             by `dir get write access` in minister::clients::write_lease"
        );
    }

    #[instrument(level = "debug", skip(self))]
    fn unlock(&mut self, req: &UnlockReq) {
        debug!("unlocking: {req:?}");
        let removed = self.0.remove(&req.key);
        assert!(
            removed.is_some(),
            "can not unlock something thats not there"
        );
    }
}

#[derive(Debug, Clone)]
pub struct LockManager {
    lock_req: mpsc::Sender<(ManagerAck, LockReq)>,
    unlock_req: mpsc::Sender<UnlockReq>,
}

type Recievers = (
    mpsc::Receiver<(ManagerAck, LockReq)>,
    mpsc::Receiver<UnlockReq>,
);

impl LockManager {
    pub(super) fn new() -> (Self, Recievers) {
        // unlock requests have a higher capacity, and are processed before
        // lock request. This ensures a server under high locking load will never
        // be able to unlock keeping the load high forever
        let (lock_req, rx_lock) = mpsc::channel(16);
        let (unlock_req, rx_unlock) = mpsc::channel(32);
        (
            Self {
                lock_req,
                unlock_req,
            },
            (rx_lock, rx_unlock),
        )
    }

    // Note: blocks no longer then HB_TIMEOUT as lock manager can not
    // block longer then that (timeout fan_out_req)
    pub async fn lock(
        &mut self,
        path: PathBuf,
        range: Range<u64>,
        key: AccessKey,
    ) -> Result<(), FanOutError> {
        let (tx, res) = oneshot::channel();
        match self.lock_req.try_send((tx, LockReq { path, range, key })) {
            Ok(_) => (),
            Err(TrySendError::Full(_)) => return Err(FanOutError::NoCapacity),
            Err(TrySendError::Closed(_)) => panic!("lock_req queue was closed"),
        }
        res.await.expect("reciever used twice")
    }

    // Note: is non blocking
    pub fn unlock(&mut self, path: PathBuf, key: AccessKey) {
        // we do not wait for a result, failure in unlock means failure
        // of clerks, replacement clecks will not hold the lock
        self.unlock_req.try_send(UnlockReq { path, key }).unwrap();
    }
}

/// look for new subjects in the chart and register them
#[instrument(skip_all)]
pub(super) async fn maintain_file_locks(
    mut members: clerks::ClientAddrMap,
    mut lock_req: mpsc::Receiver<(ManagerAck, LockReq)>,
    mut unlock_req: mpsc::Receiver<UnlockReq>,
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
            biased;
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
            req = unlock_req.recv() => {
                let req = req.expect("sender should never hang up");
                locks.unlock(&req);
                let req = req.into_request();
                fan_out_req(&mut lock_broadcast, req).await.unwrap();
            },
            // biased select; only if no locks can be unlocked lock another one
            req = lock_req.recv() => {
                let (ack, req)= req.expect("sender should never hang up");
                locks.lock(req.clone());
                let req = req.into_request();
                let res = fan_out_req(&mut lock_broadcast, req).await;
                ack.send(res).unwrap();
            },
        }
    }
}

#[derive(Debug)]
pub enum FanOutError {
    NoSubscribedNodes,
    ClerkIsDown,
    ConnectionLost,
    ClerkTimedOut,
    NoCapacity,
}

async fn processed_by_all(
    lock_broadcast: &mut broadcast::Sender<(AckSender, Request)>,
    mut rx: mpsc::Receiver<Result<(), ()>>,
) -> Result<(), FanOutError> {
    use FanOutError::*;
    for _ in 0..lock_broadcast.receiver_count() {
        rx.recv()
            .await
            .ok_or(ClerkIsDown)?
            .map_err(|_| ConnectionLost)?;
    }
    Ok(())
}

async fn fan_out_req(
    lock_broadcast: &mut broadcast::Sender<(AckSender, Request)>,
    req: Request,
) -> Result<(), FanOutError> {
    use FanOutError::*;
    let (tx, rx) = mpsc::channel(16);
    let msg = (tx, req);

    lock_broadcast.send(msg).map_err(|_| NoSubscribedNodes)?;
    timeout(HB_TIMEOUT, processed_by_all(lock_broadcast, rx))
        .await
        .map_err(|_| ClerkTimedOut)??;

    Ok(())
}
