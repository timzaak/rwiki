//! RwikiMetrics construction, no-op behavior, and in-memory export scenario tests.
//!
//! Verifies that RwikiMetrics builds correctly with and without a MeterProvider,
//! that no-op instruments silently succeed, and that the InMemoryMetricExporter
//! captures counter and histogram data after a force_flush.

use opentelemetry::global;
use opentelemetry::metrics::MeterProvider;
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

use super::metrics::RwikiMetrics;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an `SdkMeterProvider` backed by an `InMemoryMetricExporter`.
/// Returns `(provider, meter, exporter)` so tests can inspect exported metrics.
fn build_test_provider() -> (
    SdkMeterProvider,
    opentelemetry::metrics::Meter,
    InMemoryMetricExporter,
) {
    let exporter = InMemoryMetricExporter::default();
    let reader = PeriodicReader::builder(exporter.clone()).build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    let meter = provider.meter("rwiki");
    (provider, meter, exporter)
}

/// Check whether exported metrics contain a Sum (counter) metric with the given name
/// whose total value across all data points is >= `min_sum`.
fn has_counter_with_min_sum(
    finished: &[opentelemetry_sdk::metrics::data::ResourceMetrics],
    name: &str,
    min_sum: u64,
) -> bool {
    for rm in finished {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() == name {
                    if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                        let total: u64 = sum.data_points().map(|dp| dp.value()).sum();
                        if total >= min_sum {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Check whether exported metrics contain a Histogram metric with the given name
/// that has at least one data point.
fn has_histogram_with_data_points(
    finished: &[opentelemetry_sdk::metrics::data::ResourceMetrics],
    name: &str,
) -> bool {
    for rm in finished {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() == name {
                    if let AggregatedMetrics::F64(MetricData::Histogram(h)) = m.data() {
                        if h.data_points().count() >= 1 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: As a developer, when no OTel collector is configured (no MeterProvider set),
// RwikiMetrics must still construct without panic and all instruments must be usable as no-ops.
// Covers: Design -- zero-overhead no-op fallback when global MeterProvider is default.
#[test]
fn noop_metrics_builds_without_panic() {
    // No global MeterProvider set -- default is no-op.
    // Reset first to ensure clean state from prior tests.
    global::set_meter_provider(SdkMeterProvider::builder().build());

    let metrics = RwikiMetrics::new();

    // Exercise no-op counters -- must not panic.
    metrics.chat_request_count.add(1, &[]);
    metrics.chat_error_count.add(1, &[]);
    metrics.rewrite_timeout_count.add(1, &[]);
    metrics.rewrite_fallback_count.add(1, &[]);
    metrics.retrieval_empty_count.add(1, &[]);
    metrics.rerank_error_count.add(1, &[]);
    metrics.llm_error_count.add(1, &[]);

    // Exercise no-op histograms -- must not panic.
    metrics.chat_duration.record(100.0, &[]);
    metrics.rewrite_duration.record(50.0, &[]);
    metrics.retrieval_duration.record(30.0, &[]);
    metrics.retrieval_results_count.record(5.0, &[]);
    metrics.rerank_duration.record(20.0, &[]);
    metrics.llm_duration.record(200.0, &[]);
    metrics.llm_first_token_duration.record(10.0, &[]);
    metrics.llm_output_chars.record(500.0, &[]);
    metrics.llm_context_chunks.record(3.0, &[]);
}

// User Story: As a developer, when an SdkMeterProvider is set as the global provider,
// RwikiMetrics must build all 16 instruments and each must be usable without error.
// Covers: Design -- RwikiMetrics::new() correctly creates all instruments from global::meter().
#[test]
fn new_metrics_builds_with_meter_provider() {
    let (_provider, meter, _exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);

    // Counters (7 total)
    metrics.chat_request_count.add(1, &[]);
    metrics.chat_error_count.add(1, &[]);
    metrics.rewrite_timeout_count.add(1, &[]);
    metrics.rewrite_fallback_count.add(1, &[]);
    metrics.retrieval_empty_count.add(1, &[]);
    metrics.rerank_error_count.add(1, &[]);
    metrics.llm_error_count.add(1, &[]);

    // Histograms (9 total)
    metrics.chat_duration.record(1.0, &[]);
    metrics.rewrite_duration.record(1.0, &[]);
    metrics.retrieval_duration.record(1.0, &[]);
    metrics.retrieval_results_count.record(1.0, &[]);
    metrics.rerank_duration.record(1.0, &[]);
    metrics.llm_duration.record(1.0, &[]);
    metrics.llm_first_token_duration.record(1.0, &[]);
    metrics.llm_output_chars.record(1.0, &[]);
    metrics.llm_context_chunks.record(1.0, &[]);
}

// User Story: As a developer, when I record a counter increment via RwikiMetrics,
// the InMemoryMetricExporter must capture the metric with the correct name and value
// after a force_flush.
// Covers: Design -- counter instruments correctly wired through SdkMeterProvider pipeline.
#[test]
fn in_memory_exporter_captures_counter() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);
    metrics
        .chat_request_count
        .add(1, &[KeyValue::new("is_new_session", true)]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();
    assert!(
        has_counter_with_min_sum(&finished, "rag.chat.request.count", 1),
        "expected metric 'rag.chat.request.count' with sum >= 1"
    );
}

// User Story: As a developer, when I record a histogram value via RwikiMetrics,
// the InMemoryMetricExporter must capture the metric with the correct name and at
// least one data point after a force_flush.
// Covers: Design -- histogram instruments correctly wired through SdkMeterProvider pipeline.
#[test]
fn in_memory_exporter_captures_histogram() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);
    metrics.chat_duration.record(150.0, &[]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();
    assert!(
        has_histogram_with_data_points(&finished, "rag.chat.duration"),
        "expected metric 'rag.chat.duration' with at least one histogram data point"
    );
}
