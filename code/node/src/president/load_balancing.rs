use std::collections::HashSet;
use std::path::PathBuf;

use crate::Id;
use tokio::sync::mpsc;

use self::issue::Issues;

use super::{raft, LogWriter, Order};
use crate::directory::Staff;
mod directory;
use directory::Staffing;
mod issue;

#[derive(Debug, Clone)]
pub struct LoadNotifier {
    pub sender: mpsc::Sender<Event>,
}

impl LoadNotifier {
    pub async fn subject_up(&self, subject_id: Id) {
        self.sender.try_send(Event::NodeUp(subject_id)).unwrap();
    }
    pub async fn subject_down(&self, subject_id: Id) {
        self.sender.try_send(Event::NodeDown(subject_id)).unwrap();
    }
    pub async fn committed(&self, order: Order) {
        self.sender.try_send(Event::Committed(order)).unwrap();
    }
}

#[derive(Debug)]
pub enum Event {
    NodeUp(Id),
    NodeDown(Id),
    Committed(Order),
}

pub struct LoadBalancer {
    log_writer: LogWriter,
    events: mpsc::Receiver<Event>,
    policy: (),
}

impl LoadBalancer {
    pub fn new(log_writer: LogWriter) -> (Self, LoadNotifier) {
        let (tx, rx) = mpsc::channel(16);
        let notifier = LoadNotifier { sender: tx };
        (
            Self {
                events: rx,
                log_writer,
                policy: (),
            },
            notifier,
        )
    }
    pub async fn run(self, state: &raft::State) {
        let mut init = self.initialize(state).await;
        init.run().await;
    }
    pub async fn change_policy(&self) {
        // get some mutex
        // execute change
        // done
        todo!();
    }
}

impl LoadBalancer {
    async fn initialize(mut self, state: &raft::State) -> Init {
        let staffing = self.organise_staffing(state).await;
        Init {
            log_writer: self.log_writer,
            events: self.events,
            staffing,
            idle: HashSet::new(),
            policy: self.policy,
            issues: Default::default(),
        }
    }

    async fn organise_staffing(&mut self, state: &raft::State) -> Staffing {
        use Event::*;

        let mut staffing = Staffing::from_committed(&state);
        if staffing.has_root() {
            return staffing;
        }

        assert!(
            staffing.is_empty(),
            "there must be a root ministry before there can be any other"
        );

        // collect up to three not yet assigned nodes nodes, filter out those assigned to ministry
        let mut idle = HashSet::new();
        while idle.len() < 3 {
            match self.events.recv().await.unwrap() {
                NodeUp(id) => {
                    idle.insert(id);
                }
                NodeDown(id) => {
                    idle.remove(&id);
                }
                Committed(order) => {
                    staffing
                        .staff_order(order)
                        .expect("only order in a rootless cluster should be staff assignment");
                    if staffing.has_root() {
                        return staffing;
                    }
                }
            }
        }

        let minister = idle.drain().next().unwrap();
        let clerks = idle.drain().collect();
        let add_root = Order::AssignMinistry {
            subtree: PathBuf::from("/"),
            staff: Staff { minister, clerks },
        };
        self.log_writer.append(add_root).await.committed().await;
        staffing
    }
}

struct Init {
    log_writer: LogWriter,
    events: mpsc::Receiver<Event>,
    policy: (),
    staffing: Staffing,
    idle: HashSet<Id>,
    issues: Issues,
}

impl Init {
    pub async fn run(&mut self) {
        loop {
            self.process_state_changes().await;
            //
            // - TODO apply one policy change from queue
            // OR
            self.solve_worst_issue();
        }
    }

    pub fn solve_worst_issue(&mut self) {}

    async fn process_state_changes(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::NodeUp(id) => self.node_up(id),
                Event::NodeDown(id) => self.node_down(id),
                Event::Committed(_) => todo!(),
            }
        }
    }
}

impl Init {
    fn node_up(&mut self, id: Id) {
        if self.issues.solved_by_up(id) {
            return;
        }

        if self.staffing.clerk_back_up(id) {
            return
        }

        self.idle.insert(id);
    }

    fn node_down(&mut self, id: Id) {
        match self.staffing.register_node_down(id) {
            Some(issue) => self.issues.add(issue),
            None => {
                self.idle.remove(&id);
            }
        }
    }
}
