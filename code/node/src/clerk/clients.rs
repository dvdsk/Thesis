use std::path::PathBuf;

use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::{TcpListener, TcpStream};

use protocol::{Request, Response};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::directory::Directory;
use crate::redirectory::ReDirectory;
use crate::{minister, raft};

pub async fn handle_requests(
    listener: &mut TcpListener,
    state: raft::State<minister::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    dir: Directory,
) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(
            conn,
            state.clone(),
            our_subtree.to_owned(),
            redirect.clone(),
            dir.clone(),
        );
        request_handlers
            .build_task()
            .name("clerk client conn")
            .spawn(handle);
    }
}

async fn handle_conn(
    stream: TcpStream,
    state: raft::State<minister::Order>,
    our_subtree: PathBuf,
    redirect: ReDirectory,
    mut dir: Directory,
) {
    use Request::*;
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("clerk got request: {req:?}");

        let reply = match req {
            List(path) => Response::List(dir.list(&path)),
            // ReadFile(path) => {
            //     todo!("lease time");
            // }
            IsCommitted { path, idx } if path.starts_with(&our_subtree) => {
                match state.is_committed(idx) {
                    true => Response::Committed,
                    false => Response::NotCommitted,
                }
            }
            IsCommitted { path, .. } | Write { path, .. } | Create(path) => {
                let (staff, subtree) = redirect.to_staff(&path).await;
                Response::Redirect {
                    addr: staff.minister.addr,
                    subtree,
                }
            }
            RevokeRead { .. } => todo!(),
            RefreshLease => todo!(),
        };

        if let Err(e) = stream.send(reply).await {
            warn!("error replying to message: {e:?}");
            return;
        }
    }
}
