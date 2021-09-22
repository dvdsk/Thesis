use std::sync::Arc;

use futures::future::FutureExt;
use gethostname::gethostname;
use mac_address::get_mac_address;
use structopt::StructOpt;
use tokio::net::UdpSocket;
use tracing::info;
use tracing_futures::Instrument;

pub mod directory;
use directory::readserv;
mod consensus;
mod read_meta;
pub mod server_conn;
mod write_meta;

#[derive(Debug, Clone, StructOpt)]
struct Opt {
    /// Name used to identify this instance in distributed tracing solutions
    #[structopt(long)]
    name: Option<String>,

    #[structopt(long, default_value = "23811")]
    client_port: u16,

    #[structopt(long, default_value = "23812")]
    control_port: u16,

    #[structopt(long)]
    cluster_size: u16,

    /// ip/host dns of the opentelemetry tracing endpoint
    #[structopt(long, default_value = "localhost")]
    tracing_endpoint: String,

    /// optional run number defaults to -1
    #[structopt(long, default_value = "-1")]
    run_numb: i64
}

fn setup_tracing(opt: &Opt) {
    use opentelemetry::KeyValue;
    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let instance_name = opt.name.clone().unwrap_or(
        gethostname()
            .into_string()
            .expect("hostname is not valid utf8"),
    );

    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_agent_endpoint(format!("{}:6831", opt.tracing_endpoint))
        .with_service_name("mock-fs")
        .with_tags(vec![
            KeyValue::new("instance", instance_name.to_owned()),
            KeyValue::new("run", opt.run_numb),
        ])
        .install_batch(opentelemetry::runtime::Tokio)
        .unwrap();

    use tracing_subscriber::prelude::*;
    let telemetry = tracing_opentelemetry::subscriber().with_tracer(tracer);
    let fmt_sub = tracing_subscriber::fmt::subscriber().with_target(false);

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_sub)
        .with(telemetry);
    tracing::collect::set_global_default(subscriber).unwrap();
}

async fn write_server(opt: Opt, dir: readserv::Directory, state: &Arc<consensus::State>) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new();
    let dir = Directory::from(dir, servers.clone(), state);

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = server(opt.client_port, dir);

    info!("starting write server");
    futures::join!(maintain_conns, handle_req);
}

async fn host_meta_or_update(
    client_port: u16,
    state: &consensus::State,
    dir: &readserv::Directory,
) {
    use read_meta::meta_server;
    loop {
        println!("host_meta_or_update loop");
        futures::select! {
            _ = meta_server(client_port).fuse() => panic!("should not return"),
            _ = state.outdated.notified().fuse() => (),
        }
        consensus::update(state, dir).await;
    }
}

async fn read_server(
    opt: &Opt,
    state: &Arc<consensus::State>,
    chart: &discovery::Chart,
    dir: &readserv::Directory,
) {
    use consensus::election;

    // the meta server stops as soon as we detect we are outdated, then starts updating.
    // outdated detection happens inside the cmd server (from hb or update)
    //  - it notifies the meta_server which kills all current requests
    //  - then the meta server starts updating the directory using the master (if any)
    //    or blocks until there is a master
    loop {
        futures::select! {
            () = read_meta::cmd_server(opt.control_port, state.clone(), dir).fuse() => todo!(),
            _res = host_meta_or_update(opt.client_port, &state, dir).fuse() => panic!("should not return"),
            _won = election::cycle(opt.control_port, &state, chart).fuse() => {info!("won the election"); return},
        }
    }
}

async fn server(
    opt: Opt,
    state: Arc<consensus::State>,
    mut dir: readserv::Directory,
    sock: &UdpSocket,
    chart: &discovery::Chart,
) {
    discovery::cluster(sock, chart, opt.cluster_size).await;
    info!("finished discovery");
    read_server(&opt, &state, chart, &mut dir).await;

    info!("promoted to write server");
    let send_hb = consensus::maintain_heartbeat(&state, chart);
    let host = write_server(opt, dir, &state);
    futures::join!(send_hb, host);
}

#[tokio::main]
async fn main() {
    println!("prog strt");
    let opt = Opt::from_args();
    setup_tracing(&opt);

    let id = id_from_mac();
    let (sock, chart) = discovery::setup(id, opt.control_port).await;
    let dir = readserv::Directory::new();
    let state = consensus::State::new(opt.cluster_size, dir.get_change_idx());
    let state = Arc::new(state);

    let f1 = server(opt, state, dir, &sock, &chart);
    let f2 = discovery::maintain(&sock, &chart);
    futures::join!(f1, f2);

    opentelemetry::global::shutdown_tracer_provider();
}

fn id_from_mac() -> u64 {
    let mac_bytes = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .bytes();

    let mut id = 0u64.to_ne_bytes();
    id[0..6].copy_from_slice(&mac_bytes);
    u64::from_ne_bytes(id)
}
