use instance_chart::{discovery, Chart, ChartBuilder};
use rand::prelude::*;
use std::net::SocketAddr;
use tokio::task::JoinSet;

#[async_trait::async_trait]
pub trait RandomNode {
    async fn random_node(&self) -> SocketAddr;
}

pub struct ChartNodes<const N: usize, const IDX: usize> {
    chart: Chart<N, u16>,
    _discovery_task: JoinSet<()>,
}

impl<const N: usize, const IDX: usize> ChartNodes<N, IDX> {
    pub fn new(discovery_port: u16) -> Self {
        let chart = ChartBuilder::new()
            .with_id(9999999)
            .with_service_ports([0u16; N])
            .with_discovery_port(discovery_port)
            .local_discovery(true)
            .finish()
            .unwrap();
        let mut _discovery_task = JoinSet::new();
        _discovery_task
            .build_task()
            .name("sniff cluster")
            .spawn(discovery::sniff(chart.clone()));

        Self {
            chart,
            _discovery_task,
        }
    }
}

#[async_trait::async_trait]
impl<const N: usize, const IDX: usize> RandomNode for ChartNodes<N, IDX> {
    async fn random_node(&self) -> SocketAddr {
        let mut notify = self.chart.notify(); // needed in case chart is empty
        let adresses = self.chart.nth_addr_vec::<IDX>();

        let addr = {
            let mut rng = rand::thread_rng();
            adresses.into_iter().choose(&mut rng).map(|(_, addr)| addr)
        };

        if let Some(addr) = addr {
            return addr;
        }

        notify
            .recv_nth_addr::<IDX>()
            .await
            .map(|(_, addr)| addr)
            .unwrap()
    }
}
