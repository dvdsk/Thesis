use std::sync::Arc;

use gethostname::gethostname;
use mac_address::get_mac_address;
use structopt::StructOpt;
use tracing::info;

pub mod directory;
use directory::readserv;
mod consensus;
mod read_meta;
pub mod server_conn;
mod write_meta;

#[derive(Debug)]
pub struct Config {
    pub cluster_size: u16,
    pub client_port: u16,
    pub control_port: u16,
}

impl From<&Opt> for Config {
    fn from(opt: &Opt) -> Self {
        Self {
            control_port: opt.control_port,
            client_port: opt.client_port,
            cluster_size: opt.cluster_size,
        }
    }
}

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
    run_numb: i64,
}

fn setup_tracing(opt: &Opt) {
    use opentelemetry::KeyValue;
    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let instance_name = opt.name.clone().unwrap_or(
        gethostname()
            .into_string()
            .expect("hostname not valid utf8"),
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
    use tracing_subscriber::{fmt, Registry};
    let telemetry = tracing_opentelemetry::subscriber().with_tracer(tracer);
    let stdout = fmt::subscriber()
            .with_span_events(fmt::format::FmtSpan::NONE);
    //     .compact();
        // .with_span_list(false)
        // .with_current_span(false);
        // .with_target(false)
        // .with_level(true)

    let subscriber = Registry::default()
        .with(stdout)
        .with(telemetry);
    tracing::collect::set_global_default(subscriber).unwrap();
}

#[tracing::instrument]
async fn write_server(
    opt: Opt,
    dir: readserv::Directory,
    state: &Arc<consensus::State>,
    chart: discovery::Chart,
) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new(chart, opt.control_port);
    let dir = Directory::from(dir, servers, state);

    server(opt.client_port, dir).await;
    info!("starting write server");
}

#[tracing::instrument]
async fn host_meta_or_update(
    client_port: u16,
    state: &Arc<consensus::State>,
    chart: &discovery::Chart,
    dir: &readserv::Directory,
) {
    use read_meta::meta_server;
    loop {
        tokio::select! {
            _ = meta_server(client_port, dir, chart, &state) => panic!("should not return"),
            _ = state.outdated.notified() => (),
        }
        consensus::update(state, dir).await;
    }
}

#[tracing::instrument]
async fn read_server(
    opt: &Opt,
    state: &Arc<consensus::State>,
    chart: &discovery::Chart,
    dir: &readserv::Directory,
) {
    use consensus::election;

    // the meta server stops as soon as we detect the dir contains outdated info, then starts updating.
    // outdated detection happens inside the cmd server (from hb or update)
    //  - it notifies the meta_server which kills all current requests
    //  - then the meta server starts updating the directory using the master (if any)
    //    or blocks until there is a master
    loop {
        tokio::select! {
            () = read_meta::cmd_server(opt.control_port, state.clone(), dir) => todo!(),
            _res = host_meta_or_update(opt.client_port, &state, &chart, dir) => panic!("should not return"),
            _won = election::cycle(opt.control_port, &state, &chart) => {info!("won the election"); return},
        }
    }
}

#[tracing::instrument]
async fn server(
    opt: Opt,
    state: Arc<consensus::State>,
    mut dir: readserv::Directory,
    chart: discovery::Chart,
) {
    discovery::cluster(&chart, opt.cluster_size).await;
    info!("finished discovery");
    read_server(&opt, &state, &chart, &mut dir).await;

    info!("promoted to write server");
    let send_hb = consensus::maintain_heartbeat(state.clone(), chart.clone());
    let host = write_server(opt, dir, &state, chart);
    tokio::spawn(send_hb);
    host.await
}

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    let opt = Opt::from_args();
    setup_tracing(&opt);

    let id = id_from_mac();
    let (sock, chart) = discovery::setup(id, opt.control_port).await;
    let dir = readserv::Directory::new();
    let state = consensus::State::new(&opt, dir.get_change_idx());
    let state = Arc::new(state);

    tokio::spawn(server(opt, state, dir, chart.clone()));
    discovery::maintain(sock, chart).await;

    opentelemetry::global::shutdown_tracer_provider();
}

fn id_from_mac() -> u64 {
    let mac_bytes = get_mac_address()
        .unwrap()
        .expect("there should be at least one network device")
        .bytes();

    let mut id = 0u64.to_ne_bytes();
    id[0..6].copy_from_slice(&mac_bytes);
    u64::from_ne_bytes(id)
}
