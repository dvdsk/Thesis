use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::{timeout_at, Instant};
use tracing::{info, warn};

use crate::president::Chart;

const HB_TIMEOUT: Duration = Duration::from_millis(100);

pub(super) async fn president_died(heartbeat: &Notify) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();

    loop {
        let random_dur = rng.gen_range(Duration::from_secs(0)..HB_TIMEOUT);
        let hb_deadline = Instant::now() + HB_TIMEOUT + random_dur;
        match timeout_at(hb_deadline, heartbeat.notified()).await {
            Err(elapsed) => {
                warn!("heartbeat timed out, elapsed without hb: {:?}", elapsed);
                return;
            }
            Ok(_) => {
                info!(
                    "hb timeout in {} ms",
                    hb_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                );
            }
        }
    }
}

/// only returns when this node has been elected
pub(super) async fn run_for_office(chart: Chart) {
    todo!()
}
