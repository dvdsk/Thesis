use client::{WriteServer, Conn, Existence, ServerList, WriteableFile};

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

#[tokio::main]
async fn main() {
    let conn = WriteServer::from_serverlist(test_serverlist()).await.unwrap();
    let _f = WriteableFile::open(conn, "testfile", Existence::Forbidden).await.unwrap();
}
