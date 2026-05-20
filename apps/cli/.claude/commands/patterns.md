<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Patterns: mustard-cli

## Pattern 1 — Options struct + entry function

Every subcommand exposes a `<Name>Options` struct (plain `Debug + Default + Clone`) and a `pub fn <name>(project_path: &Path, opts: &<Name>Options) -> Result<()>` entry point. The `cli.rs` dispatch table constructs the struct from `clap` args and calls the function. The Tauri backend and tests call the same function directly, passing their own path.

```rust
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    pub force: bool,
    pub yes: bool,
    pub dry_run: bool,
}
pub fn init(project_path: &Path, options: &InitOptions) -> Result<()> { … }
```

Ref: `src/commands/init.rs:37-68`, `src/commands/update.rs:43-63`, `src/commands/review.rs:36-46`

---

## Pattern 2 — Template resolution split (`init` / `init_with_templates`)

Each command that needs the templates directory exposes two entry points:
- `init(path, opts)` — resolves templates via `resolve_templates_dir()`, then delegates.
- `init_with_templates(path, templates, opts)` — pure logic, no env queries. Tests and the Tauri backend use this variant.

Resolution order for `resolve_templates_dir()`:
1. `MUSTARD_TEMPLATES_DIR` env var
2. `<exe-dir>/templates` or `<exe-dir>/../templates`
3. `CARGO_MANIFEST_DIR/templates` (dev / `cargo run`)

Ref: `src/commands/init.rs:65-186`

---

## Pattern 3 — Fail-open JSON merge (`merge_json` / `read_json_object`)

`fs_ops::read_json_object` never errors: absent file, I/O failure, malformed JSON, and non-object values all collapse to an empty `Map`. `merge_json` reads via that helper, overlays a `&[(&str, Value)]` patch, and writes back pretty-printed with a trailing newline. Non-destructive: unmodified keys survive verbatim.

```rust
merge_json(&path, &[
    ("runtime", serde_json::to_value(runtime)?),
    ("version", json!(crate::VERSION)),
])
```

Ref: `src/fs_ops.rs:87-120`

---

## Pattern 4 — Fail-open shell-out (`git` / `rtk_on_path` / `has_github_remote`)

Helper functions that shell out to external tools follow a consistent fail-open shape: `Command::new(tool).args([…]).output().ok()? → None` on any failure. The caller treats `None` as a safe default (empty string, `false`, etc.) and never propagates the error upward.

```rust
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

Ref: `src/commands/git_flow.rs:84-90`, `src/commands/init.rs:297-310`, `src/commands/init.rs:474-480`

---

## Pattern 5 — Non-interactive TTY guard

All interactive prompts check `std::io::stdin().is_terminal()` before calling `dialoguer`. When stdin is not a TTY (CI, tests, Tauri sidecar) the safe default is chosen silently — merge over overwrite, proceed over cancel.

Ref: `src/commands/init.rs:200-204`, `src/commands/update.rs:92-103`, `src/commands/auto_update.rs:59-69`

---

## Pattern 6 — `CORE_FOLDERS` ownership split

`update` owns a fixed set of Mustard-managed folders (`commands/mustard`, `hooks`, `skills`, `scripts`, `refs`) plus `settings.json`. Everything else under `.claude/` is user territory and is never touched. The split is encoded in the `CORE_FOLDERS` constant.

Ref: `src/commands/update.rs:53`, `src/commands/update.rs:109-126`
