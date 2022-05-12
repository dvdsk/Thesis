use futures::{TryStreamExt, SinkExt};
use protocol::connection;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpStream, TcpListener};
use tokio::task::JoinSet;
use tracing::{warn, debug};

use super::LogWriter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    ClientReq(protocol::Request),
    #[cfg(test)]
    Test(u8),
    // LoadReport,
    // MinisterReq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    GoAway, // president itself does not assist citizens
    // Okay,
}

#[allow(dead_code)]
async fn client_req() {
}

#[cfg(test)]
async fn test_req(n: u8, log: &mut LogWriter) -> Option<Reply> {
    use super::Order;
    log.append(Order::Test(n));
    None
}

async fn handle_conn(stream: TcpStream, mut log: LogWriter) {
    use Msg::*;
    use Reply::*;
    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(Some(msg)) = stream.try_next().await {
        debug!("president got request: {msg:?}");

        let reply = match msg {
            ClientReq(_) => Some(GoAway),
            #[cfg(test)]
            Test(n) => test_req(n, &mut log).await,
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
