use client::Client;
use itertools::{chain, Itertools};
use std::{
    ffi::OsString,
    iter,
    ops::Range,
    path::PathBuf,
    time::{Duration, Instant},
};
use tracing::instrument;

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

#[derive(Debug)]
pub struct Bench {
    operations: Vec<Operation>,
    clients: usize,
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
    ) -> Vec<Duration> {
        let mut res = Vec::new();
        for op in self.operations.into_iter() {
            let start = Instant::now();
            op.perform(client, buf).await;
            res.push(start.elapsed());
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
        // let pb = ProgressBar::new((self.additional_setup.len() + needed_files.len()) as u64);
        for op in chain!(
            self.additional_setup.iter().cloned(),
            needed_files.into_iter()
        ) {
            op.clone().perform(client, &mut buf).await;
            // pb.inc(1);
        }
        // pb.finish();
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

    pub fn ls_stride(n_parts: usize, n_ops: usize) -> Bench {
        let dirs = chain!(
            iter::once(String::from("/")),
            (1..n_parts).map(|n| format!("/{n}"))
        )
        .cycle()
        .take(n_ops * n_parts);
        Self::ls_access(dirs, n_parts)
    }
    pub fn ls_batch(n_parts: usize, n_ops: usize) -> Bench {
        let dirs = chain!(
            iter::once(String::from("/")),
            (0..n_parts).map(|n| format!("/{n}"))
        )
        .flat_map(|dir| iter::repeat(dir).take(n_ops));
        Self::ls_access(dirs, n_parts)
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
            .chain((1..10).into_iter().map(|n| format!("/{n}/")))
            .flat_map(|dir| {
                (0..10)
                    .into_iter()
                    .map(move |file| format!("{dir}{file}"))
                    .map(PathBuf::from)
                    .map(|path| Operation::Touch { path })
            })
            .collect();

        Bench {
            operations,
            clients: 3,
            partitions,
            additional_setup,
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
            Command::LsStride { n_parts } => Self::ls_stride(*n_parts, 1000),
            Command::LsBatch { n_parts } => Self::ls_batch(*n_parts, 1000),
        }
    }
}

impl Command {
    pub fn args(&self) -> OsString {
        match self {
            Command::LsStride { n_parts } => format!("ls-stride {n_parts}"),
            Command::LsBatch { n_parts } => format!("ls-batch {n_parts}"),
        }
        .into()
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
