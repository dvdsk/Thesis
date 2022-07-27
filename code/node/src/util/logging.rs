use opentelemetry::sdk::resource::Resource;
use opentelemetry::sdk::trace;
use opentelemetry::KeyValue;

use tracing_error::ErrorLayer;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::filter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use std::net::IpAddr;

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
        .with_auto_split_batch(true)
        .with_service_name("raft-fs")
        .install_batch(opentelemetry::runtime::Tokio)
        .unwrap();

    tracing_opentelemetry::layer().with_tracer(tracer)
}

pub fn setup_tracing(instance: String, endpoint: Option<IpAddr>, run: u16) {
    let filter = filter::EnvFilter::builder()
        .parse("warn,client=info,node::minister=debug,node::clerk=debug,testing::action::bench=info")
        // .parse("info,instance_chart=warn,node::raft::subjects=trace") // debug subject send
        // .parse("info,instance_chart=warn")
        .unwrap();

    let uptime = fmt::time::uptime();
    if let Some(endpoint) = endpoint {
        // ugly code duplication, needed or generics get angry
        let fmt_layer = fmt::layer()
            .pretty()
            .with_line_number(true)
            .with_timer(uptime);

        let telemetry = opentelemetry(instance, endpoint, run);
        let _ignore_err = tracing_subscriber::registry()
            .with(ErrorLayer::default())
            .with(filter)
            .with(telemetry)
            .with(fmt_layer)
            .try_init();
    } else {
        let fmt_layer = fmt::layer()
            .pretty()
            .with_line_number(true)
            .with_timer(uptime);
        let _ignore_err = tracing_subscriber::registry()
            .with(ErrorLayer::default())
            .with(filter)
            .with(fmt_layer)
            .try_init();
    }
}

#[allow(dead_code)] // used in integration testing
pub fn setup_integration_tracing(instance: String, endpoint: IpAddr, run: u16) {
    let filter = filter::EnvFilter::builder()
        .parse("info,instance_chart=warn")
        .unwrap();

    let telemetry = opentelemetry(instance, endpoint, run);
    let fmt = fmt::layer()
        .pretty()
        .with_line_number(true)
        .with_test_writer();

    // console_subscriber::init();
    let _ignore_err = tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(filter)
        .with(telemetry)
        .with(fmt)
        .try_init();
}

#[allow(dead_code)]
pub fn setup_test_tracing(additional_filter: &str) {
    let base_filter = "info,instance_chart=warn";
    let filter = filter::EnvFilter::builder()
        .parse(format!("{base_filter},{additional_filter}"))
        .unwrap();

    // console_subscriber::init();
    let fmt = fmt::layer()
        .pretty()
        .with_line_number(true)
        .with_test_writer();

    let _ignore_err = tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(filter)
        .with(fmt)
        .try_init();
}

pub fn setup_errors() {
    color_eyre::install().unwrap();
}
