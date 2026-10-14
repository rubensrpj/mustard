//! Best-effort native model capabilities. Never launch a generation CLI or
//! use API credentials to discover a model. Unknown capabilities stay unknown.
use serde_json::{Value, json};

fn version(id: &str) -> Vec<u32> {
    id.strip_prefix("gpt-")
        .unwrap_or_default()
        .split('-')
        .next()
        .unwrap_or_default()
        .split('.')
        .filter_map(|v| v.parse().ok())
        .collect()
}
fn catalogue(value: &Value, now: chrono::DateTime<chrono::Utc>) -> Option<&str> {
    let fetched = chrono::DateTime::parse_from_rfc3339(value["fetched_at"].as_str()?)
        .ok()?
        .with_timezone(&chrono::Utc);
    let age = now.signed_duration_since(fetched).num_seconds();
    if !(0..=86_400).contains(&age) {
        return None;
    }
    value["models"]
        .as_array()?
        .iter()
        .filter(|m| {
            m["slug"].as_str().is_some_and(|id| {
                id.starts_with("gpt-") && id.split('-').any(|part| part == "luna")
            }) && m["visibility"] == "list"
                && m["supported_reasoning_levels"]
                    .as_array()
                    .is_some_and(|levels| levels.iter().any(|level| level["effort"] == "medium"))
        })
        .max_by(|a, b| {
            version(a["slug"].as_str().unwrap_or_default())
                .cmp(&version(b["slug"].as_str().unwrap_or_default()))
                .then_with(|| {
                    b["priority"]
                        .as_i64()
                        .unwrap_or(i64::MAX)
                        .cmp(&a["priority"].as_i64().unwrap_or(i64::MAX))
                })
        })
        .and_then(|m| m["slug"].as_str())
}

pub fn resolve(host: &str, preference: Option<&str>) -> Value {
    if host == "claude" {
        return json!({"model":"haiku","ready":true,"effort":"medium","latest_version_observed":null,
            "source":"documented-native-family-alias","preference":preference,
            "limits":"The native host resolves the current Haiku alias; actual model/usage still require native telemetry."});
    }
    if host != "codex" {
        return json!({"ready":false,"reason":"unknown-host"});
    }
    let home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| std::path::PathBuf::from(home).join(".codex"))
        });
    let cache = home.and_then(|home| {
        let file = home.join("models_cache.json");
        if std::fs::metadata(&file).ok()?.len() > 1_000_000 {
            return None;
        }
        serde_json::from_slice::<Value>(&std::fs::read(file).ok()?).ok()
    });
    let current = cache
        .as_ref()
        .and_then(|value| catalogue(value, chrono::Utc::now()));
    json!({"model":current,"ready":current.is_some(),"effort":"medium","latest_version_observed":current.map(|_|true),
        "source":"native-models-cache-best-effort","preference":preference,
        "limits":"Only a fresh native catalogue with listed Luna and medium effort authorizes delegation. Unknown/stale formats use native retrieval; no family substitution or separate process."})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newest_listed_family_with_medium_wins_and_stale_or_incompatible_catalogues_abstain() {
        let now = chrono::Utc::now();
        let row = |id: &str, effort: &str, visibility: &str| json!({"slug":id,"visibility":visibility,"supported_reasoning_levels":[{"effort":effort}]});
        let mut cache = json!({"fetched_at":now.to_rfc3339(),"models":[row("gpt-6-luna","medium","list"),row("gpt-7.1-luna","medium","list"),
            row("gpt-9-luna","high","list"),row("gpt-10-luna","medium","hidden"),row("gpt-11-sol","medium","list")]});
        assert_eq!(catalogue(&cache, now), Some("gpt-7.1-luna"));
        cache["fetched_at"] = json!((now - chrono::Duration::days(2)).to_rfc3339());
        assert_eq!(catalogue(&cache, now), None);
        assert_eq!(catalogue(&json!({}), now), None);
    }
}
