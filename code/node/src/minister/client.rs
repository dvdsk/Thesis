use std::path::{Path, PathBuf};

use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::{TcpListener, TcpStream};

use protocol::{Request, Response};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::directory::ReDirectory;
use crate::raft::LogWriter;

pub async fn handle_requests(
    listener: &mut TcpListener,
    log: LogWriter,
    our_subtree: &Path,
    redirect: &mut ReDirectory,
) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone(), our_subtree.to_owned(), redirect.clone());
        request_handlers
            .build_task()
            .name("minister client conn")
            .spawn(handle);
    }
}

async fn handle_conn(
    stream: TcpStream,
    mut _log: LogWriter,
    our_subtree: PathBuf,
    redirect: ReDirectory,
) {
    use Request::*;
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("minister got request: {req:?}");

        let reply = match req {
            CreateFile(path) if path.starts_with(&our_subtree) => create_file(path).await,
            CreateFile(path) => {
                let (staff, subtree) = redirect.to_staff(&path).await;
                Response::Redirect {
                    addr: staff.minister.addr,
                    subtree,
                }
            }
            IsCommitted { path, idx } => todo!(),
        };

        if let Err(e) = stream.send(reply).await {
            warn!("error replying to message: {e:?}");
            return;
        }
    }
}

async fn create_file(_path: PathBuf) -> Response {
    todo!("create file")
}
