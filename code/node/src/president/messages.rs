use futures::{SinkExt, TryStreamExt};
use protocol::{connection, Request, Response};
use protocol::connection::MsgStream;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use super::FixedTerm;
use crate::president;

use crate::raft::LogWriter;

async fn handle_conn(stream: TcpStream, mut _log: LogWriter<president::Order, FixedTerm>) {
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(msg)) = stream.try_next().await {
        debug!("president got request: {msg:?}");

        if let Err(e) = stream.send(Response::CouldNotRedirect).await {
            warn!("error replying to message: {e:?}");
            return;
        }
    }
}

pub async fn handle_incoming(
    listener: &mut TcpListener,
    log: LogWriter<president::Order, FixedTerm>,
) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone());
        request_handlers
            .build_task()
            .name("president msg conn")
            .spawn(handle);
    }
}
