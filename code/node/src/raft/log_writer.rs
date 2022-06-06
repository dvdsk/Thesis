use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Notify};

use crate::{Idx, Term};
use super::Order;

/// interface to append an item to the clusters raft log and
/// return once it is comitted
#[derive(Debug, Clone)]
pub struct LogWriter {
    pub term: Term,
    pub state: super::State,
    pub broadcast: broadcast::Sender<Order>,
    pub notify_tx: mpsc::Sender<(Idx, Arc<Notify>)>,
}

pub struct AppendTicket {
    pub _idx: Idx,
    pub notify: Arc<Notify>,
}

impl AppendTicket {
    pub async fn committed(self) {
        self.notify.notified().await;
    }
}

impl LogWriter {
    // returns notify
    pub async fn append(&self, order: Order) -> AppendTicket {
        let idx = self.state.append(order.clone(), self.term);
        let notify = Arc::new(Notify::new());
        self.notify_tx.send((idx, notify.clone())).await.unwrap();
        self.broadcast.send(order).unwrap();
        AppendTicket { _idx: idx, notify }
    }
    /// Verify an order was appended correctly, if it was not append it again
    #[allow(dead_code)] // not dead used in tests will be used later
    pub async fn re_append(&self, order: Order, prev_idx: Idx) -> AppendTicket {
        use super::LogEntry;

        match self.state.entry_at(prev_idx) {
            Some(LogEntry { order, .. }) if order == order => {
                let notify = Arc::new(Notify::new());
                self.notify_tx
                    .send((prev_idx, notify.clone()))
                    .await
                    .unwrap();
                AppendTicket {
                    _idx: prev_idx,
                    notify,
                }
            }
            Some(LogEntry { .. }) => self.append(order).await,
            None => self.append(order).await,
        }
    }
}
