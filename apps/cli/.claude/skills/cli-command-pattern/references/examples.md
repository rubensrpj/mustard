<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Examples: cli-command-pattern

## InitOptions struct + split entry points (init.rs)

```rust
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    pub force: bool,
    pub yes: bool,
    pub cursor: bool,
    pub dry_run: bool,
}

pub fn init(project_path: &Path, options: &InitOptions) -> Result<()> {
    let templates_dir = resolve_templates_dir()?;
    init_with_templates(project_path, &templates_dir, options)
}

pub fn init_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &InitOptions,
) -> Result<()> {
    // …pure logic, no env queries…
    Ok(())
}
```

Ref: `apps/cli/src/commands/init.rs:37-80`

---

## Dispatch table in cli.rs

```rust
fn dispatch(cli: Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    match cli.command {
        Commands::Init { force, yes, cursor, dry_run } =>
            init::init(&cwd, &InitOptions { force, yes, cursor, dry_run }),
        Commands::Update { force } =>
            update::update(&cwd, &UpdateOptions { force }),
        Commands::Config { yes } =>
            config::config(&cwd, &ConfigOptions { yes }),
        // …
    }
}
```

Ref: `apps/cli/src/cli.rs:99-126`

---

## Minimal wrapper (config.rs)

```rust
#[derive(Debug, Default, Clone)]
pub struct ConfigOptions { pub yes: bool }

pub fn config(project_path: &Path, options: &ConfigOptions) -> Result<()> {
    println!("\nMustard - Git Flow Configuration\n");
    git_flow::generate_mustard_json(project_path, !options.yes)
}
```

Ref: `apps/cli/src/commands/config.rs`
