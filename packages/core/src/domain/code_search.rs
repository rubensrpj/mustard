//! Host-independent search requests. Intent never changes a literal pattern.
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        Ok(())
    }
}
