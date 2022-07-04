use serde::{Serialize, Deserialize};
use tokio::sync::mpsc::{self, Sender};

use crate::raft::{self, Perishable, State};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MockOrder;
impl raft::Order for MockOrder {
    fn elected(_term: crate::Term) -> Self {
        MockOrder
    }
    fn resign() -> Self {
        MockOrder
    }
    fn none() -> Self {
        MockOrder
    }
}

#[test]
fn log_idx() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let tree = db.open_tree("pres").unwrap();

    let (tx, _): (Sender<Perishable<MockOrder>>, _) = mpsc::channel(1);
    let state = State::new(tx, tree);

    for i in 1..500 {
        let idx = state.append(MockOrder, 0);
        assert_eq!(idx, i);
        let meta = state.last_log_meta();
        assert_eq!(meta.idx, idx)
    }
}
