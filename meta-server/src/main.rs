use structopt::StructOpt;
use tokio::sync::mpsc;
use tokio::net::UdpSocket;
use tracing::{info, info_span};
use tracing_futures::Instrument;
use futures::future::FutureExt;
use mac_address::get_mac_address;

pub mod db;
mod read_meta;
mod write_meta;
pub mod server_conn;
use server_conn::election;
use server_conn::election::maintain_heartbeat;

#[derive(Debug, Clone, StructOpt)]
struct Opt {
    #[structopt(long)]
    client_port: u16,

    #[structopt(long)]
    control_port: u16,

    #[structopt(long)]
    cluster_size: u16,

    #[structopt(long, default_value="localhost")]
    tracing_endpoint: String,
}

fn setup_tracing(endpoint: &str) {
    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let url = format!("http://{}:14268/api/traces", endpoint);
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name("minimal-example")
        .with_collector_endpoint(url)
        .install_simple()
        .unwrap();
    
    // use opentelemetry::sdk::export::trace;
    // let tracer = trace::stdout::new_pipeline().install_simple();

    use tracing_subscriber::prelude::*;
    let telemetry = tracing_opentelemetry::subscriber().with_tracer(tracer);
    let fmt_sub = tracing_subscriber::fmt::subscriber().with_target(false);

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt_sub)
        .with(telemetry);
    tracing::collect::set_global_default(subscriber).unwrap();
}

async fn write_server(opt: &Opt) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new();
    let db = Directory::new(servers.clone());

    let maintain_conns = ReadServers::maintain(servers.conns.clone(), opt.control_port);
    let handle_req = server(opt.client_port, db);

    info!("starting write server");
    futures::join!(maintain_conns, handle_req);
}

async fn read_server(opt: &Opt, state: &'_ mut election::State<'_>) {
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

async fn server(opt: Opt, mut state: election::State<'_>, sock: &UdpSocket, chart: &discovery::Chart) {
    println!("mmmmmmmmmmmmm");
    discovery::cluster(sock, chart, opt.cluster_size).await;
    println!("discovery done");
    info!("finished discovery");
    read_server(&opt, &mut state).await;

    info!("promoted to readserver");
    let send_hb = maintain_heartbeat(&state);
    let host = write_server(&opt);
    futures::join!(send_hb, host);
}

#[tokio::main]
async fn main() {
    println!("hello world");
    let opt = Opt::from_args();
    // setup_tracing(&opt.tracing_endpoint);

    // let sp = info_span!("setup");
    // let ro = sp.enter();

    let id = get_mac_address()
        .unwrap()
        .expect("there should be at least one network decive")
        .to_string();
    let (sock, chart) = discovery::setup(id).await;
    let state = election::State::new(opt.cluster_size, &chart);

    // std::mem::drop(ro);
    // std::mem::drop(sp);

    let f1 = server(opt, state, &sock, &chart);
    let f2 = discovery::maintain(&sock, &chart);
    futures::join!(f1,f2);

    opentelemetry::global::shutdown_tracer_provider();
}
