use std::io::Result as IoResult;
use std::io::{Read, Write};

mod connection;
use protocol::{Request, Response};
pub use connection::{WriteServer, ReadServer, Conn};
pub use protocol::{ ServerList, Existence, PathString, FsEntry };

mod builder;

// TODO split into Writable and ReadOnly
pub struct WriteableFile {
    meta_conn: WriteServer,
}

pub struct ReadOnlyFile {
    meta_conn: ReadServer,
}

pub async fn ls(conn: &mut ReadServer, path: impl Into<PathString>) -> Vec<FsEntry> {
    let res = conn.request(Request::Ls(path.into())).await.unwrap();
    match res {
        Response::Ls(list) => return list,
        _ => panic!("ls request should be awnsered with ls response"),
    }
}

pub async fn mkdir(conn: &mut WriteServer, path: impl Into<PathString>) {
    conn.request(Request::AddDir(path.into())).await.unwrap();
}

pub async fn rmdir(conn: &mut WriteServer, path: impl Into<PathString>) {
    conn.request(Request::RmDir(path.into())).await.unwrap();
}

impl WriteableFile {
    pub async fn open(
        conn: impl Into<WriteServer>,
        path: impl Into<PathString>,
        existance: Existence,
    ) -> Result<Self, ()> {
        let mut conn = conn.into();
        conn.request(Request::OpenReadWrite(path.into(), existance))
            .await.unwrap();
        Ok(WriteableFile { meta_conn: conn })
    }
}

impl ReadOnlyFile {
    pub async fn open(
        conn: impl Into<ReadServer>,
        path: impl Into<PathString>,
        existance: Existence,
    ) -> Result<Self, ()> {
        let mut conn = conn.into();
        conn.request(Request::OpenReadOnly(path.into(), existance))
            .await.unwrap();
        Ok(ReadOnlyFile { meta_conn: conn })
    }
}

impl Read for ReadOnlyFile {
    fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
        todo!();
    }
}

impl Write for WriteableFile {
    fn write(&mut self, _buf: &[u8]) -> IoResult<usize> {
        todo!()
    }
    fn flush(&mut self) -> IoResult<()> {
        todo!()
    }
}
