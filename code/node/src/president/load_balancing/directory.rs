use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use instance_chart::Id;

use crate::directory::Staff;
use crate::president::raft::State;
use crate::president::Order;

use super::issue::{Issue, Issues};

#[derive(Default)]
pub(super) struct Staffing {
    by_subtree: HashMap<PathBuf, Staff>,
    pub ministers: HashMap<Id, PathBuf>,
    pub clerks: HashMap<Id, PathBuf>,
    clerks_down: HashMap<PathBuf, Vec<Id>>,
}

impl Staffing {
    pub fn staff_order(&mut self, order: Order) -> Result<(), Order> {
        use Order::*;

        match order {
            AssignMinistry { subtree, staff } => {
                self.by_subtree.insert(subtree.clone(), staff);
                self.ministers.insert(staff.minister, subtree.clone());
                let tagged_clerks = staff.clerks.into_iter().map(|c| (c, subtree.clone()));
                self.clerks.extend(tagged_clerks);
                Ok(())
            }
            not_staff_order => Err(not_staff_order),
        }
    }

    pub fn from_committed(state: &State) -> Self {
        let ministries = Staffing::default();
        for order in state.committed() {
            ministries.staff_order(order);
        }

        ministries
    }

    pub fn has_root(&self) -> bool {
        self.by_subtree.contains_key(&PathBuf::from("/"))
    }

    pub fn is_empty(&self) -> bool {
        self.by_subtree.is_empty()
    }

    pub fn has_employee(&self, id: Id) -> bool {
        self.ministers.contains_key(&id) || self.clerks.contains_key(&id)
    }

    pub fn n_clerks(&self, ministry: &Path) -> usize {
        self.by_subtree.get(ministry).unwrap().clerks.len()
    }

    pub fn register_node_down(&self, id: Id) -> Option<Issue> {
        if let Some(ministry) = self.ministers.remove(&id) {
            return Some(Issue::LeaderLess {
                subtree: ministry,
                id,
            })
        } 

        if let Some(ministry) = self.clerks.remove(&id) {
            let down = match self.clerks_down.get_mut(&ministry) {
                Some(down) => {
                    down.push(id);
                    down.clone()
                }
                None => {
                    self.clerks_down.insert(ministry, vec![id]).unwrap();
                    vec![id]
                }
            };

            if self.n_clerks(&ministry) - down.len() < 2 {
                return Some(Issue::UnderStaffed {
                    subtree: ministry,
                    down,
                })
            }
        } 

        None
    }

    pub fn clerk_back_up(&self, id: Id) -> bool {
        todo!()
    }
}
