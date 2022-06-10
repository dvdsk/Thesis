use std::collections::HashSet;
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::redirectory::Node;
use crate::raft::subjects::{Source, SourceNotify};
use crate::Id;

// TODO need a way to drop subjects too
pub struct Map {
    our_id: Id,
    current: HashSet<Node>,
    recv: Option<mpsc::Receiver<(Id, SocketAddr)>>,
}

pub struct RegistrationNotify(mpsc::Receiver<(Id, SocketAddr)>);

#[async_trait::async_trait]
impl SourceNotify for RegistrationNotify {
    type Error = &'static str;
    async fn recv_new(&mut self) -> Result<(Id, SocketAddr), Self::Error> {
        self.0
            .recv()
            .await
            .ok_or("Clerk registration channel was closed")
    }
}

impl Source for Map {
    type Notify = RegistrationNotify;

    fn notify(&mut self) -> Self::Notify {
        RegistrationNotify(self.recv.take().unwrap())
    }
    fn our_id(&self) -> Id {
        self.our_id
    }
    fn adresses(&mut self) -> Vec<(Id, SocketAddr)> {
        self.current
            .iter()
            .cloned()
            .map(|c| (c.id, c.addr))
            .collect()
    }
}

pub struct Register {
    current: HashSet<Node>,
    notify: mpsc::Sender<(Id, SocketAddr)>,
}

impl Register {
    pub(super) fn update(&mut self, new_assignment: Vec<Node>) {
        let new_assignment: HashSet<Node> = new_assignment.into_iter().collect();
        let newly_added = new_assignment.difference(&self.current);
        for clerk in newly_added {
            self.notify
                .try_send((clerk.id, clerk.addr.clone()))
                .unwrap();
        }
    }
}

impl Map {
    pub(crate) fn new(clerks: Vec<Node>, our_id: Id) -> (Register, Map) {
        let (notify, rx) = mpsc::channel(16);
        let current: HashSet<_> = clerks.into_iter().collect();
        let map = Self {
            our_id,
            current: current.clone(),
            recv: Some(rx),
        };
        let register = Register {
            current,
            notify,
        };
        (register, map)
    }
}
