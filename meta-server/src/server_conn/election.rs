use tracing::info;

pub enum ElectionResult {
    Winner,
    Loser,
}

pub async fn maintain_heartbeat() {
}

pub async fn monitor_heartbeat() {
}

pub async fn host_election(port: u16) -> ElectionResult {
    info!("hosting leader election");
    ElectionResult::Loser
}
