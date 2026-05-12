//! Inicialización del stack de tracing + OpenTelemetry para AEGIS.
//!
//! Wire del frontend `tracing` a un exporter OTLP/gRPC que apunta a Jaeger
//! en `localhost:4317`. El subscriber compuesto tiene tres capas en orden:
//!
//!   1. `EnvFilter` — filtrado por nivel (controlado por RUST_LOG).
//!   2. `OpenTelemetryLayer` — bridge a OTel: cada `tracing::span!` produce
//!      un span OTel que se exporta vía batch processor.
//!   3. `fmt::layer` — pretty-print a stdout (mientras 1.7 no lo limpie).
//!
//! El `SdkTracerProvider` retornado DEBE mantenerse vivo durante toda la
//! ejecución y ser apagado explícitamente con `.shutdown()` antes de que
//! el runtime tokio termine, para flush de los spans en cola.

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
    // 1. Exporter: gRPC/tonic hacia el endpoint OTLP de Jaeger.
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(OTLP_ENDPOINT)
        .build()
        .expect("Failed to build OTLP gRPC span exporter");

    // 2. Resource: identifica este servicio en Jaeger.
    let resource = Resource::builder()
        .with_service_name(service_name)
        .build();

    // 3. Provider con batch exporter (el simple exporter tiene un bug
    //    conocido de hang infinito con tonic — issue #204 de tracing-opentelemetry).
    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    // 4. Propagador W3C traceparent. Necesario para que CELER pueda
    //    reconstruir el SpanContext desde el trace_id del StrokeEvent.
    global::set_text_map_propagator(TraceContextPropagator::new());

    // 5. Bridge a tracing: cada `tracing::span!` se convierte en span OTel.
    let tracer = provider.tracer(service_name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // 6. Subscriber compuesto: filter -> otel -> stdout.
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with(otel_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 7. Global tracer provider. Permite que opentelemetry::global::tracer()
    //    funcione en cualquier parte del workspace (CELER lo necesita para
    //    reconstruir contexto desde el trace_id propagado).
    global::set_tracer_provider(provider.clone());

    provider
}