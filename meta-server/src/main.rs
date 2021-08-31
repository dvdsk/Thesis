use structopt::StructOpt;
use tracing::{info, span, Level};

pub mod db;
mod read_meta;
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

    if let MetaRole::ReadServer = opt.role {
        loop {
            host_read_meta(opt.clone()).await;
            match todo!("elections") {
                Won => {
                    info!("Promoted to write meta server");
                    break;
                }
                Lost => continue,
            }
        }
    }

    host_write_meta(opt).await;
}
