use std::marker::PhantomData;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use crate::Error;

use super::{interval, Chart, Id};
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
const DEFAULT_PORT: u16 = 8080;

#[derive(Default)]
pub struct ChartBuilder<IdSet, PortSet>
where
    IdSet: ToAssign,
    PortSet: ToAssign,
{
    header: u64,
    service_id: Option<Id>,
    service_port: Option<u16>,
    discovery_port: u16, // port the node is listening on for work
    rampdown: interval::Params,
    id_set: PhantomData<IdSet>,
    port_set: PhantomData<PortSet>,
}

impl ChartBuilder<No, No> {
    pub fn new() -> ChartBuilder<No, No> {
        let mut builder = ChartBuilder::default();
        builder.header = DEFAULT_HEADER;
        builder.discovery_port = DEFAULT_PORT;
        builder
    }
}

impl<IdSet, PortSet> ChartBuilder<IdSet, PortSet>
where
    IdSet: ToAssign,
    PortSet: ToAssign,
{
    pub fn with_id(self, id: Id) -> ChartBuilder<Yes, PortSet> {
        ChartBuilder {
            header: self.header,
            discovery_port: self.discovery_port,
            service_id: Some(id),
            service_port: self.service_port,
            rampdown: self.rampdown,
            id_set: PhantomData {},
            port_set: PhantomData {},
        }
    }
    pub fn with_service_port(self, service_port: u16) -> ChartBuilder<IdSet, Yes> {
        ChartBuilder {
            header: self.header,
            discovery_port: self.discovery_port,
            service_id: self.service_id,
            service_port: Some(service_port),
            rampdown: self.rampdown,
            id_set: PhantomData {},
            port_set: PhantomData {},
        }
    }
    pub fn with_header(mut self, header: u64) -> ChartBuilder<IdSet, PortSet> {
        self.header = header;
        self
    }
    pub fn with_discovery_port(mut self, port: u16) -> ChartBuilder<IdSet, PortSet> {
        self.discovery_port = port;
        self
    }
    pub fn with_rampdown(
        mut self,
        min: Duration,
        max: Duration,
        rampdown: Duration,
    ) -> ChartBuilder<IdSet, PortSet> {
        assert!(min < max);
        self.rampdown = interval::Params { min, max, rampdown };
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

impl ChartBuilder<Yes, Yes> {
    pub fn build(self) -> Result<Chart, Error> {
        let sock = open_socket(self.discovery_port)?;
        Ok(Chart {
            header: self.header,
            service_id: self.service_id.unwrap(),
            service_port: self.service_port.unwrap(),
            sock: Arc::new(sock),
            map: Arc::new(dashmap::DashMap::new()),
            interval: self.rampdown.into(),
        })
    }
}
