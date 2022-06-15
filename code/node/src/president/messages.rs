use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use protocol::connection::MsgStream;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::Idx;
use crate::president;

use crate::raft::LogWriter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    ClientReq(protocol::Request),
    #[cfg(test)]
    Test {
        n: u8,
        follow_up: Option<Idx>,
    },
    // LoadReport,
    // MinisterReq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    GoAway, // president itself does not assist citizens
    Waiting(Idx),
    Done,
    Error,
    // Okay,
}

#[cfg(test)]
async fn test_req(
    n: u8,
    partial: Option<Idx>,
    log: &mut LogWriter<president::Order>,
    stream: &mut MsgStream<Msg, Reply>,
) -> Option<Reply> {
    use super::Order;

    debug!("appending Test({n}) to log");
    let ticket = match partial {
        Some(idx) => log.re_append(Order::Test(n), idx).await,
        None => log.append(Order::Test(n)).await,
    };
    stream.send(Reply::Waiting(ticket._idx)).await.ok()?;
    ticket.notify.notified().await;
    Some(Reply::Done)
}

async fn handle_conn(stream: TcpStream, mut _log: LogWriter<president::Order>) {
    use Msg::*;
    use Reply::*;
    let mut stream: MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(Some(msg)) = stream.try_next().await {
        debug!("president got request: {msg:?}");

        let final_reply = match msg {
            ClientReq(_) => Some(GoAway),
            #[cfg(test)]
            Test { n, follow_up: partial } => test_req(n, partial, &mut _log, &mut stream).await,
        };

        if let Some(reply) = final_reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to message: {e:?}");
                return;
            }
        }
    }
}

pub async fn handle_incoming(listener: &mut TcpListener, log: LogWriter<president::Order>) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone());
        request_handlers.build_task().name("president msg conn").spawn(handle);
    }
}
