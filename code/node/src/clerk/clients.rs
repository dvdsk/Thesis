use std::path::PathBuf;

use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::{TcpListener, TcpStream};

use protocol::{Request, Response};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::directory::ReDirectory;

pub async fn handle_requests(
    listener: &mut TcpListener,
    our_subtree: PathBuf,
    redirect: ReDirectory,
) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, our_subtree.to_owned(), redirect.clone());
        request_handlers
            .build_task()
            .name("clerk client conn")
            .spawn(handle);
    }
}

async fn handle_conn(stream: TcpStream, _our_subtree: PathBuf, redirect: ReDirectory) {
    use Request::*;
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("clerk got request: {req:?}");

        let reply = match req {
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
