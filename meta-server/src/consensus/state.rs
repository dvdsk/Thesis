use std::sync::atomic::AtomicU64;

use tokio::sync::Notify;


#[derive(Debug)]
pub struct State<'a> {
    pub term: AtomicU64,
    pub change_idx: u64,
    cluster_size: u16,
    candidate: bool,
    pub chart: &'a discovery::Chart,
    pub got_valid_hb: Notify,
    pub outdated: Notify,
}

impl<'a> State<'a> {
    pub fn new(cluster_size: u16, chart: &'a discovery::Chart, change_idx: u64) -> Self {
        Self {
            term: AtomicU64::new(0),
            change_idx,
            cluster_size,
            candidate: false,
            chart,
            got_valid_hb: Notify::new(),
            outdated: Notify::new(),
        }
    }
    pub fn is_majority(&self, votes: usize) -> bool {
        let cluster_majority = (self.cluster_size as f32 * 0.5).ceil() as usize;
        votes > cluster_majority
    }

    pub fn handle_heartbeat(&self) {
        // - check term and change_idx, update master
        // - notify outdated if our change_idx to low 
        // - notify the heartbeat watcher we still have a leader
        //   in an election this ^ wil notify the election watcher we 
        //   should stop our election as someone else won
    }

    pub fn handle_votereq(&self) {
        // check term and change_idx
        // check if we are not an election candidate
        // do vote 
    }
}
