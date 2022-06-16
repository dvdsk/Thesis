use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Notify};

use crate::{Idx, Term};
use super::Order;

/// interface to append an item to the clusters raft log and
/// return once it is comitted
#[derive(Debug, Clone)]
pub struct LogWriter<O> {
    pub term: Term,
    pub state: super::State<O>,
    pub broadcast: broadcast::Sender<O>,
    pub notify_tx: mpsc::Sender<(Idx, Arc<Notify>)>,
}

pub struct AppendTicket {
    pub idx: Idx,
    pub notify: Arc<Notify>,
}

impl AppendTicket {
    pub async fn committed(self) {
        self.notify.notified().await;
    }
}

impl<O: Order> LogWriter<O> {
    // returns notify
    pub async fn append(&self, order: O) -> AppendTicket {
        let idx = self.state.append(order.clone(), self.term);
        let notify = Arc::new(Notify::new());
        self.notify_tx.send((idx, notify.clone())).await.unwrap();
        self.broadcast.send(order).unwrap();
        AppendTicket { idx, notify }
    }
    /// Verify an order was appended correctly, if it was not then append it again
    #[allow(dead_code)] // not dead used in tests will be used later
    pub async fn re_append(&self, order: O, prev_idx: Idx) -> AppendTicket {
        use super::LogEntry;

        match self.state.entry_at(prev_idx) {
            Some(LogEntry { order, .. }) if order == order => {
                let notify = Arc::new(Notify::new());
                self.notify_tx
                    .send((prev_idx, notify.clone()))
                    .await
                    .unwrap();
                AppendTicket {
                    idx: prev_idx,
                    notify,
                }
            }
            Some(LogEntry { .. }) => self.append(order).await,
            None => self.append(order).await,
        }
    }

    pub(crate) fn is_committed(&self, idx: u32) -> bool {
        self.state.is_committed(idx)
    }
}
