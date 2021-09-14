use futures::{SinkExt, TryStreamExt};
use client_protocol::{connection, Request, Response, PathString};
use std::net::{IpAddr, Ipv4Addr};
use tokio::net::TcpListener;
use tracing::instrument;

pub use crate::directory::DbError;
pub use crate::directory::writeserv::Directory;
pub use crate::server_conn::to_readserv::ReadServers;

async fn mkdir(mut directory: Directory, path: PathString) -> Response {
    match directory.mkdir(path).await {
        Ok(_) => Response::Ok,
        Err(DbError::FileExists) => Response::FileExists,
    }
}

#[instrument]
async fn handle_client(mut stream: ClientStream, directory: Directory) {
    if let Some(msg) = stream.try_next().await.unwrap() {
        let response = match msg {
            Request::Test => Response::Test,
            Request::AddDir(path) => mkdir(directory, path).await,
            // Request::OpenReadWrite(path, policy) => open_rw(path, policy).await,
            _e => {
                println!("TODO: responding to {:?}", _e);
                Response::Todo(_e)
            }
        };
        if let Err(_) = stream.send(response).await {
            return;
        }
    }
}

type ClientStream = connection::MsgStream<Request, Response>;
pub async fn server(port: u16, directory: Directory) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let dir = directory.clone();
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: ClientStream = connection::wrap(socket);
            handle_client(stream, dir).await;
        });
    }
}
