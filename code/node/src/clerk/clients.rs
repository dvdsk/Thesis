use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Peekable;
use futures::{pin_mut, SinkExt, StreamExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use time::OffsetDateTime;
use tokio::net::{TcpListener, TcpStream};

use color_eyre::Result;
use protocol::{Request, Response};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, instrument, warn};

use crate::directory::{AccessKey, Directory};
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
        map.insert(key, notify)
            .expect("key inserted multiple times");
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
    let blocks = Blocks::default();
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
    mut blocks: Blocks,
) {
    use Request::*;
    let stream: MsgStream<Request, Response> = connection::wrap(stream);
    let stream = stream.peekable();
    pin_mut!(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("clerk got request: {req:?}");

        let final_response = match req {
            List(path) => Ok(Response::List(dir.list(&path))),
            Read { path, range } if path.starts_with(&our_subtree) => {
                read_lease(path, stream.as_mut(), range, &mut dir, &mut readers).await
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
                    addr: staff.minister.addr,
                    subtree,
                })
            }
            UnblockAll => {
                blocks.reset_all(&mut dir);
                Ok(Response::Done)
            }
            UnBlockRead { key } => {
                blocks.reset(key, &mut dir);
                Ok(Response::Done)
            }
            BlockRead { path, range, key } => {
                Ok(block_read(&mut dir, &mut readers, path, range).await)
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
            expires: expires.clone(),
            area: range,
        }))
        .await?;

    loop {
        let peek = stream.as_mut().peek();
        let revoked = notify.notified();
        let timeout = sleep(raft::HB_TIMEOUT);

        let msg = tokio::select! {
            _ = revoked => break,
            _ = timeout => break,
            msg = peek => msg,
        };

        match msg {
            // recieving this before timeout means we dont drop it which
            // equals refreshing the lease
            Some(Ok(Request::RefreshLease)) => {
                stream.next().await; // take the refresh cmd out
            }
            _e => {
                warn!("error while holding read lease {_e:?}");
                break;
            }
        }
    }

    Ok(Response::LeaseDropped)
}

#[instrument(skip(dir, readers))]
async fn block_read(
    dir: &mut Directory,
    readers: &mut Readers,
    path: PathBuf,
    range: Range<u64>,
) -> Response {
    let overlapping = dir.get_overlapping(&path, &range).unwrap();
    for key in overlapping {
        readers.revoke(&key).await;
    }

    dir.get_exclusive_access(&path, &range).unwrap();
    Response::Done
}
