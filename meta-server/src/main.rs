use structopt::StructOpt;
use tracing::{info, warn, span, Level};
use futures::future::FutureExt;

pub mod db;
mod read_meta;
mod write_meta;
pub mod server_conn;
use server_conn::election::{self, host_election, maintain_heartbeat};
use server_conn::discovery::{discover_peers, maintain_discovery};

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

async fn elect_and_cmd_server(port: u16) {
    use election::ElectionResult::Winner;
    use election::monitor_heartbeat;
    use read_meta::cmd_server;

    loop {
        futures::select! {
            () = cmd_server(port).fuse() => warn!("could not reach write server"),
            () = monitor_heartbeat().fuse() => warn!("heartbeat timed out")
        }

        if let Winner = host_election(port).await {
            info!("won election");
            break;
        }
    }
}

async fn read_server(opt: Opt) {
    use read_meta::meta_server;

    futures::select! {
        _res = meta_server(opt.client_port).fuse() => (),
        _res = elect_and_cmd_server(opt.control_port).fuse() => (),
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    setup_tracing();

    let _span = span!(Level::TRACE, "server started").entered();
    // newly joining the cluster
    // need to figure out ips of > 50% of the cluster. If we can not
    // the entire cluster has failed or we have a bad connection.
    discover_peers().await;

    futures::select! {
        () = read_server(opt).fuse() => (), // must have won election 
        err = maintain_discovery().fuse() => panic!("error while maintaining discovery: {:?}", err),
    };

    let send_hb = maintain_heartbeat();
    let host = write_server(opt);
    futures::join!(send_hb, host);
}
