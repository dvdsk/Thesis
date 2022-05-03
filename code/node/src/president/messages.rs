use futures::{TryStreamExt, SinkExt};
use protocol::connection;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpStream, TcpListener};
use tokio::task::JoinSet;
use tracing::warn;

use super::LogWriter;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg {
    ClientReq(protocol::Request),
    // LoadReport,
    // MinisterReq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
    GoAway, // president does not forward
}

#[allow(dead_code)]
async fn client_req() {
}

async fn handle_conn(stream: TcpStream, _log: LogWriter) {
    use Msg::*;
    use Reply::*;
    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(ClientReq(_)) => Some(GoAway),
        };

        if let Some(reply) = reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to message: {e:?}");
                return;
            }
        }
    }
}

pub async fn handle_incoming(listener: &mut TcpListener, log: LogWriter) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone());
        request_handlers.spawn(handle);
    }
}
