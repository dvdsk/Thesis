use std::collections::HashSet;
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::raft::subjects::{Source, SourceNotify};
use crate::redirectory::{ClientAddr, MinisterAddr, Node};
use crate::Id;

// TODO need a way to drop subjects too
pub struct RaftMap {
    our_id: Id,
    current: HashSet<Node>,
    recv: Option<mpsc::Receiver<(Id, MinisterAddr)>>,
}

pub struct RegistrationNotify(mpsc::Receiver<(Id, MinisterAddr)>);

#[async_trait::async_trait]
impl SourceNotify for RegistrationNotify {
    type Error = &'static str;
    async fn recv_new(&mut self) -> Result<(Id, SocketAddr), Self::Error> {
        // TODO cherk if current is needed/ should be updated / existing update
        // code moved inside here
        self.0
            .recv()
            .await
            .map(|(id, addr)| (id, addr.untyped()))
            .ok_or("Clerk registration channel was closed")
    }
}

impl Source for RaftMap {
    type Notify = RegistrationNotify;

    fn notify(&mut self) -> Self::Notify {
        RegistrationNotify(self.recv.take().unwrap())
    }
    fn our_id(&self) -> Id {
        self.our_id
    }
    /// TODO this is used in read lock management but wont work there
    /// because read lock management needs the `client_addr`. To solve
    /// this: make map generic over addr then wrap map again with generics recolved ?
    fn adresses(&mut self) -> Vec<(Id, SocketAddr)> {
        self.current
            .iter()
            .cloned()
            .map(|c| (c.id, c.minister_addr().untyped()))
            .collect()
    }
    fn forget_impl(&self, _id: Id) {
        unimplemented!()
    }
}

type ClientAddrNotify = mpsc::Receiver<(Id, ClientAddr)>;
pub struct ClientAddrMap {
    current: HashSet<Node>,
    recv: Option<mpsc::Receiver<(Id, ClientAddr)>>,
}
// TODO split off to seperate map obj that does NOT implement Source
// TODO reciever will be a type alias for Reciever and not a RegistrationNotify
impl ClientAddrMap {
    pub fn adresses(&self) -> Vec<(Id, ClientAddr)> {
        self.current
            .iter()
            .cloned()
            .map(|c| (c.id, c.client_addr()))
            .collect()
    }

    pub fn notify(&mut self) -> ClientAddrNotify {
        self.recv.take().unwrap()
    }
}

pub struct Register {
    current: HashSet<Node>,
    raft_notify: mpsc::Sender<(Id, MinisterAddr)>,
    lock_notify: mpsc::Sender<(Id, ClientAddr)>,
}

impl Register {
    pub(super) fn update(&mut self, new_assignment: Vec<Node>) {
        let new_assignment: HashSet<Node> = new_assignment.into_iter().collect();
        let newly_added = new_assignment.difference(&self.current);
        for clerk in newly_added {
            self.raft_notify
                .try_send((clerk.id, clerk.minister_addr()))
                .unwrap();
            self.lock_notify
                .try_send((clerk.id, clerk.client_addr()))
                .unwrap();
        }
    }
}

impl RaftMap {
    pub(crate) fn new(clerks: Vec<Node>, our_id: Id) -> (Register, RaftMap, ClientAddrMap) {
        let current: HashSet<_> = clerks.into_iter().collect();

        let (raft_notify, rx) = mpsc::channel(16);
        let map_a = Self {
            our_id,
            current: current.clone(),
            recv: Some(rx),
        };

        let (lock_notify, rx) = mpsc::channel(16);
        let map_b = ClientAddrMap {
            current: current.clone(),
            recv: Some(rx),
        };

        let register = Register {
            current,
            raft_notify,
            lock_notify,
        };
        (register, map_a, map_b)
    }
}
