use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::propagation::Injector;
use opentelemetry::propagation::TextMapCompositePropagator;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::{Subscriber, subscriber::set_global_default};
use tracing_log::LogTracer;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn get_subscriber() -> impl Subscriber + Sync + Send {
    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .or_else(|| std::env::var("K_SERVICE").ok())
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
        .build();

    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
        .ok()
        .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
        .unwrap_or_else(|| "http://127.0.0.1:4317".to_string());
    eprintln!("telemetry: OTLP exporter enabled at {otlp_endpoint}");

    let otlp_span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint.clone())
        .build()
        .expect("otlp grpc span exporter");

    let otlp_metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .build()
        .expect("otlp grpc metric exporter");

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(otlp_span_exporter)
        .with_resource(resource.clone())
        .build();

    let metric_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_periodic_exporter(otlp_metric_exporter)
        .with_resource(resource)
        .build();

    global::set_meter_provider(metric_provider);

    global::set_tracer_provider(tracer_provider.clone());
    global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));

    let tracer = tracer_provider.tracer("");

    let otel_layer = OpenTelemetryLayer::new(tracer);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    Registry::default()
        .with(env_filter)
        .with(otel_layer)
        .with(tracing_stackdriver::layer())
}

pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) {
    LogTracer::init().expect("Failed to set logger");
    set_global_default(subscriber).expect("Failed to set subscriber")
}

pub fn current_trace_parent() -> Option<String> {
    struct HeaderInjector {
        value: Option<String>,
    }

    impl Injector for HeaderInjector {
        fn set(&mut self, key: &str, value: String) {
            if key.eq_ignore_ascii_case("traceparent") {
                self.value = Some(value);
            }
        }
    }

    let mut injector = HeaderInjector { value: None };
    global::get_text_map_propagator(|propagator| {
        propagator.inject(&mut injector);
    });
    injector.value
}
