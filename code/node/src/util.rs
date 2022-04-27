use color_eyre::eyre::{Result, WrapErr};
use opentelemetry::sdk::resource::Resource;
use opentelemetry::sdk::trace;
use opentelemetry::KeyValue;
use result_tools::*;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::filter;
use tracing_subscriber::prelude::*;

use std::fs;
use std::io;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::path::Path;
use std::path::PathBuf;
use tokio::net::TcpListener;

mod db;
pub use db::TypedSled;

fn opentelemetry<S>(
    instance: String,
    endpoint: IpAddr,
    run: u16,
) -> OpenTelemetryLayer<S, opentelemetry::sdk::trace::Tracer>
where
    S: tracing::subscriber::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

    let run_numb = run.to_string();
    let resouces = vec![
        KeyValue::new("instance", instance),
        KeyValue::new("run", run_numb),
    ];
    let config = trace::Config::default().with_resource(Resource::new(resouces));

    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_trace_config(config)
        .with_agent_endpoint((endpoint, 6831))
        .with_service_name("raft-fs")
        .install_batch(opentelemetry::runtime::Tokio)
        .unwrap();

    tracing_opentelemetry::layer().with_tracer(tracer)
}

pub fn setup_tracing(instance: String, endpoint: IpAddr, run: u16) {
    let filter = filter::EnvFilter::builder()
        .parse("info,multicast_discovery=debug")
        .unwrap();

    let telemetry = opentelemetry(instance, endpoint, run);
    let fmt = tracing_subscriber::fmt::layer()
        .pretty()
        .with_line_number(true);

    let _ignore_err = tracing_subscriber::registry()
        .with(filter)
        .with(telemetry)
        .with(fmt)
        .try_init();
}

pub fn setup_errors() {
    color_eyre::install().unwrap();
}

pub async fn open_socket(port: Option<NonZeroU16>) -> Result<(TcpListener, u16)> {
    let ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let addr = SocketAddr::new(ip, port.map(NonZeroU16::get).unwrap_or(0));
    let listener = TcpListener::bind(addr)
        .await
        .wrap_err("Could not bind to address: {addr}")?;

    let open_port = listener.local_addr().unwrap().port();
    match port {
        None => tracing::trace!("OS assigned free TCP port: {open_port}"),
        Some(p) => tracing::trace!("opend TCP port: {p}"),
    }
    Ok((listener, open_port))
}

pub fn runtime_dir() -> PathBuf {
    use std::io::ErrorKind::AlreadyExists;
    let temp = std::env::temp_dir().join("thesis");
    fs::create_dir(&temp)
        .to_ok_if(|e| e.kind() == AlreadyExists)
        .unwrap();
    temp
}

pub fn run_number(dir: &Path) -> u16 {
    let path = dir.join("run.txt");
    let run = match fs::read_to_string(&path).map_err(|e| e.kind()) {
        Err(io::ErrorKind::NotFound) => 0,
        Err(e) => panic!("could not access run numb file: {e:?}"),
        Ok(run) => run.parse().unwrap(),
    };
    fs::write(path, (run + 1).to_string().as_bytes()).unwrap();
    run
}

use tokio::task;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Spawn a new tokio Task and cancel it on drop.
pub fn spawn<T>(future: T) -> Wrapper<T::Output>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    Wrapper(task::spawn(future))
}

/// Cancels the wrapped tokio Task on Drop.
pub struct Wrapper<T>(task::JoinHandle<T>);

impl<T> Future for Wrapper<T>{
    type Output = Result<T, task::JoinError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { Pin::new_unchecked(&mut self.0) }.poll(cx)
    }
}

impl<T> Drop for Wrapper<T> {
    fn drop(&mut self) {
        // do `let _ = self.0.cancel()` for `async_std::task::Task`
        self.0.abort();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    mod run_number {
        use mktemp::Temp;

        use super::*;
        #[test]
        fn increases() {
            let dir = Temp::new_dir().unwrap();
            for correct in 0..10 {
                let run_numb = run_number(&dir);
                assert_eq!(run_numb, correct);
            }
        }
    }
}
