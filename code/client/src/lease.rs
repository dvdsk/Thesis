use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use futures::{SinkExt, TryStreamExt};
use protocol::Request;
use tokio::time::sleep;
use tracing::{instrument, debug, error};

use crate::{Client, RandomNode, RequestError};

pub struct Lease<'a, T: RandomNode> {
    lease_terms: protocol::Lease,
    client: &'a mut Client<T>,
}

impl<'a, T: RandomNode> Lease<'a, T> {
    #[instrument(skip(client), err(Debug))]
    pub async fn get_write(
        client: &'a mut Client<T>,
        path: &Path,
        range: Range<u64>,
    ) -> Result<Lease<'a, T>, RequestError> {
        let lease_terms = client
            .request(
                path,
                Request::Write {
                    path: path.to_owned(),
                    range,
                },
                true,
            )
            .await
            .unwrap();

        Ok(Lease {
            lease_terms,
            client,
        })
    }

    #[instrument(skip(client), err(Debug))]
    pub async fn get_read(
        client: &'a mut Client<T>,
        path: &Path,
        range: Range<u64>,
    ) -> Result<Lease<'a, T>, RequestError> {
        let lease_terms = client
            .request(
                path,
                Request::Read {
                    path: path.to_owned(),
                    range,
                },
                false,
            )
            .await
            .unwrap();

        Ok(Lease {
            lease_terms,
            client,
        })
    }

    #[instrument(skip(self), err)]
    pub async fn hold(self) -> Result<(), HoldError> {
        use HoldError::*;

        let stream = &mut self.client.conn.as_mut().unwrap().stream;

        let left = self.lease_terms.expires_in();
        let left = left.saturating_sub(Duration::from_millis(30));
        sleep(left).await;

        debug!("refreshing lease");
        stream
            .send(Request::RefreshLease)
            .await
            .map_err(ReqRefresh)?;

        loop {
            tokio::select! {
                _ = sleep(protocol::LEASE_TIMEOUT - Duration::from_millis(10)) => (),
                res = stream.try_next() => {
                    let response = res.map_err(GetRefresh)?.ok_or(ConnClosed)?;
                    match response {
                        protocol::Response::LeaseDropped => return Ok(()),
                        _ => unreachable!("LeaseDropped is the only msg we should recieve while holding lease")
                    }
                }
            } // select

            debug!("refreshing lease");
            stream // periodic lease refresh
                .send(Request::RefreshLease)
                .await
                .map_err(ReqRefresh)?;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HoldError {
    #[error("Io error requesting refresh: {0:?}")]
    ReqRefresh(std::io::Error),
    #[error("Io error recieving awnser to refresh request: {0:?}")]
    GetRefresh(std::io::Error),
    #[error("Connection was closed")]
    ConnClosed,
}
