use client::Client;
use itertools::{chain, Itertools};
use std::{
    ffi::OsString,
    ops::Range,
    path::PathBuf,
    time::Instant,
};
use tracing::instrument;

mod ls;
mod touch;
mod range;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum Operation {
    Ls { path: PathBuf },
    Touch { path: PathBuf },
    Read { path: PathBuf, range: Range<u64> },
    Write { path: PathBuf, range: Range<u64> },
}

impl Operation {
    async fn perform<T: client::RandomNode>(self, client: &mut Client<T>, buf: &mut [u8]) {
        match self {
            Operation::Ls { path } => {
                let res = client.list(path.clone()).await;
            /* TODO: remove during bench <dvdsk noreply@davidsk.dev> */
                assert!(res.len() > 0, "no files on path: {path:?}"); 
            }
            Operation::Touch { path } => client.create_file(path).await,
            Operation::Read { path, range } => {
                let mut file = client.open_readable(path).await;
                file.seek(range.start);
                file.read(&mut buf[0..(range.end as usize)]).await;
            }
            Operation::Write { path, range } => {
                let mut file = client.open_writeable(path).await;
                file.seek(range.start);
                file.write(&buf[0..(range.end as usize)]).await;
            }
        }
    }

    fn needed_file(&self) -> Option<PathBuf> {
        match self {
            Operation::Ls { .. } => None,
            Operation::Touch { .. } => None,
            Operation::Read { path, .. } => Some(path.clone()),
            Operation::Write { path, .. } => Some(path.clone()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Partition {
    pub subtree: String,
    pub clerks: usize,
}

#[derive(Debug, Clone)]
pub struct Bench {
    operations: Vec<Operation>,
    client_nodes: usize,
    pub clients_per_node: usize,
    pub partitions: Vec<Partition>,
    /// operations to run before the benchmark. The need files for reads/writes
    /// are created automatically
    additional_setup: Vec<Operation>,
}

impl Bench {
    pub async fn perform<T: client::RandomNode>(
        self,
        client: &mut Client<T>,
        buf: &mut [u8],
    ) -> Vec<(Instant, Instant)> {
        let mut res = Vec::new();
        for op in self.operations.into_iter() {
            let start = Instant::now();
            op.perform(client, buf).await;
            let stop = Instant::now();
            res.push((start, stop));
        }
        res
    }

    #[instrument(skip_all)]
    pub async fn prep<T: client::RandomNode>(&self, client: &mut Client<T>) {
        use indicatif::ProgressBar;

        let needed_files: Vec<_> = self
            .operations
            .iter()
            .filter_map(Operation::needed_file)
            .unique()
            .map(|path| Operation::Touch { path })
            .collect();

        let mut buf = vec![0u8; 1_000_00];
        let pb = ProgressBar::new((self.additional_setup.len() + needed_files.len()) as u64);
        for op in chain!(
            self.additional_setup.iter().cloned(),
            needed_files.into_iter()
        ) {
            op.clone().perform(client, &mut buf).await;
            pb.inc(1);
        }
        pb.finish();
    }
}

impl Bench {
    pub fn fs_nodes(&self) -> usize {
        self.partitions
            .iter()
            .map(|p| 1 + p.clerks)
            .sum::<usize>()
            .max(3)
            + 1 // dont forget the president
    }
    pub fn needed_nodes(&self) -> usize {
        self.fs_nodes() + self.client_nodes()
    }

    pub fn client_nodes(&self) -> usize {
        self.client_nodes
    }
}

use serde::{Deserialize, Serialize};

#[derive(clap::Subcommand, Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    LsStride {
        n_parts: usize,
    },
    LsBatch {
        n_parts: usize,
    },
    RangeByRow {
        rows: usize,
        clients_per_node: usize,
    },
    RangeWholeFile {
        rows: usize,
        clients_per_node: usize,
    },
    /// each client creates n files at unique paths
    Touch {
        n_parts: usize,
    },
    
}

impl Bench {
    pub fn from(cmd: &Command, id: usize) -> Self {
        use ls::{stride, batch};
        use Command::*;

        match *cmd {
            LsStride { n_parts } => stride(n_parts, 2_000, ls::ls),
            LsBatch { n_parts } => batch(n_parts, 2_000, ls::ls),
            RangeByRow {
                rows,
                clients_per_node,
            } => range::by_row(rows, clients_per_node),
            RangeWholeFile {
                rows,
                clients_per_node,
            } => range::whole_file(rows, clients_per_node),
            Touch { n_parts } => touch::touch(n_parts, id),
        }
    }
}

impl Command {
    pub fn args(&self) -> OsString {
        match self {
            Command::LsStride { n_parts } => format!("ls-stride {n_parts}"),
            Command::LsBatch { n_parts } => format!("ls-batch {n_parts}"),
            Command::RangeByRow {
                rows,
                clients_per_node,
            } => format!("range-by-row {rows} {clients_per_node}"),
            Command::RangeWholeFile {
                rows,
                clients_per_node,
            } => format!("range-whole-file {rows} {clients_per_node}"),
            Command::Touch { n_parts } => format!("touch {n_parts}"),
        }
        .into()
    }

    pub fn results_file(&self, node: &str) -> PathBuf {
        let path = match self {
            Command::LsStride { n_parts } => format!("LsStride/{n_parts}"),
            Command::LsBatch { n_parts } => format!("LsBatch/{n_parts}"),
            Command::RangeByRow {
                rows,
                clients_per_node,
            } => format!("RangeByRow/{rows}_{clients_per_node}"),
            Command::RangeWholeFile {
                rows,
                clients_per_node,
            } => format!("RangeWholeFile/{rows}_{clients_per_node}"),
            Command::Touch { n_parts } => format!("Touch/{n_parts}"),
        };
        PathBuf::from(format!("data/{path}/{node}.csv"))
    }
}
