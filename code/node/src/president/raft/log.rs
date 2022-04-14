use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use tokio::net::TcpSocket;

pub struct Log {
    db: sled::Tree,
}

impl Log {
    pub(crate) fn open(db: sled::Db, sock: TcpSocket) -> Result<Self> {
        Ok(Log {
            db: db
                .open_tree("president log")
                .wrap_err("Could not open db tree: \"president log\"")?,
        })
    }
}
