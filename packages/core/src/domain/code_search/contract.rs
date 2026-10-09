//! One JSON contract distributed with the plugin and compiled into the binary.
//! Its content is host independent; transport context is not model input.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

pub fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| include_str!("../../../../../plugin/hooks/search-schema.js").strip_prefix("export default ").and_then(|s|s.strip_suffix(";\n")).and_then(|s|serde_json::from_str(s).ok()).unwrap_or(Value::Null))
}

pub fn validate_input(tool: &str, input: &Value) -> Result<(), String> {
    let branch = schema()["properties"]["request"]["oneOf"].as_array()
        .and_then(|options| options.iter().find(|s| s["properties"]["tool"]["const"] == tool))
        .ok_or("search-tool-unsupported")?;
    validate(&branch["properties"]["input"], input, "input")
}
fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let valid = match schema["type"].as_str() {
        Some("object") => value.is_object(), Some("string") => value.is_string(),
        Some("integer") => value.as_u64().is_some(), Some("boolean") => value.is_boolean(),
        Some("array") => value.is_array(), _ => false,
    };
    if !valid { return Err(format!("search-contract-invalid-{path}")); }
    if let Some(options) = schema["enum"].as_array() && !options.contains(value) {
        return Err(format!("search-contract-invalid-{path}"));
    }
    if let Some(min) = schema["minimum"].as_u64() && value.as_u64().is_none_or(|v| v < min) {
        return Err(format!("search-contract-invalid-{path}"));
    }
    if let Some(values) = value.as_array() {
        if schema["minItems"].as_u64().is_some_and(|n| values.len() < n as usize) {
            return Err(format!("search-contract-invalid-{path}"));
        }
        for item in values { validate(&schema["items"], item, path)?; }
    }
    if let Some(values) = value.as_object() {
        for field in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !values.contains_key(field) { return Err(format!("search-contract-missing-{field}")); }
        }
        for (field, item) in values {
            let spec = &schema["properties"][field];
            if spec.is_null() { return Err(format!("search-contract-unknown-{field}")); }
            validate(spec, item, field)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryContext {
    pub session: String,
    pub agent: String,
    /// Changes after reload, compact, clear/resume/branch or agent replacement.
    pub epoch: String,
    #[serde(default)]
    pub acknowledged: Vec<String>,
}
impl DeliveryContext {
    pub fn validate(&self) -> Result<(), String> {
        if [&self.session, &self.agent, &self.epoch].iter().any(|s| s.is_empty() || s.len() > 160
            || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))) {
            return Err("search-contract-invalid-delivery-context".into());
        }
        if self.acknowledged.len()>128 || self.acknowledged.iter().any(|s|s.len()!=64 || !s.bytes().all(|c|c.is_ascii_hexdigit())) {
            return Err("search-contract-invalid-delivery-acknowledgement".into());
        }
        Ok(())
    }
    pub fn from_json(text: &str) -> Result<Option<Self>, String> {
        let value: Value = serde_json::from_str(text).map_err(|_| "search-contract-invalid-json")?;
        let Some(context) = value.get("context") else { return Ok(None); };
        let context: Self = serde_json::from_value(context.clone()).map_err(|_| "search-contract-invalid-delivery-context")?;
        context.validate()?;
        Ok(Some(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn typed_operations_refuse_missing_wrong_and_cross_operation_arguments() {
        assert!(schema().is_object());
        assert!(validate_input("References", &json!({"file_path":"a.ts","line":1,"column":0})).is_ok());
        for input in [json!({"file_path":"a.ts","line":0,"column":0}),json!({"file_path":"a.ts","line":1,"column":"0"}),
            json!({"file_path":"a.ts","line":1}),json!({"file_path":"a.ts","line":1,"column":0,"query":"x"})] {
            assert!(validate_input("References", &input).is_err());
        }
        assert!(validate_input("rg", &json!({"args":[1]})).is_err());
        assert!(validate_input("Read", &json!({"file_path":"a","offset":0})).is_err());
    }
}
