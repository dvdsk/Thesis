use std::net::SocketAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Notify;
use tracing::{info, warn};

use crate::server_conn::protocol::FromRS;

#[derive(Debug)]
pub struct State {
    term: AtomicU64,
    change_idx: AtomicU64,
    master: Mutex<Option<SocketAddr>>, // normal mutex since we do no async io inside crit. sect.
    cluster_size: u16,
    candidate: AtomicBool,
    pub got_valid_hb: Notify,
    pub outdated: Notify,
}

macro_rules! load_atomic {
    ($member_name:ident, $returns:ty) => {
        pub fn $member_name(&self) -> $returns {
            self.$member_name.load(Ordering::Relaxed)
        }
    };
}

impl State {
    pub fn new(cluster_size: u16, change_idx: u64) -> Self {
        Self {
            term: AtomicU64::new(0),
            change_idx: AtomicU64::new(change_idx),
            master: Mutex::new(None),
            cluster_size,
            candidate: AtomicBool::new(false),
            got_valid_hb: Notify::new(),
            outdated: Notify::new(),
        }
    }

    load_atomic!(term, u64);
    load_atomic!(change_idx, u64);
    load_atomic!(candidate, bool);

    pub fn set_candidate(&self) {
        self.candidate.store(true, Ordering::Relaxed)
    }

    pub fn set_follower(&self) {
        self.candidate.store(false, Ordering::Relaxed)
    }

    pub fn increase_term(&self) {
        self.term.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_term(&self, val: u64) {
        self.term.store(val, Ordering::Relaxed)
    }

    pub fn is_majority(&self, votes: usize) -> bool {
        let cluster_majority = (self.cluster_size as f32 * 0.5).ceil() as usize;
        votes > cluster_majority
    }

    pub fn get_master(&self) -> Option<SocketAddr> {
        *self.master.lock().unwrap()
    }

    fn check_term(&self, term: u64, source: SocketAddr) -> Result<(), ()> {
        if term < self.term() {
            warn!("ignoring hb from main: {:?}, term to old)", source);
            return Err(());
        }
        if term > self.term() {
            self.set_term(term);
            info!("new master: {:?}", &source);
            *self.master.lock().unwrap() = Some(source);
            todo!("actually update master");
        }
        self.got_valid_hb.notify_one();
        Ok(())
    }

    pub fn handle_heartbeat(&self, term: u64, change_idx: u64, source: SocketAddr) {
        if let Err(_) = self.check_term(term, source) {
            return;
        }
        assert!(
            change_idx >= self.change_idx(),
            "master change idx should never be lower!"
        );
        if change_idx > self.change_idx() {
            self.outdated.notify_one();
            return;
        }
    }

    pub fn handle_dirchange(
        &self,
        term: u64,
        change_idx: u64,
        source: SocketAddr,
    ) -> Result<(), ()> {
        self.check_term(term, source)?;
        let res = self.change_idx.compare_exchange(
            change_idx - 1,
            change_idx,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );

        if let Err(_) = res {
            self.outdated.notify_one();
            return Err(())
        }

        Ok(())
    }

    pub fn handle_votereq(&self, term: u64, change_idx: u64) -> FromRS {
        if self.term() > term {
            return FromRS::NotVoting;
        }

        if self.change_idx() > change_idx {
            return FromRS::NotVoting;
        }

        match self.candidate() {
            true => FromRS::VotedForYou(term),
            false => FromRS::NotVoting,
        }
    }
}
