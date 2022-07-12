use std::sync::Arc;
use std::time::Duration;

use color_eyre::Result;
use node::WrapErr;

use tokio::net::TcpStream;
use tokio::sync::Barrier;
use tokio::task::{self, JoinSet};
use tokio::time::timeout;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub const PORT: u16 = 19744;

async fn handle_conn(mut stream: TcpStream, barrier: Arc<Barrier>) {
    // client rdy
    stream.read_exact(&mut [0u8]).await.unwrap();
    barrier.wait().await; // all clients rdy
    stream.write_all(&[42u8]).await.unwrap();
}

async fn run_server(n_clients: usize) {
    let barrier = Arc::new(Barrier::new(n_clients));
    let listener = TcpListener::bind(("0.0.0.0", PORT)).await.unwrap();
    let mut tasks = JoinSet::new();
    for _ in 0..n_clients {
        let c = barrier.clone();
        let (stream, _) = listener.accept().await.unwrap();
        tasks.spawn(handle_conn(stream, c));
    }
}

pub struct Server(task::JoinHandle<()>);

pub fn server(n_clients: usize) -> Server {
    let handle = task::spawn(run_server(n_clients));
    Server(handle)
}

impl Server {
    pub async fn block_till_synced(self) -> Result<()> {
        timeout(Duration::from_secs(5), self.0)
            .await
            .wrap_err("sync server timeout")?
            .wrap_err("sync server crash")?;
        Ok(())
    }
}
