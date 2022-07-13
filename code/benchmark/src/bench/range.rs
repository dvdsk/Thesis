use super::Bench;
use crate::bench::Operation;
use std::path::PathBuf;

/// simulate writing in rows to a square matrix
pub fn by_row(rows: usize, clients_per_node: usize) -> Bench {
    let path: PathBuf = "/mat".into();
    let operations = (0..rows)
        .into_iter()
        .map(|n| Operation::Write {
            path: path.clone(),
            range: (n * rows) as u64..((n + 1) * rows) as u64,
        })
        .collect();

    let additional_setup = vec![Operation::Touch { path }];

    Bench {
        operations,
        client_nodes: 3,
        clients_per_node, 
        partitions: Vec::new(),
        additional_setup,
    }
}

/// read the entire file in one go
pub fn whole_file(rows: usize, clients_per_node: usize) -> Bench {
    let path: PathBuf = "/mat".into();
    let operations = vec![Operation::Write {
        path: path.clone(),
        range: 0..(rows * rows) as u64,
    }];

    let additional_setup = vec![Operation::Touch { path }];

    Bench {
        operations,
        client_nodes: 3,
        clients_per_node,
        partitions: Vec::new(),
        additional_setup,
    }
}
