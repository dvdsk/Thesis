use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};
use tracing::trace;

use crate::{Term, Id};

use super::*;
use crate::Chart;
use instance_chart::discovery;

pub async fn discoverd_majority(signal: mpsc::Sender<()>, chart: Chart, cluster_size: u16) {
    discovery::found_majority(&chart, cluster_size).await;
    signal.send(()).await.unwrap();
}

#[instrument(skip_all)]
pub async fn wait_till_pres(
    orders: &mut mpsc::Receiver<Order>,
    debug_tx: &mut mpsc::Sender<Order>,
) -> Term {
    loop {
        match orders.recv().await {
            Some(order) => {
                trace!("got applied order: {order:?}");
                debug_tx.try_send(order.clone()).unwrap();
                if let Order::BecomePres { term } = order {
                    break term;
                }
            }
            None => unreachable!("testnode dropped mpsc"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrPres {
    id: Arc<Mutex<Option<Id>>>,
    notify: Arc<Notify>,
}

impl Default for CurrPres {
    fn default() -> Self {
        Self {
            id: Arc::new(Mutex::new(None)),
            notify: Default::default(),
        }
    }
}

pub struct PresGuard<'a> {
    curr_pres: &'a mut CurrPres,
    id: Id,
}

impl<'a> Drop for PresGuard<'a> {
    fn drop(&mut self) {
        self.curr_pres.unset(self.id);
    }
}

impl CurrPres {
    pub async fn wait_for(&mut self) -> Id {
        loop {
            let notified = self.notify.notified();
            if let Some(id) = *self.id.lock().unwrap() {
                return id
            }

            notified.await;
            match self.get() {
                None => continue, // president resigned before this notify
                Some(id) => return id,
            }
        }
    }

    pub async fn kill<T>(&mut self, nodes: &mut HashMap<Id, T>) -> Id {
        let president = match tokio::time::timeout(TEST_TIMEOUT, self.wait_for()).await {
            Ok(pres) => pres,
            Err(_) => panic!("timed out waiting for president to be elected"),
        };
        std::mem::drop(nodes.remove(&president).unwrap());
        tracing::info!("############### KILLED PRESIDENT, ID: {president}");
        president
    }

    pub fn get(&mut self) -> Option<Id> {
        self.id.lock().unwrap().clone()
    }

    pub fn unset(&mut self, id: Id) {
        {
            let mut state = self.id.lock().unwrap();

            match *state {
                Some(curr_id) if curr_id == id => *state = None,
                _ => (),
            }
        }
    }

    pub fn set(&mut self, id: Id) -> PresGuard {
        let old = { self.id.lock().unwrap().replace(id) };
        assert_eq!(old, None, "can only be one president");
        self.notify.notify_one();
        PresGuard {
            curr_pres: self,
            id,
        }
    }
}
