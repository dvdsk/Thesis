use structopt::StructOpt;
use tokio::sync::mpsc;
use tokio::net::UdpSocket;
use tracing::{info, warn, span, Level};
use futures::future::FutureExt;
use mac_address::get_mac_address;

pub mod db;
mod read_meta;
mod write_meta;
pub mod server_conn;
use server_conn::election::election_cycle;

use crate::server_conn::election::maintain_heartbeat;

use self::server_conn::election;

#[derive(Debug, Clone, Copy, StructOpt)]
struct Opt {
    #[structopt(long)]
    client_port: u16,

    #[structopt(long)]
    control_port: u16,

    #[structopt(long)]
    cluster_size: u16,
}

fn setup_tracing() {
    use tracing_subscriber::prelude::*;
    let fmt_sub = tracing_subscriber::fmt::subscriber().with_target(false);

    let subscriber = tracing_subscriber::Registry::default().with(fmt_sub);
    tracing::collect::set_global_default(subscriber).unwrap();
}

async fn write_server(opt: Opt) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new();
    let db = Directory::new(servers.clone());

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = server(opt.client_port, db);

    info!("starting write server");
    futures::join!(maintain_conns, handle_req);
}

async fn read_server(opt: Opt, state: &mut election::State) {
    use read_meta::meta_server;
    // the cmd server forwards election related msgs to the
    // election_cycle. This is better then stopping the cmd server
    // and starting a election server as that risks losing packages
    let (tx, rx) = mpsc::channel(10);

    futures::select! {
        () = read_meta::cmd_server(opt.control_port, tx).fuse() => todo!(),
        _res = meta_server(opt.client_port).fuse() => panic!("meta server should not return"),
        _won = election_cycle(rx, state).fuse() => info!("won the election"),
    }
}

async fn server(opt: Opt, mut state: election::State, sock: &UdpSocket, chart: &discovery::Chart) {
    discovery::cluster(sock, chart, opt.cluster_size).await;
    info!("finished discovery");
    read_server(opt, &mut state).await;

    info!("promoted to readserver");
    let send_hb = maintain_heartbeat(state);
    let host = write_server(opt);
    futures::join!(send_hb, host);
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    setup_tracing();

    let state = election::State::new();
    let (sock, chart) = discovery::setup(state.id.to_string()).await;
    let _span = span!(Level::TRACE, "server started").entered();

    let f1 = server(opt, state, &sock, &chart);
    let f2 = discovery::maintain(&sock, &chart);
    futures::join!(f1,f2);
}
