<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Recipes: mustard-cli

## Recipe: Add a new subcommand

**Files to create/edit:**

1. `src/commands/<name>.rs` — Options struct + entry function
2. `src/commands/mod.rs` — add `pub mod <name>;`
3. `src/cli.rs` — add variant to `Commands` enum + dispatch arm

**Skeleton for `src/commands/<name>.rs`:**

```rust
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Default, Clone)]
pub struct <Name>Options {
    pub force: bool,
}

pub fn <name>(project_path: &Path, options: &<Name>Options) -> Result<()> {
    let project_path = project_path.canonicalize()?;
    // …
    Ok(())
}
```

**Skeleton for `src/cli.rs` Commands variant:**

```rust
<Name> {
    #[arg(short, long)]
    force: bool,
},
```

**Dispatch arm in `dispatch()`:**

```rust
Commands::<Name> { force } => <name>::<name>(&cwd, &<Name>Options { force }),
```

Ref: `src/commands/config.rs` (simplest existing command)

---

## Recipe: Read + patch a JSON config file

Use `fs_ops::read_json_object` + `fs_ops::merge_json`. Never use `fs::write(serde_json::to_string(…))` directly — that drops existing keys.

```rust
use crate::fs_ops::merge_json;
use serde_json::json;

merge_json(&config_path, &[
    ("my_key", json!("my_value")),
])?;
```

Ref: `src/commands/init.rs:327-336`, `src/fs_ops.rs:87-105`

---

## Recipe: Shell out to an external tool (fail-open)

Follow the `git()` helper pattern — return `Option<String>`, never `Result`.

```rust
fn probe_tool(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("tool").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

Ref: `src/commands/git_flow.rs:84-90`

---

## Recipe: Write a unit test with a fake templates tree

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fake_templates(root: &Path) -> std::path::PathBuf {
        let t = root.join("templates");
        fs::create_dir_all(t.join("commands")).unwrap();
        fs::write(t.join("CLAUDE.md"), "# rules").unwrap();
        t
    }

    #[test]
    fn my_command_creates_expected_output() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // drive init_with_templates / update_with_templates directly
    }
}
```

Ref: `src/commands/init.rs:547-639`, `src/commands/update.rs:196-307`
