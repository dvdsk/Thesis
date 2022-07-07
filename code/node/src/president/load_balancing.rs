use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::timeout;

use tokio::sync::mpsc;
use tracing::{info, instrument, warn};

use self::issue::{Issue, Issues};

use super::{raft, Chart, FixedTerm, LogWriter, Order};
use crate::redirectory::{Node, Staff};
use crate::{Id, Term};
mod staffing;
use staffing::Staffing;
mod action;
mod issue;

#[derive(Debug, Clone)]
pub struct LoadNotifier {
    pub sender: mpsc::Sender<Event>,
}

#[async_trait]
impl crate::raft::subjects::StatusNotifier for LoadNotifier {
    async fn subject_up(&self, subject: Id) {
        self.sender.try_send(Event::NodeUp(subject)).unwrap();
    }
    async fn subject_down(&self, subject: Id) {
        self.sender.try_send(Event::NodeDown(subject)).unwrap();
    }
}

impl LoadNotifier {
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
    highest_term: Term,
    log_writer: LogWriter<Order, FixedTerm>,
    events: mpsc::Receiver<Event>,
    chart: Chart,
    policy: (),
}

impl LoadBalancer {
    pub fn new(log_writer: LogWriter<Order, FixedTerm>, chart: Chart) -> (Self, LoadNotifier) {
        let (tx, rx) = mpsc::channel(16);
        let notifier = LoadNotifier { sender: tx };
        (
            Self {
                highest_term: 0,
                events: rx,
                log_writer,
                chart,
                policy: (),
            },
            notifier,
        )
    }
    pub async fn run(self, state: &raft::State<Order>, partitions: Vec<Partition>) {
        let mut init = self.initialize(state, partitions).await;
        init.run().await;
    }
    #[allow(dead_code)]
    pub async fn change_policy(&self) {
        // get some mutex
        // execute change
        // done
        todo!();
    }
}

#[derive(Clone, Debug)]
pub struct Partition {
    subtree: PathBuf,
    pub clerks: usize,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PartitionFromStrError {
    #[error("missing delimiter `:` seperating path from number of clerks")]
    NoDelimiter,
    #[error("path could not be parsed")]
    InvalidPath(<PathBuf as FromStr>::Err),
    #[error("number of clerks is not a valid number")]
    InvalidClerks(<usize as FromStr>::Err),
    #[error("need a minimum of 2 clerks per partition")]
    NotEnoughClerks,
}

impl FromStr for Partition {
    type Err = PartitionFromStrError;

    /// format: subtree:clerks
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use PartitionFromStrError::*;
        let (subtree, clerks) = s.split_once(':').ok_or(NoDelimiter)?;
        let clerks = clerks.parse().map_err(InvalidClerks)?;
        if clerks < 2 {
            return Err(NotEnoughClerks);
        }

        Ok(Self {
            subtree: subtree.parse().map_err(InvalidPath)?,
            clerks,
        })
    }
}

impl Partition {
    pub fn new(subtree: PathBuf, clerks: usize) -> Self {
        assert!(
            clerks >= 2,
            "partition needs at least two clerks to be functional"
        );
        Self { subtree, clerks }
    }
}

impl LoadBalancer {
    async fn initialize(mut self, state: &raft::State<Order>, partitions: Vec<Partition>) -> Init {
        let mut staffing = Staffing::from_committed(state);
        match (staffing.has_root(), partitions.is_empty()) {
            (true, _) => (),
            (false, false) => self.add_partitions(&mut staffing, partitions).await,
            (false, true) => self.add_root(&mut staffing).await,
        }

        Init {
            chart: self.chart,
            log_writer: self.log_writer,
            events: self.events,
            staffing,
            idle: HashMap::new(),
            _policy: self.policy,
            issues: Issues::default(),
        }
    }

    #[instrument(skip(self))]
    async fn collect_idle(&mut self, needed: usize) -> HashMap<Id, Node> {
        use Event::*;
        let mut idle = HashMap::new();
        while idle.len() < needed {
            match self.events.recv().await.unwrap() {
                NodeUp(id) => {
                    let node = Node::from_chart(id, &self.chart);
                    idle.insert(id, node);
                }
                NodeDown(id) => {
                    idle.remove(&id);
                }
                Committed(order) => {
                    panic!("no orders should arrive while adding root, got: {order:?}");
                }
            }
        }
        idle
    }

    #[instrument(skip_all)]
    async fn add_partitions(&mut self, staffing: &mut Staffing, partitions: Vec<Partition>) {
        assert!(
            staffing.is_empty(),
            "can not setup inital staff if there already is a staff"
        );

        let needed: usize = partitions.iter().map(|p| p.clerks + 1).sum();
        let mut idle = self.collect_idle(needed).await.into_values();

        for part in partitions {
            let add = Order::AssignMinistry {
                subtree: part.subtree,
                staff: Staff {
                    minister: idle.next().unwrap().clone(),
                    clerks: (&mut idle).take(part.clerks).collect(),
                    term: self.highest_term,
                },
            };
            info!("adding partition: {:?}", &add);
            self.log_writer.append(add).await.committed().await;
        }
    }

    async fn add_root(&mut self, staffing: &mut Staffing) {
        assert!(
            staffing.is_empty(),
            "there must be a root ministry before there can be any other"
        );

        // collect up to three not yet assigned nodes nodes,
        // filter out those assigned to ministry
        let mut idle = self.collect_idle(3).await.into_values();
        let minister = idle.next().unwrap();
        let clerks = idle.collect();

        self.highest_term += 1;
        let add_root = Order::AssignMinistry {
            subtree: PathBuf::from("/"),
            staff: Staff {
                minister,
                clerks,
                term: self.highest_term,
            },
        };
        info!("adding root: {:?}", &add_root);
        self.log_writer.append(add_root).await.committed().await;
    }
}

struct Init {
    chart: Chart,
    log_writer: LogWriter<Order, FixedTerm>,
    events: mpsc::Receiver<Event>,
    /// used for staffing changes due the drop of a ministry because
    /// its directory is deleted
    _policy: (),
    staffing: Staffing,
    idle: HashMap<Id, Node>,
    issues: Issues,
}

impl Init {
    pub async fn run(&mut self) {
        self.check_staffing().await;

        loop {
            // wait for a change to be able to solve existing issues or
            // for new issues to appear
            self.process_state_changes().await;

            // - TODO apply one policy change from queue
            //   ( will need to join state_change with wait for new policy change )
            // OR
            self.solve_worst_issue().await;
        }
    }

    async fn check_staffing(&mut self) {
        let ministers = self.staffing.ministers.iter();
        let clerks = self.staffing.clerks.iter();

        // start with the assumption that all nodes are down
        let needed: Vec<_> = ministers.chain(clerks).map(|(id, _)| id).copied().collect();
        for id in needed {
            self.node_down(id);
        }

        async fn process_current_state(init: &mut Init) {
            while !init.issues.is_empty() {
                init.process_state_changes().await;
            }
        }

        const STARTUP_DUR: Duration = Duration::from_millis(200);
        let res = timeout(STARTUP_DUR, process_current_state(self)).await;
        if res.is_err() {
            warn!(
                "Not all needed nodes are up after startup, resulting issues: {:?}",
                self.issues
            )
        }
    }

    #[instrument(skip(self))]
    pub async fn solve_worst_issue(&mut self) {
        use Issue::*;

        let mut unsolved = Vec::new();
        loop {
            let issue = match self.issues.remove_worst() {
                None => break,
                Some(i) => i,
            };

            info!("trying to solve: {issue:?}");
            let solved = match &issue {
                LeaderLess { subtree, .. } => self.promote_clerk(subtree).await,
                UnderStaffed { subtree, down } => self.try_assign(subtree, down).await,
                Overloaded { .. } => todo!(),
            };

            match solved {
                Ok(_) => return, // state changed due to solving issue
                Err(e) => {
                    warn!("could not solve issue, err: {e:?}");
                    unsolved.push(issue);
                }
            }
        }
    }

    /// cancel safe
    #[instrument(skip(self))]
    async fn process_state_changes(&mut self) {
        let event = self
            .events
            .recv()
            .await
            .expect("events mpsc should not be closed");
        match event {
            Event::NodeUp(node) => self.node_up(node),
            Event::NodeDown(node) => self.node_down(node),
            Event::Committed(order) => self.staffing.process_order(order),
        }
    }
}

impl Init {
    #[instrument(skip(self))]
    fn node_up(&mut self, node_id: Id) {
        if self.issues.solved_by_up(node_id) {
            return;
        }

        if self.staffing.clerk_back_up(node_id).is_some() {
            return;
        }

        let node = Node::from_chart(node_id, &self.chart);
        self.idle.insert(node_id, node);
    }

    #[instrument(skip(self))]
    fn node_down(&mut self, node_id: Id) {
        let issues = self.staffing.register_node_down(node_id);
        for issue in issues {
            self.issues.add(issue);
        }
    }
}
