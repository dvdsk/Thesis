use color_eyre::eyre::eyre;
use color_eyre::{Result, Section, SectionExt};
use futures::{SinkExt, TryStreamExt};
use protocol::connection::MsgStream;
use protocol::{connection, Request, Response};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::president::{self, Log, Order};
use crate::redirectory::{Node, ReDirectory};
use crate::{Id, Role};

async fn handle_pres_orders(
    pres_orders: &mut Log<president::Order>,
    redirectory: &mut ReDirectory,
    id: Id,
) -> Result<Role> {
    loop {
        let order = pres_orders.recv().await;
        redirectory.update(&order).await;
        match order {
            Order::AssignMinistry { subtree, staff } => {
                if staff.minister.id == id {
                    return Ok(Role::Minister {
                        term: staff.term,
                        subtree,
                        clerks: staff.clerks,
                    });
                }

                if staff.clerks.contains(&Node::local(id)) {
                    return Ok(Role::Clerk { subtree });
                }
            }
            Order::Assigned(role) => return Ok(role),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            m => {
                return Err(eyre!("recieved wrong msg"))
                    .with_section(move || format!("{m:?}").header("Msg:"))
            }
        }
    }
}

pub async fn redirect_clients(listener: &mut TcpListener, redirect: ReDirectory) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, redirect.clone());
        request_handlers
            .build_task()
            .name("idle client conn")
            .spawn(handle);
    }
}

async fn handle_conn(stream: TcpStream, redirect: ReDirectory) {
    use Request::*;
    let mut stream: MsgStream<Request, Response> = connection::wrap(stream);
    while let Ok(Some(req)) = stream.try_next().await {
        debug!("idle got request: {req:?}");

        let reply = match req {
            List(path)
            | Create(path)
            | IsCommitted { path, .. }
            | Write { path, .. }
            | Read { path, .. } => {
                let (staff, subtree) = redirect.to_staff(&path).await;
                Response::Redirect {
                    addr: staff.minister.addr,
                    subtree,
                }
            }
            RefreshLease | Lock { .. } | Unlock { .. } | UnlockAll { .. } => {
                warn!("idle node recieved inappropriate request");
                return;
            }
        };

        if let Err(e) = stream.send(reply).await {
            warn!("error replying to message: {e:?}");
            return;
        }
    }
}

pub(crate) async fn work(state: &mut super::State) -> Result<Role> {
    let super::State {
        pres_orders,
        redirectory,
        client_listener,
        id,
        ..
    } = state;

    redirectory.set_tree(None);

    tokio::select! {
        () = redirect_clients(client_listener, redirectory.clone()) => unreachable!(),
        new_role = handle_pres_orders(pres_orders, redirectory, *id) => new_role,
    }
}
