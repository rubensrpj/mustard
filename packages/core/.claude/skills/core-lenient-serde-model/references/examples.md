<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Examples — core-lenient-serde-model

## HookInput — typed fields + raw catch-all (packages/core/src/model/contract.rs)

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub tool_input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Absorbs any field the harness adds in the future.
    #[serde(flatten)]
    pub raw: Value,
}
```

## PipelineState — camelCase rename + raw catch-all (packages/core/src/model/pipeline.rs)

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineState {
    #[serde(default, rename = "specName", skip_serializing_if = "Option::is_none")]
    pub spec_name: Option<String>,
    #[serde(default, rename = "phaseName", skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    #[serde(default, rename = "isWavePlan")]
    pub is_wave_plan: bool,
    /// Unknown fields land here, not in an error.
    #[serde(flatten)]
    pub raw: Value,
}
```

## Verdict — tagged enum (packages/core/src/model/contract.rs)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verdict {
    Allow,
    Deny { reason: String },
    Warn { message: String },
    Rewrite { tool_input: Value },
    Inject { context: String },
}
```

## Round-trip test asserting unknown fields land in raw

```rust
#[test]
fn hook_input_is_lenient_about_unknown_fields() {
    let raw = r#"{"tool_name":"Bash","hook_event_name":"PreToolUse",
        "tool_input":{"command":"ls"},"future_field":42}"#;
    let input: HookInput = serde_json::from_str(raw).expect("lenient parse");
    assert_eq!(input.tool_name.as_deref(), Some("Bash"));
    assert_eq!(input.raw["future_field"], serde_json::json!(42));
}
```
