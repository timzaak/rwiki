//! Integration tests verifying RAG pipeline metrics are recorded when
//! instruments are exercised via the api crate's `RwikiMetrics`.
//!
//! Uses `InMemoryMetricExporter` to capture and assert on metric values,
//! following the same pattern established in `core::infrastructure::metrics_scenarios`.

use std::collections::HashSet;

use opentelemetry::metrics::MeterProvider;
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

use rwiki_core::infrastructure::metrics::RwikiMetrics;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn collect_metric_names(
    finished: &[opentelemetry_sdk::metrics::data::ResourceMetrics],
) -> HashSet<String> {
    let mut names = HashSet::new();
    for rm in finished {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                names.insert(m.name().to_string());
            }
        }
    }
    names
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: As a platform engineer, I need to verify that all primary RAG
// pipeline metrics (counters and histograms) are recorded and captured by the
// OTel pipeline when a realistic request sequence is simulated.
// Covers: BE-D01 (all 16 instruments), BE-D02 (RwikiMetrics integration).
#[test]
fn metrics_instruments_are_recordable_under_meter_provider() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);

    // Simulate a realistic chat request sequence
    metrics
        .chat_request_count
        .add(1, &[KeyValue::new("is_new_session", true)]);
    metrics
        .rewrite_duration
        .record(50.0, &[KeyValue::new("is_first_turn", true)]);
    metrics
        .retrieval_duration
        .record(120.0, &[KeyValue::new("search_type", "hybrid")]);
    metrics.retrieval_results_count.record(5.0, &[]);
    metrics.llm_duration.record(2000.0, &[]);
    metrics.llm_first_token_duration.record(300.0, &[]);
    metrics.llm_output_chars.record(450.0, &[]);
    metrics.llm_context_chunks.record(5.0, &[]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();

    assert!(
        has_counter_with_min_sum(&finished, "rag.chat.request.count", 1),
        "expected rag.chat.request.count with sum >= 1"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.rewrite.duration"),
        "expected rag.rewrite.duration histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.retrieval.duration"),
        "expected rag.retrieval.duration histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.retrieval.results.count"),
        "expected rag.retrieval.results.count histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.llm.duration"),
        "expected rag.llm.duration histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.llm.first_token.duration"),
        "expected rag.llm.first_token.duration histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.llm.output.chars"),
        "expected rag.llm.output.chars histogram data"
    );
    assert!(
        has_histogram_with_data_points(&finished, "rag.llm.context.chunks"),
        "expected rag.llm.context.chunks histogram data"
    );
}

// User Story: As a platform engineer, I need to confirm that empty retrieval
// results are tracked via a dedicated counter, so alerting can detect retrieval
// degradation.
// Covers: BE-D01 (retrieval_empty_count counter), BE-D03 (retrieval stage metrics).
#[test]
fn retrieval_empty_counter_increments() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);
    metrics.retrieval_empty_count.add(1, &[]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();
    assert!(
        has_counter_with_min_sum(&finished, "rag.retrieval.empty.count", 1),
        "expected rag.retrieval.empty.count with sum >= 1"
    );
}

// User Story: As a platform engineer, I need all error and timeout counters
// to be captured by the OTel pipeline so that error dashboards and alerts work
// correctly.
// Covers: BE-D01 (5 error/timeout counters), BE-D03 (error path metrics).
#[test]
fn error_counter_increments() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);
    metrics
        .chat_error_count
        .add(1, &[KeyValue::new("error_type", "llm_stream")]);
    metrics.llm_error_count.add(1, &[]);
    metrics.rerank_error_count.add(1, &[]);
    metrics.rewrite_timeout_count.add(1, &[]);
    metrics
        .rewrite_fallback_count
        .add(1, &[KeyValue::new("fallback_reason", "timeout")]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();
    assert!(
        has_counter_with_min_sum(&finished, "rag.chat.error.count", 1),
        "expected rag.chat.error.count with sum >= 1"
    );
    assert!(
        has_counter_with_min_sum(&finished, "rag.llm.error.count", 1),
        "expected rag.llm.error.count with sum >= 1"
    );
    assert!(
        has_counter_with_min_sum(&finished, "rag.rerank.error.count", 1),
        "expected rag.rerank.error.count with sum >= 1"
    );
    assert!(
        has_counter_with_min_sum(&finished, "rag.rewrite.timeout.count", 1),
        "expected rag.rewrite.timeout.count with sum >= 1"
    );
    assert!(
        has_counter_with_min_sum(&finished, "rag.rewrite.fallback.count", 1),
        "expected rag.rewrite.fallback.count with sum >= 1"
    );
}

// User Story: As a platform engineer, I need every instrument's metric name to
// match the design specification exactly, so that dashboards and alerts built
// on expected metric names do not silently break.
// Covers: BE-D01 (all 16 instrument names verified against design).
#[test]
fn all_instrument_names_match_design() {
    let (provider, meter, exporter) = build_test_provider();

    let metrics = RwikiMetrics::with_meter(meter);

    // Exercise every instrument once
    metrics.chat_request_count.add(1, &[]);
    metrics.chat_duration.record(1.0, &[]);
    metrics.chat_error_count.add(1, &[]);
    metrics.rewrite_duration.record(1.0, &[]);
    metrics.rewrite_timeout_count.add(1, &[]);
    metrics.rewrite_fallback_count.add(1, &[]);
    metrics.retrieval_duration.record(1.0, &[]);
    metrics.retrieval_results_count.record(1.0, &[]);
    metrics.retrieval_empty_count.add(1, &[]);
    metrics.rerank_duration.record(1.0, &[]);
    metrics.rerank_error_count.add(1, &[]);
    metrics.llm_duration.record(1.0, &[]);
    metrics.llm_first_token_duration.record(1.0, &[]);
    metrics.llm_error_count.add(1, &[]);
    metrics.llm_output_chars.record(1.0, &[]);
    metrics.llm_context_chunks.record(1.0, &[]);

    provider.force_flush().unwrap();

    let finished = exporter.get_finished_metrics().unwrap();
    let names = collect_metric_names(&finished);

    let expected_names: HashSet<String> = [
        "rag.chat.request.count",
        "rag.chat.duration",
        "rag.chat.error.count",
        "rag.rewrite.duration",
        "rag.rewrite.timeout.count",
        "rag.rewrite.fallback.count",
        "rag.retrieval.duration",
        "rag.retrieval.results.count",
        "rag.retrieval.empty.count",
        "rag.rerank.duration",
        "rag.rerank.error.count",
        "rag.llm.duration",
        "rag.llm.first_token.duration",
        "rag.llm.error.count",
        "rag.llm.output.chars",
        "rag.llm.context.chunks",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let missing: Vec<&str> = expected_names
        .iter()
        .filter(|name| !names.contains(*name))
        .map(|s| s.as_str())
        .collect();

    assert!(
        missing.is_empty(),
        "missing expected metric names: {:?}\nactual names: {:?}",
        missing,
        names
    );
}
