use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use futures::stream;
use tokio::sync::mpsc;

struct Update {
    stream_id: usize,
    appended: u32,
}

pub struct Commited<'a> {
    streams: stream::SelectAll<stream::BoxStream<'a, Update>>,
    highest: Vec<u32>, // index = stream_id
    pub commit_idx: Arc<AtomicU32>,
}

impl<'a> Commited<'a> {
    pub fn new(commit_idx: u32) -> Self {
        Self {
            streams: stream::SelectAll::new(),
            highest: Vec::new(),
            commit_idx: Arc::new(AtomicU32::new(commit_idx)),
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
            .enumerate()
            .map(|(i, candidate)| {
                let n_higher = self.highest[i..]
                    .iter()
                    .filter(|idx| *idx > candidate)
                    .count();
                (n_higher, candidate)
            })
            .filter(|(n, _)| *n > majority)
            .map(|(_, idx)| idx)
            .max()
            .unwrap()
    }

    pub async fn updates(&mut self) {
        use stream::StreamExt;
        let update = self.streams.next().await.unwrap();
        self.highest[update.stream_id] = update.appended;
        let new = self.majority_appended();
        self.commit_idx.store(new, Ordering::Relaxed);
    }
}
