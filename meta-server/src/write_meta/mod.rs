use futures::{SinkExt, TryStreamExt};
use client_protocol::{connection, Request, Response, PathString};
use std::net::{IpAddr, Ipv4Addr};
use tokio::net::TcpListener;
use tracing::{instrument, warn};

pub use crate::directory::DbError;
pub use crate::directory::writeserv::Directory;
pub use crate::server_conn::to_readserv::ReadServers;

#[instrument]
async fn mkdir(directory: &mut Directory, path: PathString) -> Response {
    match directory.mkdir(path).await {
        Ok(_) => Response::Ok,
        Err(DbError::FileExists) => Response::FileExists,
    }
}

#[instrument]
async fn client_conn(mut stream: ClientStream, mut directory: Directory) {
    while let Ok(msg) = stream.try_next().await {
        let msg = match msg {
            None => continue,
            Some(msg) => msg,
        };
        client_msg(&mut stream, msg, &mut directory).await;
    }
}

#[instrument]
async fn client_msg(stream: &mut ClientStream, msg: Request, directory: &mut Directory) {
    let response = match msg {
        Request::Test => Response::Test,
        Request::AddDir(path) => {
            mkdir(directory, path).await
        }
        Request::Ls(_) => Response::NotReadServ,
        // Request::OpenReadWrite(path, policy) => open_rw(path, policy).await,
        _e => {
            Response::Todo(_e)
        }
    };
    if let Err(_) = stream.send(response).await {
        warn!("could not send response to client");
        return;
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
            client_conn(stream, dir).await;
        });
    }
}
