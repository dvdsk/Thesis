use structopt::StructOpt;
use tracing::{info, span, Level};

pub mod db;
mod read_meta;
pub mod server_conn;
mod write_meta;

#[derive(Debug, Clone, StructOpt)]
struct Opt {
    #[structopt(long)]
    client_port: u16,

    #[structopt(long)]
    control_port: u16,

    #[structopt(long)]
    cluster_size: u16,
}

async fn host_write_meta(opt: Opt) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new();
    let db = Directory::new(servers.clone());

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = server(opt.client_port, db);

    info!("starting write server");
    futures::join!(maintain_conns, handle_req);
}

async fn host_read_meta(opt: Opt) {
    use read_meta::server;

    server(opt.client_port).await;
}

fn setup_tracing() {
    use tracing_subscriber::prelude::*;
    let fmt_sub = tracing_subscriber::fmt::subscriber().with_target(false);

    let subscriber = tracing_subscriber::Registry::default().with(fmt_sub);
    tracing::collect::set_global_default(subscriber).unwrap();
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    setup_tracing();

    let _span = span!(Level::TRACE, "server started").entered();
    // newly joining the cluster 
    // need to figure out ips of > 50% of the cluster. If we can not 
    // the entire cluster has failed or we have a bad connection. 
    discover_peers();

    loop {
        // use discoverd peer adresses to run election, keep open for 
        // recieving more peers (might have only found 51% or cluster is healing)
        let res = futures::select! {
            res = host_election() => {info!("election result"); res}
            err = maintain_discovery() => panic!("error while maintaining discovery: {:?}", err),
        };

        if let Winner = res {
            break;
        }

        futures::select! {
            err = host_read_meta() => info!("heartbeat timeout, starting election"),
            res = standby_elections() => info!("new leader elected: {}", res),
            err = maintain_discovery() => panic!("error while maintaining discovery: {:?}", err),
        }
    }

    let send_hb = send_heartbeats();
    let host = host_write_meta(opt);
    futures::join!(send_hb, host);
}
