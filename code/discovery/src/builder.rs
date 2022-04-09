use std::marker::PhantomData;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::Error;

use super::{Chart, Id};
use tokio::net::UdpSocket;

#[derive(Debug, Default)]
pub struct Yes;
#[derive(Debug, Default)]
pub struct No;

pub trait ToAssign: core::fmt::Debug {}
pub trait Assigned: ToAssign {}
pub trait NotAssigned: ToAssign {}

impl ToAssign for Yes {}
impl ToAssign for No {}

impl Assigned for Yes {}
impl NotAssigned for No {}

const DEFAULT_HEADER: u64 = 66_87_164_55_203_64_126_67;

pub struct ChartBuilder<IdSet: ToAssign> {
    header: u64,
    id: Option<Id>,
    port: u16, // port the node is listening on for work

    id_set: PhantomData<IdSet>,
}

impl ChartBuilder<No> {
    pub fn new() -> ChartBuilder<No> {
        ChartBuilder::<No> {
            header: DEFAULT_HEADER,
            id: None,
            port: 8888,
            id_set: PhantomData {},
        }
    }
    pub fn from_port(port: u16) -> ChartBuilder<No> {
        ChartBuilder {
            header: DEFAULT_HEADER,
            id: None,
            port,
            id_set: Default::default(),
        }
    }
}

impl<IdSet: ToAssign> ChartBuilder<IdSet> {
    pub fn with_id(self, id: Id) -> ChartBuilder<Yes> {
        ChartBuilder {
            header: self.header,
            id: Some(id),
            port: self.port,
            id_set: PhantomData {},
        }
    }
    pub fn with_header(mut self, header: u64) -> ChartBuilder<IdSet> {
        self.header = header;
        self
    }
}

fn open_socket(port: u16) -> Result<UdpSocket, Error> {
    assert_ne!(port, 0);

    use Error::*;
    let interface = Ipv4Addr::from([0, 0, 0, 0]);
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);

    use socket2::{Domain, SockAddr, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, None).map_err(Construct)?;
    sock.set_reuse_port(true).map_err(SetReuse)?; // allow binding to a port already in use
    sock.set_broadcast(true).map_err(SetBroadcast)?; // enable udp broadcasting
    sock.set_multicast_loop_v4(true).map_err(SetMulticast)?; // send broadcast to self

    let address = SocketAddr::from((interface, port));
    let address = SockAddr::from(address);
    sock.bind(&address).map_err(Bind)?;
    sock.join_multicast_v4(&multiaddr, &interface)
        .map_err(JoinMulticast)?;

    let sock = std::net::UdpSocket::from(sock);
    sock.set_nonblocking(true).map_err(SetNonBlocking)?;
    let sock = UdpSocket::from_std(sock).map_err(ToTokio)?;
    Ok(sock)
}

impl ChartBuilder<Yes> {
    pub fn build(self) -> Result<Chart, Error> {
        let sock = open_socket(self.port)?;
        Ok(Chart {
            header: self.header,
            id: self.id.unwrap(),
            sock: Arc::new(sock),
            map: Arc::new(dashmap::DashMap::new()),
        })
    }
}
