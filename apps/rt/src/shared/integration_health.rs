//! One read-only health projection for diagnostics and the panel. Does not
//! call a provider, initialize a model, mutate config or infer hook trust.
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn snapshot(root: &Path) -> Value {
    let config = mustard_core::ProjectConfig::load(root);
    let switches = mustard_core::Switches::read(root);
    let version = mustard_core::harness_version();
    let receipt: Value = std::fs::read_to_string(root.join(".claude/mustard/installation.json"))
        .ok()
        .filter(|text| text.len() <= 1_048_576)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let key = mustard_core::io::jev_gate::find_key(
        root,
        &config,
        std::env::var(mustard_core::io::jev_gate::KEY_ENV).ok(),
    );
    let mut purposes = serde_json::Map::new();
    for purpose in ["search", "wave-planning", "context"] {
        purposes.insert(
            purpose.into(),
            json!(
                config.ai_fallback_enabled()
                    && key.is_ok()
                    && mustard_core::io::jev_gate::setting_allows(config.judgement_filter(purpose))
            ),
        );
    }
    let mut issues = Vec::new();
    if config.unreadable {
        issues.push("configuration-unreadable");
    }
    if config.version.as_deref() != Some(version.as_str()) {
        issues.push("project-version-drift");
    }
    if receipt["complete"] != true || receipt["version"] != version {
        issues.push("installation-unverified-for-running-version");
    }
    // The swap runs the rtk copy that ships with Mustard, never the PATH one,
    // and rtk's own hook left in the local settings races it.
    let rtk_available = crate::hooks::bash::rtk_swap::bundled().is_some();
    let rtk_matches=!switches.rtk_hook_left();
    if config.rtk() && (!rtk_available || !rtk_matches) {
        issues.push("rtk-incomplete");
    }
    if config.ai_fallback_enabled() && key.is_err() {
        issues.push("jev-credential-unavailable");
    }
    json!({"ok":issues.is_empty(),"issues":issues,"running_version":version,"project_version":config.version,
        "configuration_readable":!config.unreadable,"installation_receipt_current":receipt["complete"]==true && receipt["version"]==version,
        "rtk":{"enabled":config.rtk(),"executable_available":rtk_available,"own_hook_left":switches.rtk_hook,"configuration_matches":rtk_matches},
        "jev":{"credential_available":key.is_ok(),"credential_error":key.err().map(|e|e.reason()),"purposes":purposes,"balance_checked":false},
        "vectors":mustard_core::io::code_vectors::status(root),"hosts":super::host_observation::snapshot(root),
        "remedy":"Run mustard-rt run upsert for this project with the intended installed version; setup-codex installs Codex files. Review native hook trust in the host. Never replace an active wave's runtime mid-execution.",
        "network_requests":0,"model_calls":0})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_configured_but_unobserved_project_is_not_reported_as_operational() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"jev":{"key":"private-test-key"},"ai":{"fallback":true},"rtk":false}"#,
        )
        .unwrap();
        let report = snapshot(dir.path());
        assert_eq!(report["jev"]["credential_available"], true);
        assert_eq!(report["ok"], false);
        assert_eq!(report["vectors"]["status"], "index-not-populated");
        assert!(!report.to_string().contains("private-test-key"));
        assert!(!dir.path().join(".claude").exists());
    }
}
