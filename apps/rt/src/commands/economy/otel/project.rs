//! OTLP/JSON → `claude_code_otel` projection.
//!
//! A faithful port of the `projectMetrics` / `projectLogs` walkers in
//! `otel-collector.js`. Kept separate from the HTTP server so the bucketing,
//! attribute flattening and datapoint-value extraction are unit-testable
//! without binding a socket.

use super::MetricRow;
use serde_json::Value;

/// Floor `time_unix_nano` (a protobuf-JSON number-or-string) to the start of
/// its containing minute, in ms epoch. Falls back to `now_ms` floored when the
/// value is missing or non-finite — matching the JS `bucketMs`.
#[must_use]
pub fn bucket_ms(time_unix_nano: Option<&Value>, now_ms: i64) -> i64 {
    let nanos = time_unix_nano.and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        // protobuf JSON encodes 64-bit ints as strings.
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    });
    match nanos {
        Some(n) if n.is_finite() => {
            let ms = (n / 1e6) as i64;
            (ms / 60_000) * 60_000
        }
        _ => (now_ms / 60_000) * 60_000,
    }
}

/// Extract a single string value from an OTLP attribute KV (the `anyValue`
/// shape) — `stringValue` / `intValue` / `doubleValue` / `boolValue`.
fn attr_value(value: &Value) -> Option<String> {
    if let Some(s) = value.get("stringValue").and_then(Value::as_str) {
        return Some(s.to_string());
    }
    for key in ["intValue", "doubleValue", "boolValue"] {
        if let Some(v) = value.get(key) {
            return Some(match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

/// Flatten an OTLP `attributes[]` array into a `{key: stringValue}` map.
#[must_use]
pub fn flatten_attrs(attrs: Option<&Value>) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Some(Value::Array(list)) = attrs else {
        return out;
    };
    for kv in list {
        let (Some(key), Some(value)) = (kv.get("key").and_then(Value::as_str), kv.get("value"))
        else {
            continue;
        };
        if let Some(val) = attr_value(value) {
            out.insert(key.to_string(), Value::String(val));
        }
    }
    out
}

/// Extract the numeric value from an OTLP datapoint (`asDouble` or `asInt`).
#[must_use]
pub fn point_value(dp: &Value) -> f64 {
    if let Some(d) = dp.get("asDouble").and_then(Value::as_f64) {
        return d;
    }
    match dp.get("asInt") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Project one metric's datapoints into [`MetricRow`]s. Returns an empty vec
/// for a metric with no name or no `sum`/`gauge` datapoints.
fn project_metric(metric: &Value, now_ms: i64) -> Vec<MetricRow> {
    let Some(name) = metric.get("name").and_then(Value::as_str) else {
        return Vec::new();
    };
    let points = metric
        .get("sum")
        .and_then(|s| s.get("dataPoints"))
        .or_else(|| metric.get("gauge").and_then(|g| g.get("dataPoints")))
        .and_then(Value::as_array);
    let Some(points) = points else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for dp in points {
        let mut attrs = flatten_attrs(dp.get("attributes"));
        let bucket = bucket_ms(dp.get("timeUnixNano"), now_ms);
        let session_id = attrs.get("session.id").and_then(Value::as_str).map(str::to_string);
        let model = attrs.get("model").and_then(Value::as_str).map(str::to_string);
        let token_type = attrs.get("type").and_then(Value::as_str).map(str::to_string);
        let sum = point_value(dp);
        // Drop the projected keys so `attrs` carries only the remainder.
        attrs.remove("session.id");
        attrs.remove("model");
        attrs.remove("type");
        rows.push(MetricRow {
            ts_bucket: bucket,
            metric: name.to_string(),
            session_id,
            model,
            token_type,
            sum,
            attrs: Value::Object(attrs).to_string(),
        });
    }
    rows
}

/// Walk an OTLP metrics body (`resourceMetrics[].scopeMetrics[].metrics[]`)
/// into a flat list of [`MetricRow`]s.
#[must_use]
pub fn project_metrics(body: &Value, now_ms: i64) -> Vec<MetricRow> {
    let mut out = Vec::new();
    let Some(resource_metrics) = body.get("resourceMetrics").and_then(Value::as_array) else {
        return out;
    };
    for rm in resource_metrics {
        let Some(scope_metrics) = rm.get("scopeMetrics").and_then(Value::as_array) else {
            continue;
        };
        for sm in scope_metrics {
            let Some(metrics) = sm.get("metrics").and_then(Value::as_array) else {
                continue;
            };
            for m in metrics {
                out.extend(project_metric(m, now_ms));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bucket_floors_to_minute() {
        // 90s in nanos → minute 1 (60_000 ms).
        let nanos = json!("90000000000");
        assert_eq!(bucket_ms(Some(&nanos), 0), 60_000);
        // A numeric encoding works too.
        let num = json!(150_000_000_000_i64);
        assert_eq!(bucket_ms(Some(&num), 0), 120_000);
    }

    #[test]
    fn bucket_falls_back_to_now_when_missing() {
        assert_eq!(bucket_ms(None, 125_000), 120_000);
    }

    #[test]
    fn flatten_handles_all_value_shapes() {
        let attrs = json!([
            { "key": "session.id", "value": { "stringValue": "abc" } },
            { "key": "n", "value": { "intValue": "42" } },
            { "key": "d", "value": { "doubleValue": 1.5 } },
            { "key": "b", "value": { "boolValue": true } },
            { "key": "skip", "value": {} }
        ]);
        let flat = flatten_attrs(Some(&attrs));
        assert_eq!(flat.get("session.id").unwrap(), "abc");
        assert_eq!(flat.get("n").unwrap(), "42");
        assert_eq!(flat.get("d").unwrap(), "1.5");
        assert_eq!(flat.get("b").unwrap(), "true");
        assert!(!flat.contains_key("skip"));
    }

    #[test]
    fn point_value_reads_double_and_int() {
        assert!((point_value(&json!({ "asDouble": 3.5 })) - 3.5).abs() < f64::EPSILON);
        assert!((point_value(&json!({ "asInt": "7" })) - 7.0).abs() < f64::EPSILON);
        assert!(point_value(&json!({})).abs() < f64::EPSILON);
    }

    #[test]
    fn project_metrics_extracts_projected_keys() {
        let body = json!({
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "claude_code.token.usage",
                        "sum": { "dataPoints": [{
                            "timeUnixNano": "90000000000",
                            "asInt": "100",
                            "attributes": [
                                { "key": "session.id", "value": { "stringValue": "s1" } },
                                { "key": "model", "value": { "stringValue": "opus" } },
                                { "key": "type", "value": { "stringValue": "input" } },
                                { "key": "extra", "value": { "stringValue": "kept" } }
                            ]
                        }]}
                    }]
                }]
            }]
        });
        let rows = project_metrics(&body, 0);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.ts_bucket, 60_000);
        assert_eq!(r.metric, "claude_code.token.usage");
        assert_eq!(r.session_id.as_deref(), Some("s1"));
        assert_eq!(r.model.as_deref(), Some("opus"));
        assert_eq!(r.token_type.as_deref(), Some("input"));
        assert!((r.sum - 100.0).abs() < f64::EPSILON);
        // Projected keys dropped; the remainder is kept.
        assert_eq!(r.attrs, r#"{"extra":"kept"}"#);
    }

    #[test]
    fn project_metrics_reads_gauge_datapoints() {
        let body = json!({
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "claude_code.cost.usage",
                        "gauge": { "dataPoints": [{ "asDouble": 0.25 }] }
                    }]
                }]
            }]
        });
        let rows = project_metrics(&body, 600_000);
        assert_eq!(rows.len(), 1);
        assert!((rows[0].sum - 0.25).abs() < f64::EPSILON);
        assert_eq!(rows[0].ts_bucket, 600_000);
    }

    #[test]
    fn project_metrics_ignores_malformed_input() {
        assert!(project_metrics(&json!({}), 0).is_empty());
        assert!(project_metrics(&json!({ "resourceMetrics": "nope" }), 0).is_empty());
        // A metric with no datapoints contributes nothing.
        let body = json!({ "resourceMetrics": [{ "scopeMetrics": [{
            "metrics": [{ "name": "x" }] }] }] });
        assert!(project_metrics(&body, 0).is_empty());
    }

}
