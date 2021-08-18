use std::io::{Read, Write};
use std::io::Result as IoResult;
use std::path::PathBuf;

mod connection;
pub use connection::Conn;

mod builder;
pub use builder::OpenOptions;

// TODO split into Writable and ReadOnly
pub struct File {
    meta_conn: Conn,
}

impl File {
    /// attempts to open a file in read-only mode.
    ///
    /// see the [`OpenOptions::open`] method for details.
    pub fn open(conn: impl Into<Conn>, path: impl Into<PathBuf>) -> Result<Self, ()> {
        OpenOptions::new().read(true).open(conn, path)
    }
}

impl Read for File {
    fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
        todo!();
    }
}

impl Write for File {
    fn write(&mut self, _buf: &[u8]) -> IoResult<usize> {
        todo!()
    }
    fn flush(&mut self) -> IoResult<()> {
        todo!()
    }
}
