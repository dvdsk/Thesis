use node::Partition;
use std::{iter, ops::Range, path::PathBuf, io};

#[derive(Debug, Clone)]
pub enum Operation {
    Ls { path: PathBuf },
    Touch { path: PathBuf },
    Read { path: PathBuf, range: Range<u64> },
    Write { path: PathBuf, range: Range<u64> },
}

pub struct Bench {
    operations: Vec<Operation>,
    clients: usize,
    partitions: Vec<Partition>,
    /// operations to run before the benchmark
    setup: Vec<Operation>,
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
            .map(|p| Partition::new(p, 2))
            .collect();
        let setup = Vec::new();

        Bench {
            operations,
            clients: 10,
            partitions,
            setup,
        }
    }
}

use serde::{Serialize, Deserialize};
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
