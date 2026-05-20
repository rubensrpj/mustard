<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Examples — core-fail-open-error

## Error enum (packages/core/src/error.rs)

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("check failed: {0}")]
    CheckFailed(String),
    #[error("sqlite error: {0}")]
    Sqlite(String),
}

impl Error {
    pub fn config(msg: impl Into<String>) -> Self { Self::Config(msg.into()) }
    pub fn check_failed(msg: impl Into<String>) -> Self { Self::CheckFailed(msg.into()) }
}
```

## fail_open helpers (packages/core/src/error.rs)

```rust
pub fn fail_open<T>(result: Result<T>, fallback: T) -> T {
    result.unwrap_or(fallback)
}
pub fn fail_open_with<T>(result: Result<T>, fallback: impl FnOnce() -> T) -> T {
    result.unwrap_or_else(|_| fallback())
}
```

## NotFound vs Io distinction (packages/core/src/io/fs.rs)

```rust
pub fn read_to_string(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound =>
            Err(Error::NotFound(path.display().to_string())),
        Err(err) => Err(Error::from(err)),
    }
}
```

## Fail-silent metric emit (packages/core/src/metrics.rs)

```rust
pub fn emit_metric(cwd: &Path, line: &MetricLine) -> bool {
    emit_metric_inner(cwd, line).is_ok()
}
fn emit_metric_inner(cwd: &Path, line: &MetricLine) -> Result<()> {
    if line.event.trim().is_empty() {
        return Err(crate::error::Error::config("metric event name is empty"));
    }
    let path = metric_file_path(cwd, &line.event);
    let serialized = serde_json::to_string(&line.to_json())?;
    append_line(&path, &serialized)
}
```

## Config fail-open (packages/core/src/config.rs)

```rust
pub fn resolve<F>(mustard_json: Option<&str>, checks: &[&str], env_var: F) -> Self
where F: Fn(&str) -> Option<String> {
    // A parse failure falls back to an empty config — fail-open.
    let from_file = mustard_json
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| Self::from_json(&value).ok())
        .unwrap_or_default();
    // ... env layer applied on top
}
```

## read_optional — NotFound → Ok(None) (packages/core/src/io/pipeline_repo.rs)

```rust
pub fn read_optional(repo: &impl PipelineRepo, spec_name: &str)
    -> Result<Option<PipelineState>>
{
    match repo.read(spec_name) {
        Ok(state) => Ok(Some(state)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(err) => Err(err),
    }
}
```
