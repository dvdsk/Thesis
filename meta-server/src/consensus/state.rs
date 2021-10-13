use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use tokio::sync::Notify;
use tracing::{error, info, warn};

use crate::server_conn::protocol::FromRS;
use crate::Config;

#[derive(Debug)]
pub struct State {
    term: AtomicU64,
    change_idx: AtomicU64,
    candidate: AtomicBool,
    // id that recieved vote in current term, thus reset when term changes
    voted_for: AtomicU64,
    sync_voted_for: Mutex<()>,

    // normal mutex since we do no async io inside crit. sect.
    // never keep locked across an .await point! (will deadlock)
    master: Mutex<Option<IpAddr>>,
    pub config: Config,
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
    pub fn new(config: impl Into<Config>, change_idx: u64) -> Self {
        Self {
            term: AtomicU64::new(0),
            change_idx: AtomicU64::new(change_idx),
            candidate: AtomicBool::new(false),
            voted_for: AtomicU64::new(0),
            sync_voted_for: Mutex::new(()),
            master: Mutex::new(None),
            config: config.into(),
            got_valid_hb: Notify::new(),
            outdated: Notify::new(),
        }
    }

    load_atomic!(term, u64);
    load_atomic!(change_idx, u64);
    load_atomic!(voted_for, u64);
    load_atomic!(candidate, bool);

    fn reset_voted_for(&self) {
        self.voted_for.store(0, ORD);
    }

    pub fn set_candidate(&self) {
        self.candidate.store(true, ORD)
    }

    pub fn set_follower(&self) {
        self.candidate.store(false, ORD)
    }

    /// may only be called by master and only from the
    /// readservers publish function
    pub fn increase_change_idx(&self) -> u64 {
        let prev = self.change_idx.fetch_add(1, ORD);
        prev+1
    }

    pub fn increase_term(&self) -> u64 {
        debug_assert!(
            self.candidate.load(ORD),
            "can only safely be used if we are a candidate or master,
            in which case candidate should still be true otherwise this 
            can race with resetting voted_for"
        );
        self.term.fetch_add(1, ORD)
    }

    pub fn set_term(&self, val: u64) {
        self.term.store(val, ORD)
    }

    pub fn get_master(&self) -> Option<IpAddr> {
        *self.master.lock().unwrap()
    }

    pub fn set_master(&self, source: SocketAddr) {
        let mut curr = self.master.lock().unwrap();
        let source = source.ip();
        if *curr != Some(source) {
            info!("new master: {}", source);
        }
        *curr = Some(source);
    }

    #[tracing::instrument(skip(self))]
    fn check_term(&self, term: u64, source: SocketAddr) -> Result<(), ()> {
        loop {
            let our_term = self.term();
            if term < our_term {
                warn!("ignoring hb from main: {:?}, term to old)", source);
                return Err(());
            }
            if term >= our_term {
                let res = self.term.compare_exchange(our_term, term, ORD, ORD);
                if res.is_err() {
                    continue; // term changed out under us, run procedure again
                }
                self.set_master(source)
            }
            break;
        }
        self.got_valid_hb.notify_one();
        Ok(())
    }

    #[tracing::instrument(skip(self))]
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

    #[tracing::instrument(skip(self))]
    pub fn handle_dirchange(
        &self,
        term: u64,
        change_idx: u64,
        source: SocketAddr,
    ) -> Result<(), ()> {
        self.check_term(term, source)?;
        let res = self
            .change_idx
            .compare_exchange(change_idx - 1, change_idx, ORD, ORD);
        // TODO need to also update db change_idx

        if let Err(curr_val) = res {
            self.outdated.notify_one();
            error!(
                "change idx ({:?}) is not what we expected ({:?}), we are outdated!",
                curr_val, change_idx
            );
            return Err(());
        }

        Ok(())
    }

    // keep lock out of async sections (deadlock)
    fn update_if_later_term(&self, term: u64) {
        // this needs to be synchornized, else the following could happen
        //      A increases term -> B increases term further -> B resets voted -> B votes
        //      -> A reset vote, now the vote to B is no longer counted
        let _guard = self.sync_voted_for.lock().unwrap();
        if term > self.term() {
            self.set_term(term);
            self.reset_voted_for();
        }
    }

    #[tracing::instrument(skip(self))]
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

        self.update_if_later_term(term);

        let res = self.voted_for.compare_exchange(0, id, ORD, ORD);
        match res {
            Ok(_) => FromRS::VotedForYou(term),
            Err(voted_for) if voted_for == id => FromRS::VotedForYou(term),
            Err(_) => {
                info!("not voting for you");
                FromRS::NotVoting
            }
        }
    }
}
