use color_eyre::{eyre, Help, Report};
use instance_chart::Id;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use tokio::sync::{mpsc, Notify};
use tracing::{debug, instrument};

use crate::{Idx, Term};

pub use self::append::LogEntry;
use self::vote::ElectionOffice;

use super::{Order, HB_PERIOD};
type LogIdx = u32;

pub mod append;
pub mod vote;

#[derive(Debug)]
struct Vars {
    last_applied: AtomicU32,
    commit_index: AtomicU32,
    heartbeat: Notify,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            last_applied: AtomicU32::new(0),
            commit_index: AtomicU32::new(0),
            heartbeat: Default::default(),
        }
    }
}

mod db {
    #[repr(u8)]
    pub enum Prefix {
        /// reserved to allow unwrap_or_default to reduce no key found Option
        /// to an unused key_prefix.
        #[allow(dead_code)]
        Invalid = 0,
        /// contains last seen term and voted_for
        ElectionData = 1,
        /// keys are 5 bytes: [this prefix, u32 as_ne_bytes],
        /// entries are 4 bytes term as_ne_bytes + byte serialized order,
        Log = 2,
    }

    impl From<&u8> for Prefix {
        fn from(prefix: &u8) -> Self {
            use Prefix::*;
            match *prefix {
                x if x == ElectionData as u8 => ElectionData,
                x if x == Log as u8 => Log,
                _ => Invalid,
            }
        }
    }
    pub const ELECTION_DATA: [u8; 1] = [Prefix::ElectionData as u8];
    #[allow(dead_code)]
    pub const LOG: [u8; 1] = [Prefix::Log as u8];
    pub fn log_key(idx: u32) -> [u8; 5] {
        let mut key = [Prefix::Log as u8, 0, 0, 0, 0];
        key[1..5].clone_from_slice(&idx.to_be_bytes());
        key
    }
}

#[derive(Debug, Clone)]
enum OrderAge {
    Fresh { process_by: Instant },
    Old,
}

#[derive(Debug, Clone)]
pub struct Perishable<O> {
    pub order: O,
    age: OrderAge,
}

impl<O: std::fmt::Debug> Perishable<O> {
    pub fn new_fresh(order: O, process_by: Instant) -> Perishable<O> {
        Perishable {
            order,
            age: OrderAge::Fresh { process_by },
        }
    }

    #[instrument(skip_all, fields(_order, _time_left))]
    pub fn perished(&self) -> bool {
        let process_by = match self.age {
            OrderAge::Old => return true,
            OrderAge::Fresh { process_by } => process_by,
        };

        let _time_left = process_by.saturating_duration_since(Instant::now());
        let _order = format!("{:?}", self.order);
        !process_by.elapsed().is_zero()
    }

    pub fn error(&self) -> Report {
        let elapsed_note = match self.age {
            OrderAge::Fresh { process_by } => format!("{:?} too late", process_by.elapsed()),
            OrderAge::Old => String::from("Old order"),
        };
        eyre::eyre!("order processed too slow")
            .note(elapsed_note)
            .with_note(|| format!("order was: {:?}", self.order))
    }
}

#[derive(Debug, Clone)]
pub struct State<O> {
    pub election_office: Arc<Mutex<ElectionOffice>>,
    /// sends orders to ObserverLog or Log where they can be consumed
    /// should be consumed within one HB period or state becomes inconsistent
    tx: mpsc::Sender<Perishable<O>>,
    db: sled::Tree,
    vars: Arc<Vars>,
}

impl<O: Order> State<O> {
    pub fn new(tx: mpsc::Sender<Perishable<O>>, db: sled::Tree) -> Self {
        let election_office = ElectionOffice::from_tree(&db);
        election_office.init_election_data();
        let state = Self {
            election_office: Arc::new(Mutex::new(election_office)),
            tx,
            db,
            vars: Default::default(),
        };
        // append_msg committed index starts at zero, insert a zeroth
        // log entry that can safely be committed
        state.insert_into_log(0, &append::LogEntry::default());
        state
    }

    /// Only call when switching subtree in an observerlog
    pub(super) fn reset(&mut self) {
        self.vars.last_applied.store(0, Ordering::SeqCst); 
        self.vars.commit_index.store(0, Ordering::SeqCst); 
        self.db.clear().unwrap();
        self.insert_into_log(0, &append::LogEntry::default());
    }

    /// returns if term increased
    #[instrument(skip_all)]
    pub(crate) async fn watch_term(&self) {
        let mut sub = self.db.watch_prefix(db::ELECTION_DATA);
        while let Some(event) = (&mut sub).await {
            match event {
                sled::Event::Insert { key, value } if key == db::ELECTION_DATA => {
                    let data: vote::ElectionData = bincode::deserialize(&value).unwrap();
                    debug!("saw term increase, it is now: {}", data.term());
                    return;
                }
                _ => panic!("term key should never be removed"),
            }
        }
        unreachable!("db subscriber should never be dropped")
    }

    pub(super) async fn increment_term(&self) -> u32 {
        self.election_office.lock().await.increment_term()
    }

    pub fn heartbeat(&self) -> &Notify {
        &self.vars.heartbeat
    }

    /// vote for self, returns false if
    /// a vote was already cast for this term
    #[instrument(skip(self), ret)]
    pub async fn vote_for_self(&self, term: Term, id: Id) -> bool {
        let election_office = self.election_office.lock().await;
        election_office.set_voted_for(term, id)
    }
}

#[derive(Debug, Default)]
pub struct LogMeta {
    pub term: u32,
    pub idx: u32,
}

impl<O: Order> State<O> {
    /// for an empty log return LogMeta{ term: 0, idx: 0 }
    #[instrument(level = "trace", skip(self), ret)]
    pub(crate) fn last_log_meta(&self) -> LogMeta {
        use db::Prefix;

        let max_key = [u8::MAX];
        let (key, value) = self
            .db
            .get_lt(max_key) 
            .expect("internal db issue")
            .unwrap_or_default();

        match key.first().map(Prefix::from).unwrap_or(Prefix::Invalid) {
            Prefix::Log => LogMeta {
                idx: u32::from_be_bytes(key[1..5].try_into().unwrap()),
                term: u32::from_ne_bytes(value[0..4].try_into().unwrap()),
            },
            _ => Default::default(),
        }
    }

    #[instrument(skip_all, level = "trace")]
    pub(crate) fn entry_at(&self, idx: u32) -> Option<LogEntry<O>> {
        use crate::util::TypedSled;
        self.db.get_val(db::log_key(idx))
    }

    pub(crate) fn append(&self, order: O, term: Term) -> Idx {
        use crate::util::TypedSled;
        loop {
            let LogMeta { idx, .. } = self.last_log_meta();
            let entry = LogEntry {
                term,
                order: order.clone(),
            };
            let res = self.db.set_unique(db::log_key(idx + 1), entry);
            if res.is_ok() {
                break idx + 1;
            }
        }
    }

    /// insert an order to the user facing facade (Log or ObserverLog)
    /// this order need not have come through the raft log
    pub(super) async fn insert_unlogged_order(&self, ord: O) {
        let unlogged = Perishable::new_fresh(ord, Instant::now() + HB_PERIOD);
        self.tx.send(unlogged).await.unwrap();
    }

    pub(crate) fn committed(&self) -> Vec<O> {
        let last = db::log_key(self.commit_index());
        self.db
            .range(db::log_key(0)..last)
            .map(|r| r.expect("the log is empty, which should not be possible"))
            .map(|(_, value)| value)
            .map(|bytes| bincode::deserialize::<LogEntry<O>>(&bytes))
            .map(|r| r.expect("decoding the bytes failed")) // panics!
            .map(|entry| entry.order)
            .collect()
    }

    pub(crate) fn is_committed(&self, idx: u32) -> bool {
        self.commit_index() >= idx
    }
}
