use futures::stream;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Notify};
use tracing::{debug, instrument};

use crate::raft::{Order, State, HB_PERIOD};
use crate::Idx;

struct Waiters {
    new: mpsc::Receiver<(Idx, Arc<Notify>)>,
    list: Vec<(Idx, Arc<Notify>)>,
}

impl Waiters {
    #[instrument(skip_all)]
    async fn maintain(&mut self) {
        loop {
            let (idx, notify) = self.new.recv().await.expect("channel has been closed");
            let sorted_insert_pos = self
                .list
                .binary_search_by_key(&idx, |(idx, _)| *idx)
                .expect_err("idx should not yet be inside list");
            self.list.insert(sorted_insert_pos, (idx, notify));
        }
    }

    fn notify(&mut self, commit_idx: Idx) {
        let stop = self.list.binary_search_by_key(&commit_idx, |(idx, _)| *idx);
        let stop = match stop {
            Ok(found_value) => found_value + 1,
            Err(end_point) => end_point,
        };

        for (_, notify) in self.list.drain(..stop) {
            notify.notify_one()
        }
    }
}

struct Update {
    stream_id: usize,
    appended: u32,
}

pub struct Committed<'a, O: Order> {
    streams: stream::SelectAll<stream::BoxStream<'a, Update>>,
    highest: Vec<u32>, // index = stream_id
    waiters: Waiters,
    state: &'a State<O>,
}

impl<'a, O: Order> Committed<'a, O> {
    pub fn new(notify_rx: mpsc::Receiver<(Idx, Arc<Notify>)>, state: &'a State<O>) -> Self {
        Self {
            streams: stream::SelectAll::new(),
            highest: Vec::new(),
            waiters: Waiters {
                new: notify_rx,
                list: Vec::new(),
            },
            state,
        }
    }

    pub fn track_subject(&mut self) -> mpsc::Sender<u32> {
        let (tx, append_updates) = mpsc::channel(1);
        let stream_id = self.streams.len();
        let stream = Box::pin(stream::unfold(append_updates, move |mut rx| async move {
            let appended = rx.recv().await.unwrap();
            let yielded = Update {
                stream_id,
                appended,
            };
            Some((yielded, rx))
        }));
        self.streams.push(stream);
        self.highest.push(0);
        tx
    }

    fn majority_appended(&self) -> u32 {
        let majority = crate::util::div_ceil(self.highest.len(), 2);

        *self
            .highest
            .iter()
            .map(|candidate| {
                let n_higher = self.highest.iter().filter(|idx| *idx >= candidate).count();
                (n_higher, candidate)
            })
            .filter(|(n, _)| *n >= majority)
            .map(|(_, idx)| idx)
            .max()
            .unwrap()
    }

    #[instrument(skip_all)]
    pub async fn maintain(&mut self) {
        use stream::StreamExt;
        loop {
            tokio::select! {
                idx_update = self.streams.next() => {
                    let update = idx_update.unwrap();
                    self.highest[update.stream_id] = update.appended;
                    let new = self.majority_appended();

                    let old = self.state.commit_index();
                    if new < old {
                        continue
                    }

                    self.waiters.notify(new);
                    debug!("commit index increased: {old} -> {new}");
                    self.state.set_commit_index(new);
                    let deadline = Instant::now() + HB_PERIOD;
                    let n_entries = new - old; // number of entries that can be committed now
                    self.state.apply_comitted(deadline, n_entries, new).unwrap();
                }
                () = self.waiters.maintain() => (),
            }
        }
    }
}
