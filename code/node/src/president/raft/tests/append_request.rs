use std::sync::Arc;

use tokio::sync::{mpsc, Barrier};
use tokio::task::JoinSet;

use crate::president::raft::state::append::Reply;
use crate::president::raft::State;
use crate::president::Order;
use crate::{util, Id, Term};

use super::state::append::Request;

#[derive(Debug, Clone)]
struct RequestGen {
    term: Term,
    leader_id: Id,
    prev_log_idx: u32,
    prev_log_term: u32,
    entries: Vec<Order>,
    leader_commit: u32,
}

impl RequestGen {
    fn new(leader_id: Id) -> Self {
        Self {
            term: 2,
            leader_id,
            prev_log_idx: 0,
            prev_log_term: 0,
            entries: Vec::new(),
            leader_commit: 0,
        }
    }
    /// create a new leader that missed a number
    /// of messages from leader `self`
    fn new_leader(&self, missed: usize) -> Self {
        let new_end = self.entries.len() - missed;
        Self {
            term: self.term + 1,
            leader_id: self.leader_id + 1,
            prev_log_idx: self.prev_log_idx - missed as u32,
            entries: self.entries[..new_end].to_vec(),
            ..self.clone()
        }
    }
    fn heartbeat(&mut self) -> Request {
        let req = Request {
            term: self.term,
            leader_id: self.leader_id,
            prev_log_idx: self.prev_log_idx,
            prev_log_term: self.prev_log_term,
            entries: Vec::new(),
            leader_commit: self.leader_commit,
        };
        self.prev_log_term = self.term;
        req
    }
    fn correct(&mut self, n: u8) -> Request {
        let entry = Order::Test(n);
        self.entries.push(entry.clone());
        let req = Request {
            entries: vec![entry],
            ..self.heartbeat()
        };
        self.prev_log_idx += 1;
        self.prev_log_term = self.term;
        req
    }
    fn commit(&mut self, n: u8) {
        let vec_idx = self
            .entries
            .binary_search_by_key(&n, |e| {
                if let Order::Test(n) = e {
                    *n
                } else {
                    panic!("only Test items expected")
                }
            })
            .unwrap() as u32;
        let log_idx = vec_idx + 1;
        self.leader_commit = self.leader_commit.max(log_idx);
    }
}

// TODO: replace Request creating functions with a struct that allows us
// to generate:
//  - a heartbeat doing nothing
//  - a heartbeat increating the committed entry
//  - a request sending Order::Test
//  - invalid requests for the above ^

fn setup() -> (RequestGen, State, mpsc::Receiver<Order>) {
    const ID: u64 = 2;
    let gen = RequestGen::new(ID);

    let db = sled::Config::new().temporary(true).open().unwrap();
    let tree = db.open_tree("pres").unwrap();
    let (order_tx, order_rx) = mpsc::channel(16);
    let state = State::new(order_tx, tree.clone(), ID);
    (gen, state, order_rx)
}

#[tokio::test]
async fn only_correct_entries() {
    util::setup_test_tracing("node=trace,node::util::db=warn");
    let (mut gen, state, mut order_rx) = setup();

    let reply = state.append_req(gen.correct(1)).await.unwrap();
    assert_eq!(reply, Reply::AppendOk);

    gen.commit(1);
    let reply = state.append_req(gen.heartbeat()).await.unwrap();
    assert_eq!(reply, Reply::HeartBeatOk);
    let order = order_rx.recv().await.unwrap();
    assert_eq!(order, Order::Test(1));

    let reply = state.append_req(gen.correct(2)).await.unwrap();
    assert_eq!(reply, Reply::AppendOk);

    gen.commit(2);
    let reply = state.append_req(gen.correct(3)).await.unwrap();
    assert_eq!(reply, Reply::AppendOk);
    let order = order_rx.recv().await.unwrap();
    assert_eq!(order, Order::Test(2));
}

#[tokio::test]
async fn some_incorrect_entries() {
    util::setup_test_tracing("node=trace,node::util::db=warn");
    let (mut gen, state, mut order_rx) = setup();

    for n in 10..13 {
        let reply = state.append_req(gen.correct(n)).await.unwrap();
        assert_eq!(reply, Reply::AppendOk);
    }
    gen.commit(10);
    let reply = state.append_req(gen.heartbeat()).await.unwrap();
    assert_eq!(reply, Reply::HeartBeatOk);

    // simulate a newly elected leader
    let mut zombi_gen = gen;
    let mut gen = zombi_gen.new_leader(2);
    let reply = state.append_req(gen.correct(21)).await.unwrap();
    assert_eq!(reply, Reply::AppendOk);

    let order = order_rx.recv().await.unwrap();
    assert_eq!(order, Order::Test(10));

    // zombi leader sends an order
    let reply = state.append_req(zombi_gen.correct(14)).await.unwrap();
    assert_eq!(reply, Reply::ExPresident(gen.term));

    gen.commit(21);
    let reply = state.append_req(gen.heartbeat()).await.unwrap();
    assert_eq!(reply, Reply::HeartBeatOk);

    let order = order_rx.recv().await.unwrap();
    assert_eq!(order, Order::Test(21));
}

async fn append_correct(
    mut gen: RequestGen,
    state: State,
    barrier: Arc<Barrier>,
    nums: u8,
) -> RequestGen {
    barrier.wait().await;
    for n in 0..nums {
        let req = gen.correct(n);

        let reply = state.append_req(req).await.unwrap();
        assert!(reply == Reply::AppendOk || reply == Reply::InconsistentLog);
    }
    gen
}

/// appends the same items many times
#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn append_multiple_simultaneous() {
    util::setup_test_tracing("node=trace,node::util::db=warn");
    let (gen, state, mut order_rx) = setup();

    let runs = 20;
    let nums = 10;
    let barrier = Arc::new(Barrier::new(runs));
    let mut tasks = JoinSet::new();
    for _ in 0..runs {
        tasks
            .build_task()
            .name("append_correct")
            .spawn(append_correct(
                gen.clone(),
                state.clone(),
                barrier.clone(),
                nums,
            ));
    }

    let mut new_gen = None;
    while let Some(gen) = tasks.join_one().await.unwrap() {
        new_gen = Some(gen);
    }
    let mut gen = new_gen.expect("task set is empty, it should not be");

    for n in 0..nums {
        gen.commit(n as u8);
        let reply = state.append_req(gen.heartbeat()).await.unwrap();
        assert_eq!(reply, Reply::HeartBeatOk);

        let order = order_rx.recv().await.unwrap();
        assert_eq!(order, Order::Test(n as u8));
    }
}

#[tokio::test]
async fn mpsc_full() {
    util::setup_test_tracing("node=trace,node::util::db=warn");
    const ID: u64 = 2;
    let mut gen = RequestGen::new(ID);

    let db = sled::Config::new().temporary(true).open().unwrap();
    let tree = db.open_tree("pres").unwrap();
    let (order_tx, _order_rx) = mpsc::channel(16);
    let state = State::new(order_tx, tree.clone(), ID);

    // get some entries rdy to be committed
    for n in 0..16 {
        let req = gen.correct(n);

        let reply = state.append_req(req).await.unwrap();
        assert_eq!(reply, Reply::AppendOk);
        gen.commit(n as u8); // "commit" entry
    }
    // ensure last entry is committed
    let reply = state.append_req(gen.heartbeat()).await.unwrap();
    assert_eq!(reply, Reply::HeartBeatOk);

    // next entry will cause order_mpsc to be full
    let req = gen.correct(16); // 17th entry
    gen.commit(16); // "commit" entry
    let reply = state.append_req(req).await.unwrap();
    assert_eq!(reply, Reply::AppendOk);
    let res = state.append_req(gen.heartbeat()).await;
    assert!(res.is_err());
}
