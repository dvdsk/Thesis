use std::collections::HashMap;
use std::path::{Path, PathBuf};

use instance_chart::Id;

use crate::directory::Staff;
use crate::president::raft::State;
use crate::president::Order;

use super::issue::Issue;

#[derive(Default)]
pub(super) struct Staffing {
    by_ministry: HashMap<PathBuf, Staff>,
    pub ministers: HashMap<Id, PathBuf>,
    pub clerks: HashMap<Id, PathBuf>,
    clerks_down: HashMap<PathBuf, Vec<Id>>,
}

impl Staffing {
    pub fn process_order(&mut self, order: Order) -> Result<(), Order> {
        use Order::*;

        match order {
            AssignMinistry { subtree, staff } => {
                self.by_ministry.insert(subtree.clone(), staff.clone());
                self.ministers.insert(staff.minister.id, subtree.clone());
                let tagged_clerks = staff.clerks.iter().map(|c| (c.id, subtree.clone()));
                self.clerks
                    .extend(tagged_clerks.map(|(clerk, path)| (clerk, path)));
                Ok(())
            }
            not_staff_order => Err(not_staff_order),
        }
    }

    pub fn from_committed(state: &State) -> Self {
        let mut ministries = Staffing::default();
        for order in state.committed() {
            let _ig_other = ministries.process_order(order);
        }

        ministries
    }

    pub fn has_root(&self) -> bool {
        self.by_ministry.contains_key(&PathBuf::from("/"))
    }

    pub fn is_empty(&self) -> bool {
        self.by_ministry.is_empty()
    }

    pub fn n_clerks(&self, ministry: &Path) -> usize {
        self.by_ministry.get(ministry).unwrap().clerks.len()
    }

    pub fn register_node_down(&mut self, id: Id) -> Option<Issue> {
        if let Some(ministry) = self.ministers.remove(&id) {
            return Some(Issue::LeaderLess {
                subtree: ministry,
                id,
            });
        }

        if let Some(ministry) = self.clerks.remove(&id) {
            let down = match self.clerks_down.get_mut(&ministry) {
                Some(down) => {
                    down.push(id);
                    down.clone()
                }
                None => {
                    self.clerks_down.insert(ministry.clone(), vec![id]).unwrap();
                    vec![id]
                }
            };

            if self.n_clerks(&ministry) - down.len() < 2 {
                return Some(Issue::UnderStaffed {
                    subtree: ministry,
                    down,
                });
            }
        }

        None
    }

    pub fn staff(&self, subtree: &Path) -> &Staff {
        self.by_ministry.get(subtree).unwrap()
    }

    pub fn clerk_back_up(&mut self, id: Id) -> Option<()> {
        let ministry = self.clerks.get(&id)?;
        let down = self.clerks_down.get_mut(ministry)?;
        // remove
        let idx = down
            .iter()
            .enumerate()
            .find(|(_, down_id)| **down_id == id)
            .unwrap()
            .0;
        down.swap_remove(idx);
        Some(())
    }
}
