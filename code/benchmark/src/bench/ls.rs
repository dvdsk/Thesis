use super::Bench;
use crate::bench::{Operation, Partition};
use itertools::chain;
use std::{iter, path::PathBuf};

pub fn ls_stride(n_parts: usize, n_ops: usize) -> Bench {
    let dirs = chain!(
        iter::once(String::from("/")),
        (1..n_parts).map(|n| format!("/{n}"))
    )
    .cycle()
    .take(n_ops * n_parts);
    ls_access(dirs, n_parts)
}
pub fn ls_batch(n_parts: usize, n_ops: usize) -> Bench {
    let dirs = chain!(
        iter::once(String::from("/")),
        (1..n_parts).map(|n| format!("/{n}"))
    )
    .flat_map(|dir| iter::repeat(dir).take(n_ops));
    ls_access(dirs, n_parts)
}

fn ls_access(dirs: impl Iterator<Item = String>, n_parts: usize) -> Bench {
    assert!(
        n_parts < 10,
        "update the files created on setup to support more then n_parts"
    );

    let operations = dirs
        .map(PathBuf::from)
        .map(|path| Operation::Ls { path })
        .collect();

    let partitions = iter::once("/".to_string())
        .chain((1..n_parts).into_iter().map(|n| format!("/{n}")))
        .map(|p| Partition {
            subtree: p,
            clerks: 2,
        })
        .collect();

    let additional_setup = iter::once("/".to_string())
        .chain((1..n_parts).into_iter().map(|n| format!("/{n}/")))
        .flat_map(|dir| {
            (0..10)
                .into_iter()
                .map(move |file| format!("{dir}file_{file}"))
                .map(PathBuf::from)
                .map(|path| Operation::Touch { path })
        })
        .collect();

    Bench {
        operations,
        client_nodes: 3,
        clients_per_node: 10,
        partitions,
        additional_setup,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ls_access() {
        let bench_stride = ls_stride(3, 3);
        let bench_batch = ls_batch(3, 3);

        dbg!(&bench_batch);
        dbg!(&bench_stride);

        assert_eq!(bench_stride.additional_setup, bench_batch.additional_setup);
        let stride_accesses: HashSet<_> = bench_stride.operations.into_iter().collect();
        let batch_accesses: HashSet<_> = bench_batch.operations.into_iter().collect();
        let diff: Vec<_> = stride_accesses.symmetric_difference(&batch_accesses).collect();
        assert_eq!(diff, Vec::<&Operation>::new(), "there should be no difference (left empty)");
    }
}
