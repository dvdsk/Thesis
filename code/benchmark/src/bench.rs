use client::Client;
use std::{collections::HashSet, iter, ops::Range, path::PathBuf};

#[derive(Debug, Clone)]
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
                client.list(path).await;
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
                file.write(&mut buf[0..(range.end as usize)]).await;
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

#[derive(Debug)]
pub struct Bench {
    operations: Vec<Operation>,
    clients: usize,
    pub partitions: Vec<Partition>,
    /// operations to run before the benchmark
    setup: Vec<Operation>,
}

impl Bench {
    pub async fn perform<T: client::RandomNode>(self, client: &mut Client<T>, buf: &mut [u8]) {
        for op in self.operations.into_iter() {
            op.perform(client, buf).await;
        }
    }

    pub async fn prep<T: client::RandomNode>(&self, client: &mut Client<T>) {
        use itertools::chain;
        let needed_files: HashSet<_> = chain!(self.operations.iter(), self.setup.iter())
            .filter_map(Operation::needed_file)
            .collect();
        for path in needed_files {
            client.create_file(path).await
        }
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
        self.clients
    }

    pub fn ls_stride(n_parts: usize) -> Bench {
        let dirs = (0..n_parts).cycle().take(1000 * n_parts);
        Self::ls_access(dirs, n_parts)
    }
    pub fn ls_batch(n_parts: usize) -> Bench {
        let dirs = (0..n_parts)
            .map(|dir| iter::repeat(dir).take(1000))
            .flatten();
        Self::ls_access(dirs, n_parts)
    }

    fn ls_access(dirs: impl Iterator<Item = usize>, n_parts: usize) -> Bench {
        let operations = dirs
            .map(|n| format!("/{n}"))
            .map(PathBuf::from)
            .map(|path| Operation::Ls { path })
            .collect();

        let partitions = (0..n_parts)
            .into_iter()
            .map(|n| format!("/{n}"))
            .map(|p| Partition{subtree: p.into(), clerks: 2})
            .collect();
        let setup = Vec::new();

        Bench {
            operations,
            clients: 3,
            partitions,
            setup,
        }
    }
}

use serde::{Deserialize, Serialize};
#[derive(clap::Subcommand, Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    LsStride { n_parts: usize },
    LsBatch { n_parts: usize },
}

impl From<&Command> for Bench {
    fn from(cmd: &Command) -> Self {
        match cmd {
            Command::LsStride { n_parts } => Self::ls_stride(*n_parts),
            Command::LsBatch { n_parts } => Self::ls_batch(*n_parts),
        }
    }
}

impl Command {
    pub fn args(&self) -> String {
        match self {
            Command::LsStride { n_parts } => format!("ls-stride {n_parts}"),
            Command::LsBatch { n_parts } => format!("ls-batch {n_parts}"),
        }
    }
}
  
// impl Command {
//     pub fn serialize(&self) -> [u8;100] {
//         let buf = [0u8;100];
//         let mut buf = io::Cursor::new(buf);
//         bincode::serialize_into(&mut buf, self);
//         buf.into_inner()
//     }
//
//     pub fn deserialize(buf: [u8; 100]) -> color_eyre::Result<Self> {
//         Ok(bincode::deserialize(&buf)?)
//     }
// }
