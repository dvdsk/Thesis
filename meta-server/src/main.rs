use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use gethostname::gethostname;
use mac_address::get_mac_address;
use tokio::net::TcpListener;
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

#[derive(Debug, Clone, Subcommand)]
enum Command {
    Local {
        id: u64,
    },
    Deploy {
        #[clap(long)]
        client_port: u16,

        #[clap(long)]
        control_port: u16,
    },
}

#[derive(Debug, Clone, Parser)]
struct Opt {
    #[clap(subcommand)]
    command: Command,
    /// Name used to identify this instance in distributed tracing solutions
    #[clap(long)]
    name: Option<String>,

    #[clap(long)]
    cluster_size: u16,

    /// ip/host dns of the opentelemetry tracing endpoint
    #[clap(long, default_value = "localhost")]
    tracing_endpoint: String,

    /// optional run number defaults to -1
    #[clap(long, default_value = "-1")]
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
    use tracing_subscriber::Registry;
    let telemetry = tracing_opentelemetry::subscriber().with_tracer(tracer);

    let subscriber = Registry::default().with(telemetry);
    tracing::collect::set_global_default(subscriber).unwrap();
}

fn setup_logging() {
    use simplelog::*;
    let config = ConfigBuilder::default()
        .set_time_format_str("%T%.3f")
        .build();

    TermLogger::init(
        LevelFilter::Info,
        config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .expect("could not setup logger");
}

#[tracing::instrument(skip(dir, state, chart, hb_control))]
async fn write_server(
    dir: readserv::Directory,
    state: &Arc<consensus::State>,
    chart: discovery::Chart,
    hb_control: consensus::HbControl,
) {
    use write_meta::{server, Directory, ReadServers};

    let servers = ReadServers::new(chart, state.config.control_port);
    let dir = Directory::from(dir, servers, state, hb_control);

    server(state.config.client_port, dir).await;
    info!("starting write server");
}

#[tracing::instrument(skip(state, chart, dir))]
async fn host_meta_or_update(
    mut client_listener: TcpListener,
    state: &Arc<consensus::State>,
    chart: &discovery::Chart,
    dir: &readserv::Directory,
) {
    use read_meta::meta_server;
    loop {
        tokio::select! {
            _ = meta_server(&mut client_listener, dir, chart, &state) => panic!("should not return"),
            _ = state.outdated.notified() => (),
        }
        consensus::update(state, dir).await;
    }
}

#[tracing::instrument(skip(state, chart, dir))]
async fn read_server(
    control_listener: TcpListener,
    client_listener: TcpListener,
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
    let control_port = control_listener.local_addr().unwrap().port();
    loop {
        tokio::select! {
            () = read_meta::cmd_server(control_listener, state.clone(), dir) => todo!(),
            _res = host_meta_or_update(client_listener, &state, &chart, dir) => panic!("should not return"),
            _won = election::cycle(control_port, &state, &chart) => {info!("won the election"); return},
        }
    }
}

#[tracing::instrument(skip(state, dir, chart))]
async fn server(
    opt: Opt,
    control_listener: TcpListener,
    client_listener: TcpListener,
    state: Arc<consensus::State>,
    mut dir: readserv::Directory,
    chart: discovery::Chart,
) {
    discovery::cluster(chart.clone(), opt.cluster_size).await;
    info!("finished discovery");
    read_server(control_listener, client_listener, &state, &chart, &mut dir).await;

    info!("promoted to write server");
    let (hb_controller, rx) = consensus::HbControl::new();
    let send_hb = consensus::maintain_heartbeat(state.clone(), chart.clone(), rx);
    let host = write_server(dir, &state, chart, hb_controller);
    tokio::spawn(send_hb);
    host.await
}

#[tokio::main]
async fn main() {
    let opt = Opt::parse();
    setup_logging();
    // setup_tracing(&opt);

    let (control_port, client_port, id) = match opt.command {
        Command::Deploy {
            client_port,
            control_port,
        } => (client_port, control_port, id_from_mac()),
        Command::Local { id } => (0, 0, id), // let os pick ports
    };

    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), control_port);
    let control_listener = TcpListener::bind(addr).await.unwrap();
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), client_port);
    let client_listener = TcpListener::bind(addr).await.unwrap();

    let control_port = control_listener.local_addr().unwrap().port();
    let client_port = client_listener.local_addr().unwrap().port();
    info!("id: {id}, control port: {control_port}, client_port: {client_port}");

    let (sock, chart) = discovery::setup(id, control_port).await;
    let (dir, change_idx) = readserv::Directory::new();

    let config = Config { client_port, control_port, cluster_size: opt.cluster_size };
    let state = consensus::State::new(config, change_idx);
    let state = Arc::new(state);

    tokio::spawn(server(
        opt,
        control_listener,
        client_listener,
        state,
        dir,
        chart.clone(),
    ));
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
