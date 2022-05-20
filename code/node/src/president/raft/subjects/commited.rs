use futures::stream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};
use tracing::debug;

use crate::Idx;

struct Waiters {
    new: mpsc::Receiver<(Idx, Arc<Notify>)>,
    list: Vec<(Idx, Arc<Notify>)>,
}

impl Waiters {
    async fn maintain(&mut self) {
        loop {
            let (idx, notify) = self.new.recv().await.unwrap();
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

pub struct Commited<'a> {
    streams: stream::SelectAll<stream::BoxStream<'a, Update>>,
    highest: Vec<u32>, // index = stream_id
    pub commit_idx: Arc<AtomicU32>,
    waiters: Waiters,
}

impl<'a> Commited<'a> {
    pub fn new(commit_idx: u32, notify_rx: mpsc::Receiver<(Idx, Arc<Notify>)>) -> Self {
        Self {
            streams: stream::SelectAll::new(),
            highest: Vec::new(),
            commit_idx: Arc::new(AtomicU32::new(commit_idx)),
            waiters: Waiters {
                new: notify_rx,
                list: Vec::new(),
            },
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

    pub async fn maintain(&mut self) {
        use stream::StreamExt;
        loop {
            tokio::select! {
                idx_update = self.streams.next() => {
                    let update = idx_update.unwrap();
                    self.highest[update.stream_id] = update.appended;
                    let new = self.majority_appended();

                    let old = self.commit_idx.load(Ordering::Relaxed);
                    if old < new {
                        self.waiters.notify(new);
                        debug!("commit index increased: {old} -> {new}");
                    }
                    self.commit_idx.store(new, Ordering::Relaxed);
                }
                () = self.waiters.maintain() => (),
            }
        }
    }
}