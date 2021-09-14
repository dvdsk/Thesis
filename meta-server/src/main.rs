use futures::future::FutureExt;
use gethostname::gethostname;
use mac_address::get_mac_address;
use structopt::StructOpt;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::info;
use tracing_futures::Instrument;

pub mod directory;
use directory::readserv;
mod read_meta;
pub mod server_conn;
mod write_meta;
use server_conn::election;
use server_conn::election::maintain_heartbeat;

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
}

fn setup_tracing(instance_name: &str, endpoint: &str) {
    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name(format!("mock-fs {}", instance_name))
        .with_agent_endpoint(format!("{}:6831", endpoint))
        .install_simple()
        .unwrap();

    use tracing_subscriber::prelude::*;
    let telemetry = tracing_opentelemetry::subscriber().with_tracer(tracer);
    let fmt_sub = tracing_subscriber::fmt::subscriber().with_target(false);

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_sub)
        .with(telemetry);
    tracing::collect::set_global_default(subscriber).unwrap();
}

#[tracing::instrument]
async fn write_server(opt: Opt, dir: readserv::Directory) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new();
    let dir = Directory::from(dir, servers.clone());

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = server(opt.client_port, dir);

    info!("starting write server");
    futures::join!(maintain_conns, handle_req);
}

#[tracing::instrument]
async fn read_server(opt: &Opt, state: &'_ mut election::State<'_>, dir: &mut readserv::Directory) {
    use read_meta::meta_server;
    // the cmd server forwards election related msgs to the
    // election_cycle. This is better then stopping the cmd server
    // and starting a election server as that risks losing packages
    let (tx, rx) = mpsc::channel(10);

    futures::select! {
        () = read_meta::cmd_server(opt.control_port, tx).fuse() => todo!(),
        _res = meta_server(opt.client_port).fuse() => panic!("meta server should not return"),
        _won = election::cycle(rx, state).fuse() => info!("won the election"),
    }
}

#[tracing::instrument]
async fn server(
    opt: Opt,
    mut state: election::State<'_>,
    mut dir: readserv::Directory,
    sock: &UdpSocket,
    chart: &discovery::Chart,
) {
    discovery::cluster(sock, chart, opt.cluster_size).await;
    info!("finished discovery");
    read_server(&opt, &mut state, &mut dir).await;

    info!("promoted to readserver");
    let send_hb = maintain_heartbeat(&state);
    let host = write_server(opt, dir);
    futures::join!(send_hb, host);
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let instance_name = opt.name.clone().unwrap_or(
        gethostname()
            .into_string()
            .expect("hostname is not valid utf8"),
    );
    setup_tracing(&instance_name, &opt.tracing_endpoint);

    let id = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .to_string();
    let (sock, chart) = discovery::setup(id).await;
    let dir = readserv::Directory::new();
    let state = election::State::new(opt.cluster_size, &chart);

    let f1 = server(opt, state, dir, &sock, &chart);
    let f2 = discovery::maintain(&sock, &chart);
    futures::join!(f1, f2);

    opentelemetry::global::shutdown_tracer_provider();
}
