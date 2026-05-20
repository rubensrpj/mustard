<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Examples — core-trait-backed-io

## EventSink trait (packages/core/src/io/event_store.rs)

```rust
pub trait EventSink {
    fn append(&self, event: &HarnessEvent) -> Result<()>;
}
```

## PipelineRepo trait (packages/core/src/io/pipeline_repo.rs)

```rust
pub trait PipelineRepo {
    fn read(&self, spec_name: &str) -> Result<PipelineState>;
    fn write(&self, spec_name: &str, state: &PipelineState) -> Result<()>;
}
```

## FsPipelineRepo constructor pattern

```rust
pub fn for_project(project_dir: impl AsRef<Path>) -> Self {
    let states_dir = project_dir
        .as_ref()
        .join(".claude")
        .join(PIPELINE_STATES_DIR);
    Self { states_dir }
}
```

## Atomic write (packages/core/src/io/fs.rs)

```rust
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp = temp_path_for(path);
    let write_result = (|| -> Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    fs::rename(&temp, path).map_err(Error::from)
}
```

## In-memory fake for EventSink (test in packages/core/src/io/event_store.rs)

```rust
struct FakeSink {
    collected: RefCell<Vec<String>>,
}
impl EventSink for FakeSink {
    fn append(&self, event: &HarnessEvent) -> Result<()> {
        self.collected.borrow_mut().push(event.event.clone());
        Ok(())
    }
}
```

## In-memory fake for PipelineRepo (test in packages/core/src/io/pipeline_repo.rs)

```rust
struct FakeRepo {
    store: Mutex<HashMap<String, PipelineState>>,
}
impl PipelineRepo for FakeRepo {
    fn read(&self, spec_name: &str) -> Result<PipelineState> {
        self.store
            .lock()
            .ok()
            .and_then(|m| m.get(spec_name).cloned())
            .ok_or_else(|| Error::NotFound(spec_name.to_string()))
    }
    fn write(&self, spec_name: &str, state: &PipelineState) -> Result<()> {
        if let Ok(mut m) = self.store.lock() {
            m.insert(spec_name.to_string(), state.clone());
        }
        Ok(())
    }
}
```

## Env trait and MapEnv (packages/core/src/env.rs)

```rust
pub trait Env {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}

pub struct MapEnv {
    inner: std::cell::RefCell<std::collections::HashMap<String, String>>,
}
impl MapEnv {
    pub fn new() -> Self { Self::default() }
    pub fn with(self, key: &str, value: &str) -> Self {
        self.inner.borrow_mut().insert(key.to_string(), value.to_string());
        self
    }
}
```
