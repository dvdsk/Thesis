use std::collections::HashMap;
use std::path::{Path, PathBuf};

use instance_chart::Id;
use tracing::instrument;

use crate::president;
use crate::president::raft::State;
use crate::redirectory::Staff;

use super::issue::Issue;

#[derive(Default, Debug)]
pub(super) struct Staffing {
    by_ministry: HashMap<PathBuf, Staff>,
    pub ministers: HashMap<Id, PathBuf>,
    pub clerks: HashMap<Id, PathBuf>,
    clerks_down: HashMap<PathBuf, Vec<Id>>,
}

impl Staffing {
    pub fn process_order(&mut self, order: president::Order) {
        use president::Order::*;

        if let AssignMinistry { subtree, staff } = order {
            self.by_ministry.insert(subtree.clone(), staff.clone());
            self.ministers.insert(staff.minister.id, subtree.clone());
            let tagged_clerks = staff.clerks.iter().map(|c| (c.id, subtree.clone()));
            self.clerks
                .extend(tagged_clerks.map(|(clerk, path)| (clerk, path)));
            self.reset_down(&subtree);
        }
    }

    pub fn from_committed(state: &State<president::Order>) -> Self {
        let mut ministries = Staffing::default();
        for order in state.committed() {
            ministries.process_order(order);
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

    fn reset_down(&mut self, ministry: &Path) {
        self.clerks_down.remove(ministry);
    }

    fn clerks_add_down(&mut self, ministry: &Path, id: Id) {
        match self.clerks_down.get_mut(ministry) {
            Some(down) => {
                down.push(id);
            }
            None => {
                let existing = self.clerks_down.insert(ministry.to_owned(), vec![id]);
                assert_eq!(existing, None);
            }
        }
    }

    #[instrument(skip(self), ret)]
    pub fn register_node_down(&mut self, id: Id) -> Vec<Issue> {
        let mut issues = Vec::new();

        let ministry = if let Some(ministry) = self.ministers.remove(&id) {
            issues.push(Issue::LeaderLess {
                subtree: ministry.clone(),
                id,
            });
            ministry
        } else if let Some(ministry) = self.clerks.remove(&id) {
            self.clerks_add_down(&ministry, id);
            ministry
        } else {
            return Vec::new();
        };

        let down = self
            .clerks_down
            .get(&ministry)
            .cloned()
            .unwrap_or_default();
        let clerks_left = self.n_clerks(&ministry) - down.len();
        if clerks_left < 2 {
            issues.push(Issue::UnderStaffed {
                subtree: ministry,
                down,
            });
        }
        issues
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
