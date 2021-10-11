use client::FsEntry;
use client::{ls, mkdir, rmdir, Conn, ReadServer, ServerList, WriteServer};

fn serverlist_from_args() -> ServerList {
    let mut args = std::env::args();
    let port = args.nth(1).unwrap().parse().unwrap();
    ServerList {
        port,
        write_serv: None,
        read_serv: None,
        fallback: args.map(|a| a.parse().unwrap()).collect(),
    }
}

fn setup_tracing() {
    use tracing_subscriber::FmtSubscriber;
    let _subscriber = FmtSubscriber::builder().try_init().unwrap();
}

#[tokio::main]
async fn main() {
    setup_tracing();
    let list = serverlist_from_args();

    let mut wconn = WriteServer::from_serverlist(list.clone()).await.unwrap();
    let mut rconn = ReadServer::from_serverlist(list).await.unwrap();

    mkdir(&mut wconn, "another_dir").await;
    mkdir(&mut wconn, "test_dir").await;
    mkdir(&mut wconn, "test_dir/subdir").await;
    let res = ls(&mut rconn, "").await;
    assert_eq!(
        res,
        vec![
            FsEntry::Dir("another_dir".to_string()),
            FsEntry::Dir("test_dir".to_string()),
            FsEntry::Dir("test_dir/subdir".to_string())
        ]
    );
    dbg!(res);

    rmdir(&mut wconn, "test_dir").await;
    let res = ls(&mut rconn, "").await;
    assert_eq!(
        res,
        vec![
            FsEntry::Dir("another_dir".to_string()),
        ]
    );
    dbg!(res);
}
