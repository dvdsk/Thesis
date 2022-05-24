use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};

use crate::Term;

use super::*;
use crate::president::Chart;
use instance_chart::{discovery, Id};

pub async fn discoverd_majority(signal: mpsc::Sender<()>, chart: Chart, cluster_size: u16) {
    discovery::found_majority(&chart, cluster_size).await;
    signal.send(()).await.unwrap();
}

pub async fn wait_till_pres(
    orders: &mut mpsc::Receiver<Order>,
    tx: &mut mpsc::Sender<Order>,
) -> Term {
    loop {
        dbg!("loopyyy");
        match dbg!(orders.recv().await) {
            Some(order) => {
                tx.send(dbg!(&order).clone()).await.unwrap();
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
