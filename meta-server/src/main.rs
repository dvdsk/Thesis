use structopt::StructOpt;
use tracing::{info, span, Level};

pub mod db;
pub mod server_conn;
mod write_meta;

#[derive(Debug, Clone, StructOpt)]
enum MetaRole {
    ReadServer,
    WriteServer,
}

#[derive(Debug, Clone, StructOpt)]
struct Opt {
    #[structopt(long)]
    client_port: u16,

    #[structopt(long)]
    control_port: u16,

    #[structopt(subcommand)]
    role: MetaRole,
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

async fn host_read_meta(opt: Opt) {}

use opentelemetry::global;
use opentelemetry::trace::Tracer;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing::{error};
fn setup_tracing() -> () {
    // use tracing_opentelemetry::OpenTelemetryLayer;
    // use tracing_subscriber::layer::SubscriberExt;
    // use tracing_subscriber::Registry;
    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name("report_example")
        .with_collector_endpoint("http://localhost:14268/api/traces")
        // .install_batch(opentelemetry::runtime::Tokio)
        .install_simple()
        .unwrap();

    use tracing_subscriber::prelude::*;
    let opt = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry().with(opt).try_init().unwrap();

    let root = span!(tracing::Level::INFO, "app_start");
    let _enter = root.enter();
}

// #[tokio::main]
// async fn main() {
fn main() {
    // setup_stdout_logger();
    setup_tracing();
    let root = span!(tracing::Level::INFO, "app_start3");
    let _enter = root.enter();
    info!("yo");

    opentelemetry::global::shutdown_tracer_provider(); // sending remaining spans
    std::thread::sleep_ms(1000);
    // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

// #[tokio::main]
// async fn main() {
//     // let opt = Opt::from_args();
//     test();

//     // let _span = span!(Level::TRACE, "shaving_yaks").entered();
//     // info!("test");

//     // if let MetaRole::ReadServer = opt.role {
//     //     host_read_meta(opt.clone()).await;
//     //     info!("Promoted to write meta server");
//     // }

//     // host_write_meta(opt).await;
// }
