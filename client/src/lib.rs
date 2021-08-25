use std::io::Result as IoResult;
use std::io::{Read, Write};
use std::path::PathBuf;

mod connection;
use protocol::Request;
pub use connection::{WriteServer, ReadServer, Conn};
pub use protocol::{ ServerList, Existence };

mod builder;

// TODO split into Writable and ReadOnly
pub struct WriteableFile {
    meta_conn: WriteServer,
}

pub struct ReadOnlyFile {
    meta_conn: ReadServer,
}

impl WriteableFile {
    pub async fn open(
        conn: impl Into<WriteServer>,
        path: impl Into<PathBuf>,
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
        path: impl Into<PathBuf>,
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
