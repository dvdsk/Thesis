use std::net::IpAddr;

use opentelemetry::sdk::resource::Resource;
use opentelemetry::sdk::trace;
use opentelemetry::KeyValue;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::filter;
use tracing_subscriber::prelude::*;

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
        .with_agent_endpoint(format!("{}:6831", endpoint))
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
        .with_line_number(true)
        .with_test_writer();

    let _ignore_err = tracing_subscriber::registry()
        .with(filter)
        .with(telemetry)
        .with(fmt)
        .try_init();
}

pub fn setup_errors() {
    color_eyre::install().unwrap();
}
