use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use tokio::sync::Notify;
use tracing::{info, warn};

use crate::server_conn::protocol::FromRS;

#[derive(Debug)]
pub struct State {
    term: AtomicU64,
    change_idx: AtomicU64,
    candidate: AtomicBool,
    voted_for: AtomicU64,
    master: Mutex<Option<SocketAddr>>, // normal mutex since we do no async io inside crit. sect.
    pub cluster_size: u16,
    pub got_valid_hb: Notify,
    pub outdated: Notify,
}

const ORD: Ordering = Ordering::SeqCst;

macro_rules! load_atomic {
    ($member_name:ident, $returns:ty) => {
        pub fn $member_name(&self) -> $returns {
            self.$member_name.load(ORD)
        }
    };
}

impl State {
    pub fn new(cluster_size: u16, change_idx: u64) -> Self {
        Self {
            term: AtomicU64::new(0),
            change_idx: AtomicU64::new(change_idx),
            candidate: AtomicBool::new(false),
            voted_for: AtomicU64::new(0),
            master: Mutex::new(None),
            cluster_size,
            got_valid_hb: Notify::new(),
            outdated: Notify::new(),
        }
    }

    load_atomic!(term, u64);
    load_atomic!(change_idx, u64);
    load_atomic!(voted_for, u64);
    load_atomic!(candidate, bool);

    pub fn reset_voted_for(&self) {
        self.voted_for.store(0, ORD);
    }

    pub fn set_candidate(&self) {
        self.candidate.store(true, ORD)
    }

    pub fn set_follower(&self) {
        self.candidate.store(false, ORD)
    }

    pub fn increase_term(&self) {
        self.term.fetch_add(1, ORD);
    }

    pub fn set_term(&self, val: u64) {
        self.term.store(val, ORD)
    }

    pub fn get_master(&self) -> Option<SocketAddr> {
        *self.master.lock().unwrap()
    }

    fn check_term(&self, term: u64, source: SocketAddr) -> Result<(), ()> {
        loop {
            let our_term = self.term();
            if term < our_term {
                warn!("ignoring hb from main: {:?}, term to old)", source);
                return Err(());
            }
            if term > our_term {
                let res = self.term.compare_exchange(
                    our_term,
                    term,
                    ORD,
                    ORD,
                );
                if res.is_err() {
                    continue; // term changed out under us, run procedure again
                }
                info!("new master: {:?}", &source);
                self.reset_voted_for();
                *self.master.lock().unwrap() = Some(source);
            }
            break;
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
            ORD,
            ORD,
        );

        if let Err(_) = res {
            self.outdated.notify_one();
            return Err(());
        }

        Ok(())
    }

    pub fn handle_votereq(&self, term: u64, change_idx: u64, id: u64) -> FromRS {
        if self.term() > term {
            return FromRS::NotVoting;
        }

        if self.change_idx() > change_idx {
            return FromRS::NotVoting;
        }

        if self.candidate() {
            return FromRS::NotVoting;
        }

        let res = self
            .voted_for
            .compare_exchange(0, id, ORD, ORD);
        match res {
            Ok(_) => FromRS::VotedForYou(term),
            Err(voted_for) if voted_for == id => FromRS::VotedForYou(term),
            Err(_) => FromRS::NotVoting,
        }
    }
}
