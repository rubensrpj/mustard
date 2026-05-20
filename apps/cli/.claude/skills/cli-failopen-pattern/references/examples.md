<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Examples: cli-failopen-pattern

## read_json_object — never errors (fs_ops.rs)

```rust
pub fn read_json_object(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}
```

Ref: `apps/cli/src/fs_ops.rs:111-120`

---

## merge_json — surgical, preserves unrelated keys (fs_ops.rs)

```rust
pub fn merge_json(path: &Path, updates: &[(&str, Value)]) -> Result<()> {
    let mut object = read_json_object(path);
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    let mut serialized = serde_json::to_string_pretty(&Value::Object(object))?;
    serialized.push('\n');
    fs::write(path, serialized)?;
    Ok(())
}
```

Ref: `apps/cli/src/fs_ops.rs:87-105`

---

## Fail-open git probe (git_flow.rs)

```rust
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

Ref: `apps/cli/src/commands/git_flow.rs:84-90`

---

## Opt-in guard for global settings (init.rs)

```rust
pub(crate) fn ensure_global_permissions() -> Result<()> {
    if !global_permissions_opt_in() {
        println!("  Global settings: skipped (set MUSTARD_GLOBAL_PERMISSIONS=1 …)");
        return Ok(());
    }
    // … write ~/.claude/settings.json
}

fn global_permissions_opt_in() -> bool {
    std::env::var("MUSTARD_GLOBAL_PERMISSIONS")
        .map(|v| { let v = v.trim().to_ascii_lowercase(); v == "1" || v == "true" })
        .unwrap_or(false)
}
```

Ref: `apps/cli/src/commands/init.rs:350-433`

---

## Warn-only ensure_rtk (init.rs)

```rust
ensure_global_permissions().unwrap_or_else(|err| {
    eprintln!("[mustard] warning: could not update global permissions: {err}");
});
ensure_rtk();  // never returns an error — all failures are printed and ignored
```

Ref: `apps/cli/src/commands/init.rs:130-133`
