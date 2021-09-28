use client::{Conn, Existence, ServerList, WriteServer, WriteableFile};

fn test_serverlist() -> ServerList {
    ServerList {
        read_serv: "127.0.0.1:8081".parse().unwrap(),
        write_serv: "127.0.0.1:8082".parse().unwrap(),
        fallback: vec![
            "127.0.0.1:8083".parse().unwrap(),
            "127.0.0.1:8084".parse().unwrap(),
            "127.0.0.1:8085".parse().unwrap(),
        ],
    }
}

fn serverlist_from_args() -> ServerList {
    let mut args = std::env::args();
    ServerList {
        read_serv: args.next().parse().unwrap(),
        write_serv: args.next().parse().unwrap(),
        fallback: args.map(|s| s.parse()).collect(),
    }
}

#[tokio::main]
async fn main() {

    let list = serverlist_from_args();
    let wconn = WriteServer::from_serverlist(list)
        .await
        .unwrap();
    let _f = WriteableFile::open(conn, "testfile", Existence::Forbidden)
        .await
        .unwrap();
}
