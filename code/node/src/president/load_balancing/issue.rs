use crate::Id;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::PathBuf;

/// the shared raft log/state determines the correct cluster state
/// any deviation from it is an issue.
///
/// For example a clerk that went down is still assigned to its
/// ministry until a replacement is found resolving the issue.
/// If the node gets back up the issue is resolved
#[derive(Default)]
pub struct Issues {
    by_priority: BinaryHeap<Issue>,
    active: HashSet<Issue>,
    by_employee: HashMap<Id, Issue>,
}

impl Issues {
    pub fn add(&mut self, issue: Issue) {
        self.by_priority.push(issue.clone());
        self.active.insert(issue);
    }
    pub fn remove_worst(&mut self) -> Option<Issue> {
        let mut worst = self.by_priority.pop()?;
        while !self.active.remove(&worst) {
            worst = self.by_priority.pop()?;
        }
        Some(worst)
    }
    /// returns true
    pub fn solved_by_up(&mut self, employee: Id) -> bool {
        if let Some(issue) = self.by_employee.remove(&employee) {
            assert!(self.active.remove(&issue));
            true
        } else {
            false
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Issue {
    // lost a minister
    LeaderLess { subtree: PathBuf, id: Id },
    // less clerks then minimum (=3) left
    UnderStaffed { subtree: PathBuf, down: Vec<Id> },
    // experiencing a load higher then allowed
    #[allow(dead_code)]
    Overloaded { load: (), subtree: PathBuf },
}

impl Ord for Issue {
    fn cmp(&self, other: &Self) -> Ordering {
        use Issue::*;
        use Ordering::*;
        match (self, other) {
            (LeaderLess { .. }, LeaderLess { .. }) => Equal,
            (LeaderLess { .. }, UnderStaffed { .. }) => Greater,
            (LeaderLess { .. }, Overloaded { .. }) => Greater,
            (UnderStaffed { .. }, LeaderLess { .. }) => Less,
            (UnderStaffed { .. }, UnderStaffed { .. }) => Equal,
            (UnderStaffed { .. }, Overloaded { .. }) => Less,
            (Overloaded { .. }, LeaderLess { .. }) => Less,
            (Overloaded { .. }, UnderStaffed { .. }) => Less,
            (Overloaded { load: load_a, .. }, Overloaded { load: load_b, .. }) => {
                load_a.cmp(load_b)
            }
        }
    }
}

impl PartialOrd for Issue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
