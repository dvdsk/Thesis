use std::ops::Range;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use color_eyre::Result;
use futures::stream::Peekable;
use futures::{pin_mut, SinkExt, StreamExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::{TcpListener, TcpStream};

use protocol::{Request, Response};
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tokio::time::{sleep_until, timeout, timeout_at, Instant};
use tracing::{debug, warn};

use crate::directory::{self, Directory};
use crate::raft::{self, LogWriter, HB_TIMEOUT};
use crate::redirectory::ReDirectory;

use super::read_locks::LockManager;

pub async fn handle_requests(
    listener: &mut TcpListener,
    log: LogWriter<super::Order>,
    our_subtree: &Path,
    redirect: &mut ReDirectory,
    dir: Directory,
    manager: LockManager,
) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(
            conn,
            log.clone(),
            our_subtree.to_owned(),
            redirect.clone(),
            dir.clone(),
            manager.clone(),
        );
        request_handlers
            .build_task()
            .name("minister client conn")
            .spawn(handle);
    }
}

/// checks if a request can be handled by this minister
async fn check_subtree(
    our_subtree: &PathBuf,
    req: &Request,
    redirect: &ReDirectory,
) -> Result<(), Response> {
    use Request::*;
    match req {
        List(path) => {
            let (staff, subtree) = redirect.to_staff(path).await;
            Err(Response::Redirect {
                addr: staff.minister.addr,
                subtree,
            })
        }
        Create(path) | IsCommitted { path, .. } if !path.starts_with(&our_subtree) => {
            let (staff, subtree) = redirect.to_staff(path).await;
            Err(Response::Redirect {
                addr: staff.minister.addr,
                subtree,
            })
        }
        _ => Ok(()),
    }
}

type ClientStream<'a> = Pin<&'a mut Peekable<MsgStream<Request, Response>>>;

// Track locks here too to update new nodes
async fn handle_conn(
    stream: TcpStream,
    mut log: LogWriter<super::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    mut dir: Directory,
    mut manager: LockManager,
) {
    use Request::*;
    let stream: MsgStream<Request, Response> = connection::wrap(stream);
    let stream = stream.peekable();
    pin_mut!(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("minister got request: {req:?}");
        let res = check_subtree(&our_subtree, &req, &redirect).await;
        let final_response = match res {
            Err(redirect) => Ok(redirect),
            Ok(_) => match req {
                Create(path) => create_file(path, &mut stream, &mut log, &mut dir).await,
                Write { path, range } => {
                    write_lease(path, stream.as_mut(), range, &mut dir, &mut manager).await
                }
                IsCommitted { idx, .. } => match log.is_committed(idx) {
                    true => Ok(Response::Committed),
                    false => Ok(Response::NotCommitted),
                },
                RefreshLease => Ok(Response::LeaseDropped),
                _ => unreachable!("pattern should have been checked in `check_subtree`"),
            },
        };

        let final_response = match final_response {
            Ok(response) => response,
            Err(e) => {
                warn!("error handling req: {e:?}");
                return;
            }
        };

        if let Err(e) = stream.send(final_response).await {
            warn!("error replying to message: {e:?}");
            return;
        }
    }
}

// guard to ensure that the clerks access gets unlocked if the write lock is
// released by returning from the `write_lease` function
struct WriteLease<'a> {
    dir_lease: directory::LeaseGuard<'a>,
    manager: &'a mut LockManager,
}

impl<'a> Drop for WriteLease<'a> {
    fn drop(self: &mut WriteLease<'a>) {
        self.manager
            .unlock(self.dir_lease.path.to_owned(), self.dir_lease.key());
    }
}

/// offers a write lease for some period, the client should immediatly queue
/// for another write lease to keep it
async fn write_lease(
    path: PathBuf,
    mut stream: ClientStream<'_>,
    range: Range<u64>,
    dir: &mut Directory,
    manager: &mut LockManager,
) -> Result<Response> {
    let dir_lease = match dir.get_write_access(&path, &range) {
        Err(e) => return Ok(Response::Error(e.to_string())),
        Ok(lease) => lease,
    };

    // revoke all reads for this file on the clerks, if this times
    // out the clerk or the client will already have dropped the lease
    let deadline = Instant::now() + HB_TIMEOUT;
    let lock_clerks = manager.lock(path.clone(), range.clone(), dir_lease.key());
    match timeout_at(deadline, lock_clerks).await {
        Err(..) => (),
        Ok(Ok(_)) => (),
        Ok(Err(e)) => {
            warn!("error locking clerks: {e:?}");
            sleep_until(deadline).await;
        }
    }

    let _lease_guard = WriteLease { dir_lease, manager };
    let expires = OffsetDateTime::now_utc() + raft::HB_TIMEOUT;
    stream
        .send(Response::WriteLease(protocol::Lease {
            expires,
            area: range,
        }))
        .await?;

    loop {
        let peek = stream.as_mut().peek();
        match timeout(raft::HB_TIMEOUT, peek).await {
            // recieving this before timeout means we dont drop it which
            // equals refreshing the lease
            Ok(Some(Ok(Request::RefreshLease))) => {
                stream.next().await; // take the refresh cmd out
            }
            _ => {
                break; // lease is released on drop
            }
        }
    }
    Ok(Response::LeaseDropped)
}

async fn create_file(
    path: PathBuf,
    stream: &mut ClientStream<'_>,
    log: &mut LogWriter<super::Order>,
    dir: &mut Directory,
) -> Result<Response> {
    let ticket = log.append(super::Order::Create(path.clone())).await;
    stream.send(Response::Ticket { idx: ticket.idx }).await?;

    ticket.committed().await;
    dir.add_entry(&path);
    Ok(Response::Done)
}
