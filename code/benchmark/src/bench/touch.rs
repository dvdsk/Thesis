use super::Bench;
use crate::bench::{Operation, Partition};
use std::{iter, path::PathBuf};

pub fn touch(n_parts: usize, id: usize) -> Bench {
    assert!(
        n_parts < 10,
        "update the files created on setup to support more then n_parts"
    );

    let part = id % n_parts;
    let path = if part == 0 {
        String::new()
    } else {
        format!("/{part}")
    };

    let operations = (0..10)
        .into_iter()
        .map(|file| format!("{path}/{file}_{id}"))
        .map(PathBuf::from)
        .map(|path| Operation::Touch { path })
        .collect();

    let partitions = iter::once("/".to_string())
        .chain((1..n_parts).into_iter().map(|n| format!("/{n}")))
        .map(|p| Partition {
            subtree: p,
            clerks: 2,
        })
        .collect();

    Bench {
        operations,
        client_nodes: 3,
        clients_per_node: 3,
        partitions,
        additional_setup: Vec::new(),
    }
}
