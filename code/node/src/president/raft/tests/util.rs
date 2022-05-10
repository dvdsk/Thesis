
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

pub async fn wait_till_pres(orders: &mut mpsc::Receiver<Order>, tx: &mut mpsc::Sender<Order>) -> Term {
    loop {
        match orders.recv().await {
            Some(order) => {
                tx.send(order.clone()).await.unwrap();
                if let Order::BecomePres { term } = order {
                    break term;
                }
            }
            None => panic!("testnode dropped mpsc"),
        }
    }
}


#[derive(Debug, Clone)]
pub struct CurrPres {
    state: Arc<Mutex<std::result::Result<Option<Id>, ()>>>,
    notify: Arc<Notify>,
}

impl Default for CurrPres {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(Ok(None))),
            notify: Default::default(),
        }
    }
}

struct PresGuard<'a> {
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
            self.notify.notified().await;
            match self.get() {
                None => continue,  // president resigned before this notify 
                Some(id) => return id,
            }
        }
    }

    pub fn get(&mut self) -> Option<Id> {
        self.state
            .lock()
            .unwrap()
            .clone()
            .expect("can only be one president")
    }

    pub fn unset(&mut self, id: Id) {
        let state = &mut self.state.lock().unwrap();
        let pres = state.as_mut().expect("can only be one president");

        match pres {
            Some(curr_id) if *curr_id == id => *pres = None,
            _ => (),
        }
    }

    pub fn set(&mut self, id: Id) -> PresGuard {
        let old = {
            let state = &mut self.state.lock().unwrap();
            state
                .as_mut()
                .expect("can only be one president")
                .replace(id)
        };
        assert_eq!(old, None, "can only be one president");
        self.notify.notify_one();
        PresGuard {
            curr_pres: self,
            id,
        }
    }
}
