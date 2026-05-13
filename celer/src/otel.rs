//! Inicialización del stack de tracing + OpenTelemetry para CELER.
//!
//! Wire del frontend `tracing` a un exporter OTLP/gRPC que apunta a Jaeger
//! en `localhost:4317`. El subscriber compuesto tiene tres capas en orden:
//!
//!   1. `EnvFilter` — filtrado por nivel (controlado por RUST_LOG).
//!   2. `OpenTelemetryLayer` — bridge a OTel: cada `tracing::span!` produce
//!      un span OTel que se exporta vía batch processor.
//!   3. `fmt::layer` — pretty-print a stdout (mientras 1.7 no lo limpie).
//!
//! En CELER, el `SdkTracerProvider` retornado se mantiene vivo durante toda
//! la ejecución del proceso (asignado con `let _otel_provider`), pero NO
//! se llama `.shutdown()` explícito porque `warp::serve` bloquea para siempre.
//! Trade-off: en SIGINT/SIGKILL el último batch de spans no-flusheado se
//! pierde. Aceptable para demo portfolio; graceful shutdown deferido.

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
    Resource,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const OTLP_ENDPOINT: &str = "http://localhost:4317";

/// Inicializa el subscriber global de `tracing` con bridge a OpenTelemetry.
///
/// # Panics
/// Si el exporter OTLP no se puede construir (config inválida, no si Jaeger
/// no está corriendo — el batch exporter es resiliente al endpoint caído).
pub fn init_tracing(service_name: &'static str) -> SdkTracerProvider {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(OTLP_ENDPOINT)
        .build()
        .expect("Failed to build OTLP gRPC span exporter");

    let resource = Resource::builder()
        .with_service_name(service_name)
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = provider.tracer(service_name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with(otel_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    global::set_tracer_provider(provider.clone());

    provider
}
