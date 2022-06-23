use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Peekable;
use futures::{pin_mut, SinkExt, StreamExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use protocol::AccessKey;
use time::OffsetDateTime;
use tokio::net::{TcpListener, TcpStream};

use color_eyre::Result;
use protocol::{Request, Response};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, warn};

use super::locks::Locks;
use crate::directory::Directory;
use crate::redirectory::ReDirectory;
use crate::{minister, raft};

#[derive(Default, Clone)]
struct Readers(Arc<Mutex<HashMap<AccessKey, Arc<Notify>>>>);

impl Readers {
    pub async fn revoke(&mut self, key: &AccessKey) {
        let mut map = self.0.lock().await;
        let notify = map.remove(key).expect("multiple revokes issued");
        notify.notify_one();
    }

    async fn add(&self, key: AccessKey, notify: Arc<Notify>) {
        let mut map = self.0.lock().await;
        let existing = map.insert(key, notify);
        assert!(existing.is_none(), "key inserted multiple times");
    }
}

pub async fn handle_requests(
    listener: &mut TcpListener,
    state: raft::State<minister::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    dir: Directory,
) {
    let readers = Readers::default();
    let blocks = Locks::default();
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(
            conn,
            state.clone(),
            our_subtree.to_owned(),
            redirect.clone(),
            dir.clone(),
            readers.clone(),
            blocks.clone(),
        );
        request_handlers
            .build_task()
            .name("clerk client conn")
            .spawn(handle);
    }
}

type ClientStream<'a> = Pin<&'a mut Peekable<MsgStream<Request, Response>>>;

async fn handle_conn(
    stream: TcpStream,
    state: raft::State<minister::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    mut dir: Directory,
    mut readers: Readers,
    mut locks: Locks,
) {
    use Request::*;
    let stream: MsgStream<Request, Response> = connection::wrap(stream);
    let stream = stream.peekable();
    pin_mut!(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("got request: {req:?}");

        let final_response = match req {
            List(path) => Ok(Response::List(dir.list(&path))),
            Read { path, range } if path.starts_with(&our_subtree) => {
                read_lease(path, stream.as_mut(), range, &mut dir, &readers).await
            }
            IsCommitted { path, idx } if path.starts_with(&our_subtree) => {
                match state.is_committed(idx) {
                    true => Ok(Response::Committed),
                    false => Ok(Response::NotCommitted),
                }
            }
            IsCommitted { path, .. } | Write { path, .. } | Create(path) | Read { path, .. } => {
                let (staff, subtree) = redirect.to_staff(&path).await;
                Ok(Response::Redirect {
                    staff: staff.for_client(),
                    subtree,
                })
            }
            HighestCommited => Ok(Response::HighestCommited(state.commit_index())),
            UnlockAll => {
                locks.reset_all(&mut dir).await;
                Ok(Response::Done)
            }
            Unlock { path, key } => {
                locks.reset(path, key, &mut dir).await;
                Ok(Response::Done)
            }
            Lock { path, range, key } => {
                // for consistency this has to happen first, if we did it later new
                // reads could be added while we are revoking the existing
                locks.add(&mut dir, path.clone(), range.clone(), key).await;
                let overlapping = dir.get_overlapping_reads(&path, &range).unwrap();
                for key in overlapping {
                    readers.revoke(&key).await;
                }
                Ok(Response::Done)
            }
            RefreshLease => Ok(Response::LeaseDropped),
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

async fn read_lease(
    path: PathBuf,
    mut stream: ClientStream<'_>,
    range: Range<u64>,
    dir: &mut Directory,
    readers: &Readers,
) -> Result<Response> {
    let lease = match dir.get_read_access(&path, &range) {
        Err(e) => return Ok(Response::Error(e.to_string())),
        Ok(lease) => lease,
    };

    let notify = Arc::new(Notify::new());
    readers.add(lease.key(), notify.clone()).await;

    let expires = OffsetDateTime::now_utc() + raft::HB_TIMEOUT;
    stream
        .send(Response::ReadLease(protocol::Lease {
            expires,
            area: range,
        }))
        .await?;

    loop {
        let peek = stream.as_mut().peek();
        let revoked = notify.notified();
        let timeout = sleep(protocol::LEASE_TIMEOUT);

        let peeked_msg = tokio::select! {
            _ = revoked => break,
            _ = timeout => break,
            msg = peek => msg,
        };

        match peeked_msg {
            // recieving this before timeout means we dont drop it which
            // equals refreshing the lease
            Some(Ok(Request::RefreshLease)) => {
                stream.next().await; // take the refresh cmd out
            }
            _e => {
                debug!("client canceld lease");
                break;
            }
        }
    }

    Ok(Response::LeaseDropped)
}
