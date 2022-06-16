use std::net::SocketAddr;
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
use tokio::time::{timeout, sleep};
use tracing::{debug, warn};

use crate::directory::Directory;
use crate::raft::{self, LogWriter, HB_TIMEOUT};
use crate::redirectory::ReDirectory;

pub async fn handle_requests(
    listener: &mut TcpListener,
    log: LogWriter<super::Order>,
    our_subtree: &Path,
    redirect: &mut ReDirectory,
    dir: Directory,
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
            let (staff, subtree) = redirect.to_staff(&path).await;
            Err(Response::Redirect {
                addr: staff.minister.addr,
                subtree,
            })
        }
        Create(path) | IsCommitted { path, .. } if !path.starts_with(&our_subtree) => {
            let (staff, subtree) = redirect.to_staff(&path).await;
            Err(Response::Redirect {
                addr: staff.minister.addr,
                subtree,
            })
        }
        _ => Ok(()),
    }
}

type ClientStream<'a> = Pin<&'a mut Peekable<MsgStream<Request, Response>>>;

async fn handle_conn(
    stream: TcpStream,
    mut log: LogWriter<super::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    mut dir: Directory,
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
                    write_lease(path, stream.as_mut(), range, &mut dir, &redirect).await
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

/// offers a write lease for some period, the client should immediatly queue
/// for another write lease to keep it
async fn write_lease(
    path: PathBuf,
    mut stream: ClientStream<'_>,
    range: Range<u64>,
    dir: &mut Directory,
    redirect: &ReDirectory,
) -> Result<Response> {
    let key = match dir.get_write_access(&path, &range) {
        Err(e) => return Ok(Response::Error(e)),
        Ok(key) => key,
    };

    // revoke all reads for this file on the clerks, if this times 
    // out the clerk or the client will already have dropped the lease
    let revoke_clerks = revoke_reads_on_clerks(&path, redirect, &range);
    let _ig_err = timeout(HB_TIMEOUT, revoke_clerks).await;

    let expires = OffsetDateTime::now_utc() + raft::HB_TIMEOUT;
    stream
        .send(Response::WriteLease(protocol::Lease {
            expires: expires.clone(),
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
                dir.revoke_access(&path, key);
                break;
            }
        }
    }
    Ok(Response::LeaseDropped)
}

async fn revoke_read(addr: SocketAddr, path: PathBuf, range: Range<u64>) -> Result<()> {
    let stream = TcpStream::connect(addr).await?;
    let mut stream: MsgStream<Response, Request> = connection::wrap(stream);
    stream.send(Request::RevokeRead { path, range }).await?;
    Ok(())
}

/// connects all the clerks and revokes reads, might block forever therefore should be ran
/// inside a timeout
async fn revoke_reads_on_clerks(path: &PathBuf, redirect: &ReDirectory, range: &Range<u64>) {
    let (staff, _) = redirect.to_staff(path).await;
    let mut requests = staff
        .clerks
        .into_iter()
        .map(|clerk| revoke_read(clerk.addr, path.clone(), range.clone()))
        .fold(JoinSet::new(), |mut set, fut| {
            set.build_task().name("revoke_read").spawn(fut);
            set
        });

    while let Some(res) = requests.join_one().await {
        if res.is_err() {
            // clerk is malfunctioning, must ensure the clerk's clients
            // read leases have timed out
            sleep(HB_TIMEOUT).await;
        }
    }
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
