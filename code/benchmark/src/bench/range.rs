use super::Bench;
use crate::bench::Operation;
use std::path::PathBuf;

/// simulate writing 10 long rows
pub fn by_row(rows_len: usize, clients_per_node: usize, client_nodes: usize) -> Bench {
    let path: PathBuf = "/mat".into();
    let operations = (0..10)
        .into_iter()
        .map(|n| Operation::Write {
            path: path.clone(),
            range: (n * rows_len) as u64..((n + 1) * rows_len) as u64,
        })
        .collect();

    let additional_setup = vec![Operation::Touch { path }];

    Bench {
        operations,
        client_nodes,
        clients_per_node, 
        partitions: Vec::new(),
        additional_setup,
    }
}

/// read the entire file in one go
pub fn whole_file(rows_len: usize, clients_per_node: usize, client_nodes: usize) -> Bench {
    let path: PathBuf = "/mat".into();
    let operations = vec![Operation::Write {
        path: path.clone(),
        range: 0..(10 * rows_len) as u64,
    }];

    let additional_setup = vec![Operation::Touch { path }];

    Bench {
        operations,
        client_nodes,
        clients_per_node,
        partitions: Vec::new(),
        additional_setup,
    }
}
