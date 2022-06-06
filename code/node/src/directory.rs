#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use instance_chart::Id;
use serde::{Serialize, Deserialize};

use crate::raft::State;
use crate::president::Order;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staff {
    pub minister: Id,
    pub clerks: Vec<Id>,
}

impl Staff {
    pub fn len(&self) -> usize {
        self.clerks.len() + 1
    }
}

#[derive(Default)]
pub(super) struct SubtreeAssignment {
    by_subtree: HashMap<PathBuf, Staff>,
}

impl SubtreeAssignment {
    pub fn staff_order(&mut self, order: Order) -> Result<(), Order> {
        use Order::*;

        match order {
            AssignMinistry { subtree, staff } => {
                self.by_subtree.insert(subtree, staff);
                Ok(())
            }
            not_staff_order => Err(not_staff_order),
        }
    }

    pub fn from_committed(state: &State) -> Self {
        let mut ministries = SubtreeAssignment::default();
        for order in state.committed() {
            let _ig_other = ministries.staff_order(order);
        }

        ministries
    }
}
