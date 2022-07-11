use std::{
    io::{Write, Read},
    net::TcpListener,
    sync::{Arc, Barrier},
    thread::{self, JoinHandle},
};

pub const PORT: u16 = 19744;

fn server(n_clients: usize) {
    let barrier = Arc::new(Barrier::new(n_clients));
    let listener = TcpListener::bind(("0.0.0.0", PORT)).unwrap();
    for _ in 0..n_clients {
        let c = barrier.clone();
        let (mut stream, _) = listener.accept().unwrap();
        thread::spawn(move || {
            // client rdy
            stream.read_exact(&mut[0u8]).unwrap();
            c.wait(); // all clients rdy
            stream.write(&[42u8]).unwrap();
        });
    }
}

pub struct Server(JoinHandle<()>);

pub fn start_server(n_clients: usize) -> Server {
    let handle = thread::spawn(move || server(n_clients));
    Server(handle)
}

impl Server {
    pub fn block_till_synced(self) {
        self.0.join().unwrap();
    }
}
