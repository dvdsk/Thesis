use crate::{Chart, Id};
use async_trait::async_trait;
use std::fmt::Debug;
use std::net::SocketAddr;

#[async_trait]
pub trait SourceNotify {
    type Error: Debug;
    async fn recv_new(&mut self) -> Result<(Id, SocketAddr), Self::Error>;
}

#[async_trait]
impl SourceNotify for instance_chart::Notify<3, u16> {
    type Error = tokio::sync::broadcast::error::RecvError;
    async fn recv_new(&mut self) -> Result<(Id, SocketAddr), Self::Error> {
        self.recv_nth_addr::<0>().await
    }
}

#[async_trait]
pub trait Source {
    type Notify: SourceNotify;
    fn notify(&mut self) -> Self::Notify;
    fn our_id(&self) -> Id;
    fn adresses(&mut self) -> Vec<(Id, SocketAddr)>;
    fn forget(&self, id: Id) {
        self.forget_impl(id)
    }
    fn forget_impl(&self, id: Id);
}

impl Source for Chart {
    type Notify = instance_chart::Notify<3, u16>;

    fn notify(&mut self) -> Self::Notify {
        Chart::notify(self)
    }
    fn our_id(&self) -> Id {
        self.our_id()
    }
    fn adresses(&mut self) -> Vec<(Id, SocketAddr)> {
        self.nth_addr_vec::<0>()
    }
    fn forget_impl(&self, id: Id) {
        Chart::forget(self, id)
    }
}
