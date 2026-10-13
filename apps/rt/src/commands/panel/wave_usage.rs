//! Local live usage cache. Never rewrites authoritative sends or executes a
//! wave. An old measurement cannot migrate to a different dispatch attempt.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn file(root: &Path, name: &str) -> PathBuf {
    let mut hash = mustard_core::io::sha256::Sha256::new();
    hash.update(name.as_bytes());
    root.join(".claude/mustard/usage/specs").join(format!("{}.json", hash.hex_digest()))
}

pub(super) fn read(root: &Path, name: &str) -> Value {
    mustard_core::io::fs::lock::read_shared(&file(root, name))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn refresh(start: &Path, selected: Option<&str>, session: Option<&str>, config: Option<&Path>) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let name = selected
        .map(str::to_string)
        .or_else(|| crate::shared::context::checkout::current_spec(&start.to_string_lossy()));
    let result = (|| {
        let name = name.ok_or("no-current-spec")?;
        let path = mustard_core::io::spec_events::spec_file(&root, &name).map_err(|_| "invalid-spec")?;
        let log = mustard_core::io::spec_events::read(&path)
            .map_err(|_| "usage-log-unreadable")?
            .ok_or("no-spec-log")?;
        let value = crate::commands::flow::round::usage_snapshot(&root, &log, session, config);
        mustard_core::io::fs::write_atomic(file(&root, &name), value.to_string().as_bytes()).map_err(|_| "usage-cache-write-failed")?;
        Ok::<_, &str>(())
    })();
    result.map_or_else(|reason| json!({"ok":false,"reason":reason}), |()| json!({"ok":true}))
}

pub(super) fn current_at(cache:&Value,wave:u64,dispatch:u64)->Option<&Value> {
    cache["waves"]
        .as_array()?
        .iter()
        .find(|row| row["wave"] == wave && row["dispatch"] == dispatch)
        .map(|row| &row["usage"])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_measurements_are_bound_to_the_dispatch_and_not_its_numeric_wave_alone() {
        let log = mustard_core::domain::spec_events::parse_log("{\"v\":1,\"id\":1,\"type\":\"send\",\"wave\":1}\n");
        let cache = json!({"waves":[{"wave":1,"dispatch":1,"usage":{"tokens":123}}]});
        assert_eq!(current_at(&cache, 1, log.last_dispatch_by_wave()[&1]).unwrap()["tokens"], 123);
        let next = mustard_core::domain::spec_events::parse_log(
            "{\"v\":1,\"id\":1,\"type\":\"send\",\"wave\":1}\n{\"v\":1,\"id\":2,\"type\":\"send\",\"wave\":1,\"resends\":1}\n",
        );
        assert!(current_at(&cache, 1, next.last_dispatch_by_wave()[&1]).is_none());
    }
}
