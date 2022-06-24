use color_eyre::eyre::eyre;
use futures::{SinkExt, TryStreamExt};
use protocol::connection;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::timeout_at;
use tracing::{instrument, warn};

use crate::redirectory::{Node, Staff};
use crate::Idx;
use color_eyre::{Report, Result};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Init;
use protocol::{Request, Response};

#[derive(thiserror::Error, Debug)]
enum RequestErr {
    #[error("could not connect to clerk")]
    Connect(io::Error),
    #[error("could not send request for highest committed entry")]
    Send(io::Error),
    #[error("could not recieve response")]
    Recieve(io::Error),
    #[error("clerk disconnected before awnsering")]
    Disconnected,
}

#[instrument(err(Debug))]
async fn request_commit_idx(clerk: Node) -> Result<(Node, Idx), RequestErr> {
    use RequestErr::*;
    let stream = TcpStream::connect(clerk.client_addr().untyped())
        .await
        .map_err(Connect)?;
    let mut stream: connection::MsgStream<Response, Request> = connection::wrap(stream);
    stream.send(Request::HighestCommited).await.map_err(Send)?;
    loop {
        match stream.try_next().await.map_err(Recieve)? {
            None => return Err(Disconnected),
            Some(Response::HighestCommited(idx)) => return Ok((clerk, idx)),
            Some(_other) => unreachable!("should be awnsered with response highestcommitted"),
        }
    }
}

#[derive(thiserror::Error, Debug)]
enum ContactClerksErr {
    #[error("All clerks took too long to awnser")]
    AllTimedOut,
    #[error("Connections to all clerks errored")]
    AllErrored,
}

#[instrument(ret, err)]
async fn most_experienced(clerks: &[Node]) -> Result<Node, ContactClerksErr> {
    use ContactClerksErr::*;
    let mut requests: JoinSet<_> = clerks
        .iter()
        .map(|clerk| request_commit_idx(clerk.clone()))
        .fold(JoinSet::new(), |mut set, fut| {
            set.build_task().name("request_commit_idx").spawn(fut);
            set
        });

    let mut max_idx = 0;
    let mut most_experienced = None;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let request_done = timeout_at(deadline, requests.join_one());
        let response = match request_done.await {
            Ok(Some(resp)) => resp,
            Ok(None) => return most_experienced.ok_or(AllErrored), // no more tasks to join
            Err(_timeout) => return most_experienced.ok_or(AllTimedOut), // time is up
        };

        let (clerk, idx) = match response.expect("request commit idx task panicked") {
            Ok(res) => res,
            Err(e) => {
                warn!("io error reaching out to node: {e:?}");
                continue;
            }
        };

        if idx >= max_idx {
            max_idx = idx;
            most_experienced = Some(clerk);
        }
    }
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
    #[instrument(skip(self), err)]
    pub(crate) async fn promote_clerk(&self, subtree: &Path) -> Result<(), Report> {
        let staff = self.staffing.staff(subtree);
        let candidate = most_experienced(&staff.clerks).await?;

        let clerks = staff
            .clerks
            .iter()
            .cloned()
            .filter(|clerk| *clerk != candidate)
            .collect();
        let new_staff = Staff {
            minister: candidate,
            clerks,
            term: staff.term + 1,
        };

        self.update_ministry_staff(subtree.to_owned(), new_staff)
            .await;
        Ok(())
    }

    fn take_idle_node(&mut self) -> Result<Node, Report> {
        let id = *self.idle.keys().next().ok_or(eyre!("No free idle nodes left"))?;
        Ok(self.idle.remove(&id).unwrap())
    }

    #[instrument(skip(self), err)]
    pub(crate) async fn try_assign(&mut self, subtree: &Path, down: &[u64]) -> Result<(), Report> {
        let staff = self.staffing.staff(subtree);
        if staff.len() < 2 {
            return Err(eyre!("Not enough staff left to restore ministry"));
        }

        let mut new_staff = staff.clone();
        new_staff.clerks.push(self.take_idle_node()?);
        new_staff.term += 1;

        self.update_ministry_staff(subtree.to_owned(), new_staff)
            .await;
        Ok(())
    }
}
