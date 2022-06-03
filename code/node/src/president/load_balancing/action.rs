use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::directory::Staff;
use crate::president::Chart;
use crate::{Id, Idx};
use color_eyre::Result;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

use super::Init;
use crate::messages::{Msg, Reply};

#[instrument(err)]
async fn request_commit_idx(addr: SocketAddr, id: Id) -> Result<(Id, Idx), io::Error> {
    let stream = TcpStream::connect(addr).await?;
    let mut stream: connection::MsgStream<Reply, Msg> = connection::wrap(stream);
    stream.send(Msg::ReqCommitIdx).await?;
    loop {
        match stream.try_next().await? {
            None => continue,
            Some(Reply::CommitIdx(idx)) => return Ok((id, idx)),
        }
    }
}

#[instrument(skip(clerks), ret)]
async fn most_experienced(clerks: &[Id], chart: &Chart) -> Option<Id> {
    let mut requests: JoinSet<_> = clerks
        .into_iter()
        .map(|id| (chart.get_nth_addr::<1>(*id).unwrap(), id))
        .map(|(addr, id)| request_commit_idx(addr, *id))
        .fold(JoinSet::new(), |mut set, fut| {
            set.build_task().name("request_commit_idx").spawn(fut);
            set
        });

    let mut max_idx = 0;
    let mut most_experienced = None;
    while let Some(res) = requests.join_one().await.unwrap() {
        let (clerk, idx) = match res {
            Ok(res) => res,
            Err(_) => continue,
        };
        if idx > max_idx {
            max_idx = idx;
            most_experienced = Some(clerk);
        }
    }
    most_experienced
}

impl Init {
    async fn update_ministry_staff(&self, subtree: PathBuf, new_staff: Staff) {
        self.log_writer
            .append(crate::president::Order::AssignMinistry {
                subtree,
                staff: new_staff,
            })
            .await
            .committed()
            .await;
    }

    /// solve a ministry without minister by promoting the most experianced clerk
    #[instrument(skip(self))]
    pub(crate) async fn promote_clerk(&self, subtree: PathBuf) -> Result<(), &'static str> {
        let staff = self.staffing.staff(&subtree);
        let candidate = most_experienced(&staff.clerks, &self.chart)
            .await
            .ok_or("Could not contact any staff")?;

        let new_staff = Staff {
            minister: candidate,
            clerks: staff
                .clerks
                .iter()
                .copied()
                .filter(|id| *id != candidate)
                .collect(),
        };
        self.update_ministry_staff(subtree, new_staff).await;
        Ok(())
    }

    #[instrument(skip(self))]
    pub(crate) async fn try_assign(
        &mut self,
        subtree: PathBuf,
        down: Vec<u64>,
    ) -> Result<(), &'static str> {
        let staff = self.staffing.staff(&subtree);
        if staff.len() < 2 {
            return Err("Not enough staff left to restore ministry");
        }

        let idle = self.idle.drain().next().ok_or("No free staff left")?;
        let mut new_staff = staff.clone();
        new_staff.clerks.push(idle);

        self.update_ministry_staff(subtree, new_staff).await;
        Ok(())
    }
}
