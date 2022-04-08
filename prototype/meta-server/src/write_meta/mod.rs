use futures::{SinkExt, TryStreamExt};
use client_protocol::{connection, Request, Response, PathString, Existence};
use std::net::{IpAddr, Ipv4Addr};
use tokio::net::TcpListener;
use tracing::{instrument, warn};

pub use crate::directory::DbError;
pub use crate::directory::writeserv::Directory;
pub use crate::server_conn::to_readserv::ReadServers;

#[instrument(level = "debug", skip(directory))]
async fn mkdir(directory: &mut Directory, path: PathString) -> Response {
    match directory.mkdir(path).await {
        Ok(_) => Response::Ok,
        Err(DbError::FileExists) => Response::FileExists,
        Err(_) => panic!("error should not occur for mkdir"),
    }
}

#[instrument(level = "debug", skip(directory))]
async fn rmdir(directory: &mut Directory, path: PathString) -> Response {
    match directory.rmdir(path).await {
        Ok(_) => Response::Ok,
        Err(DbError::NoSuchDir) => Response::NoSuchDir,
        Err(_) => panic!("error should not occur for rmdir"),
    }
}

#[instrument(level = "debug", skip(directory))]
async fn open(directory: &mut Directory, path: PathString, existance: Existence) -> Response {
    match directory.open(path, existance).await {
        Ok(_lease) => todo!(),
        Err(_) => panic!("should not occur for open file"),
    }
}

#[instrument(level = "debug", skip(directory, stream))]
async fn client_conn(mut stream: ClientStream, mut directory: Directory) {
    while let Ok(msg) = stream.try_next().await {
        let msg = match msg {
            None => continue,
            Some(msg) => msg,
        };
        client_msg(&mut stream, msg, &mut directory).await;
    }
}


#[instrument(level = "debug", skip(stream, directory))]
async fn client_msg(stream: &mut ClientStream, msg: Request, directory: &mut Directory) {
    use Request::*;
    let response = match msg {
        Test => Response::Test,
        AddDir(path) => {
            mkdir(directory, path).await
        }
        RmDir(path) => {
            rmdir(directory, path).await
        }
        OpenReadWrite(path, existance) => {
            open(directory, path, existance).await
        }
        Ls(_) | OpenReadOnly(..) => Response::NotReadServ,
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
