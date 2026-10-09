//! Host-independent search requests. Intent never changes a literal pattern.
use super::knowledge::investigation::Purpose;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable wire contract shared by host adapters; the host supplies this version.
pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub schema_version: u32,
    pub request: Request,
}

impl Invocation {
    pub fn new(request: Request) -> Self {
        Self {
            schema_version: CONTRACT_VERSION,
            request,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(alias = "tool_name")]
    pub tool: String,
    #[serde(default, alias = "tool_input")]
    pub input: Value,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub purpose: super::knowledge::investigation::Purpose,
    /// Explicitly permit optional classification of unresolved responsibility.
    /// Exact occurrences are never removed by this decision.
    #[serde(default)]
    pub choose: bool,
}

impl Request {
    /// New adapters use the versioned envelope and explicit investigation fields.
    /// Retain the original CLI shape for existing callers and native fallbacks.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("search-contract-invalid-json: {e}"))?;
        if value.get("schema_version").is_some() || value.get("request").is_some() {
            let fields = value["request"]
                .as_object()
                .ok_or("search-contract-request-object-required")?;
            for key in ["tool", "input", "intent", "purpose"] {
                if !fields.contains_key(key) {
                    return Err(format!("search-contract-missing-{key}"));
                }
            }
            let invocation: Invocation = serde_json::from_value(value)
                .map_err(|e| format!("search-contract-invalid-request: {e}"))?;
            if invocation.schema_version != CONTRACT_VERSION {
                return Err("search-unsupported-contract-version".into());
            }
            Ok(invocation.request)
        } else {
            serde_json::from_value(value)
                .map_err(|e| format!("search-contract-invalid-request: {e}"))
        }
    }

    /// Optional structured annotation on an original host command. Plain prose
    /// remains locate: it is never interpreted as an implicit investigation.
    pub fn describe(&mut self, description: &str) {
        self.purpose = Purpose::Locate;
        self.intent = description.chars().take(1000).collect();
        if let Some((purpose, intent)) = description
            .strip_prefix("mustard:")
            .and_then(|s| s.split_once(':'))
            && let Some(purpose) = Purpose::parse(purpose.trim())
            && (purpose == Purpose::Locate || !intent.trim().is_empty())
        {
            self.purpose = purpose;
            self.intent = intent.trim().chars().take(1000).collect();
        }
    }

    pub fn native(argv: &[String]) -> Result<Self, String> {
        let (tool, args) = argv.split_first().ok_or("search-tool-required")?;
        Ok(Self {
            tool: tool.clone(),
            input: serde_json::json!({"args":args}),
            intent: String::new(),
            purpose: Default::default(),
            choose: false,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.intent.len() > 4000 || self.input.to_string().len() > 64 * 1024 {
            return Err("search-request-too-large".into());
        }
        if !matches!(
            self.tool.as_str(),
            "rg" | "grep" | "git" | "Grep" | "Glob" | "Read"
        ) {
            return Err("search-tool-unsupported; use the original host tool".into());
        }
        if !self.input.is_object() {
            return Err("search-input-object-required".into());
        }
        if (self.purpose != Purpose::Locate || self.choose) && self.intent.trim().is_empty() {
            return Err("search-intent-required; provide the specific question this investigation must answer".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versioned_call_preserves_original_arguments_and_requires_explicit_fields() {
        let request =
            Request::native(&["rg".into(), "-n".into(), "literal.*".into(), "src".into()]).unwrap();
        let envelope = serde_json::to_value(Invocation::new(request.clone())).unwrap();
        let decoded = Request::from_json(&envelope.to_string()).unwrap();
        assert_eq!(decoded.input, request.input);
        assert_eq!(decoded.purpose, Purpose::Locate);
        assert!(!decoded.choose);
        for key in ["tool", "input", "intent", "purpose"] {
            let mut missing = envelope.clone();
            missing["request"].as_object_mut().unwrap().remove(key);
            assert!(Request::from_json(&missing.to_string()).is_err(), "{key}");
        }
        let mut future = envelope.clone();
        future["schema_version"] = json!(2);
        assert!(
            Request::from_json(&future.to_string())
                .unwrap_err()
                .contains("version")
        );
        let mut unknown = envelope;
        unknown["host_private_field"] = json!(true);
        assert!(Request::from_json(&unknown.to_string()).is_err());
    }

    #[test]
    fn investigations_require_a_question_and_literal_legacy_calls_remain_valid() {
        let mut request =
            Request::from_json(r#"{"tool":"rg","input":{"args":["needle","."]}}"#).unwrap();
        request.validate().unwrap();
        for purpose in [
            Purpose::Understand,
            Purpose::Spec,
            Purpose::Implement,
            Purpose::Validate,
        ] {
            request.purpose = purpose;
            request.intent = "  ".into();
            assert!(request.validate().unwrap_err().contains("intent-required"));
            request.intent = "Verify which operation persists the current copy".into();
            request.validate().unwrap();
        }
        request.purpose = Purpose::Locate;
        request.intent.clear();
        request.choose = true;
        assert!(request.validate().is_err());
    }

    #[test]
    fn explicit_description_sets_purpose_without_rewriting_native_input_or_enabling_choice() {
        let mut request = Request::native(&["rg".into(), "current_copy".into()]).unwrap();
        let input = request.input.clone();
        request.describe("mustard:understand: Verify the data source of the current copy");
        assert_eq!(request.purpose, Purpose::Understand);
        assert_eq!(request.intent, "Verify the data source of the current copy");
        assert_eq!(request.input, input);
        assert!(!request.choose);
        request.describe("Find persistence");
        assert_eq!(request.purpose, Purpose::Locate);
        for description in [
            "Understand persistence",
            "mustard:unknown: question",
            "mustard:spec: ",
        ] {
            let mut native = Request::native(&["rg".into(), "needle".into()]).unwrap();
            native.describe(description);
            assert_eq!(native.purpose, Purpose::Locate);
            assert_eq!(native.intent, description);
        }
    }
}
