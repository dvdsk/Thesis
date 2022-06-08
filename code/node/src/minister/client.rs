use std::path::{Path, PathBuf};

use futures::TryStreamExt;
use protocol::connection::{MsgStream, self};
use tokio::net::{TcpListener, TcpStream};

use protocol::{Request, Response};
use tokio::task::JoinSet;
use tracing::debug;

use crate::directory::ReDirectory;
use crate::raft::LogWriter;

pub async fn handle_requests(listener: &mut TcpListener, log: LogWriter, our_subtree: &Path, redirect: ReDirectory) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone(), our_subtree.to_owned(), redirect.clone());
        request_handlers.build_task().name("minister client conn").spawn(handle);
    }
}

async fn handle_conn(stream: TcpStream, mut _log: LogWriter, our_subtree: PathBuf, redirect: ReDirectory) {
    use Request::*;
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("minister got request: {req:?}");

        let final_reply = match req {
            CreateFile(path) if path.starts_with(&our_subtree) => create_file(path).await,
            CreateFile(path) => Response::Redirect (redirect.to_staff(&path).await.minister.addr)
        };
    }
}

async fn create_file(path: PathBuf) -> Response {
    todo!()
}
