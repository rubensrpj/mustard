use super::{Asset, publish};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::sha256::Sha256;
use serde_json::{Value, json};
use std::path::Path;

/// Prepare a complete immutable static snapshot. Only explicit CLI calls
/// reach this writer; local panels/statusline never create these files.
pub(crate) fn prepare_snapshot(
    root: &Path,
    parent: &Path,
    public: &Value,
    render: fn(&Value) -> String,
) -> Value {
    let version = snapshot_id(public, render);
    let directory = parent.join(&version);
    let prepared = (|| -> Result<(), String> {
        safe_directory(root, &directory)?;
        for file in [
            "prepare.lock",
            "manifest.json",
            "snapshot.json",
            "index.html",
        ] {
            if std::fs::symlink_metadata(directory.join(file))
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                return Err("publication resource is a symbolic link".into());
            }
        }
        let _lock =
            LockedFile::exclusive(&directory.join("prepare.lock")).map_err(|e| e.to_string())?;
        safe_directory(root, &directory)?;
        let stored = std::fs::read(directory.join("snapshot.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let reusable = stored.as_ref().is_some_and(|stored| {
            snapshot_id(stored, render) == version
                && std::fs::read_to_string(directory.join("index.html"))
                    .is_ok_and(|html| html == render(stored))
        });
        if reusable {
            return Ok(());
        }
        for (file, body) in [
            ("snapshot.json", public.to_string()),
            ("index.html", render(public)),
            (
                "manifest.json",
                json!({"snapshot_id":version,"published":false,"remote_url":null}).to_string(),
            ),
        ] {
            mustard_core::io::fs::write_atomic(directory.join(file), body.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    match prepared {
        Ok(()) => json!({"ok":true,"prepared":true,"published":false,"snapshot_id":version,
            "page":directory.join("index.html"),"database":directory.join("snapshot.json"),"manifest":directory.join("manifest.json"),
            "next":"Os arquivos são locais. Configure o destino para publicar pelo binário sob pedido explícito."}),
        Err(detail) => json!({"ok":false,"reason":"publication-export-failed","detail":detail}),
    }
}

fn snapshot_id(public: &Value, render: fn(&Value) -> String) -> String {
    let mut stable = public.clone();
    if let Some(object) = stable.as_object_mut() {
        object.remove("at");
    }
    let mut hash = Sha256::new();
    hash.update(stable.to_string().as_bytes());
    hash.update(render(&stable).as_bytes());
    hash.hex_digest()
}

/// Upload only the allowlisted database and native HTML, never internal
/// receipts or model-authored code. Failed upload preserves the local export.
pub(crate) fn upload_prepared(
    root: &Path,
    base: &Path,
    mut prepared: Value,
    scope: &str,
    render: fn(&Value) -> String,
) -> Value {
    if prepared["ok"] != true {
        return prepared;
    }
    let result = (|| -> Result<Value, String> {
        let database = Path::new(
            prepared["database"]
                .as_str()
                .ok_or("publication-no-database")?,
        );
        let directory = database.parent().ok_or("publication-no-directory")?;
        safe_directory(base, directory)?;
        if std::fs::symlink_metadata(database)
            .map_err(|_| "publication-no-database")?
            .file_type()
            .is_symlink()
        {
            return Err("publication-unsafe-database".into());
        }
        let public: Value = serde_json::from_slice(
            &std::fs::read(database).map_err(|_| "publication-no-database")?,
        )
        .map_err(|_| "publication-invalid-database")?;
        if json!(snapshot_id(&public, render)) != prepared["snapshot_id"] {
            return Err("publication-content-changed".into());
        }
        Ok(publish(
            root,
            directory,
            scope,
            &[
                Asset {
                    path: "/index.html",
                    content_type: "text/html; charset=utf-8",
                    bytes: render(&public).into_bytes(),
                },
                Asset {
                    path: "/snapshot.json",
                    content_type: "application/json",
                    bytes: public.to_string().into_bytes(),
                },
            ],
        ))
    })();
    let remote = result.unwrap_or_else(|reason| json!({"published":false,"reason":reason}));
    if let (Some(local), Some(remote)) = (prepared.as_object_mut(), remote.as_object()) {
        local.extend(remote.clone());
    }
    prepared["next"] = prepared
        .get("hint")
        .cloned()
        .unwrap_or_else(|| json!("Consulte reason; nenhum envio remoto foi confirmado."));
    prepared
}

/// Create each ancestor without following existing links. Atomic writes
/// use fresh sibling tempfiles and never reuse an attacker-controlled `.tmp`.
fn safe_directory(root: &Path, directory: &Path) -> Result<(), String> {
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    let relative = directory.strip_prefix(root).map_err(|e| e.to_string())?;
    let mut current = base.clone();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("unsafe publication path".into());
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err("publication ancestor is not a regular directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.to_string()),
                }
                let meta = std::fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("unsafe publication ancestor".into());
                }
            }
            Err(error) => return Err(error.to_string()),
        }
        if !current
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&base)
        {
            return Err("publication path escapes project".into());
        }
    }
    Ok(())
}
