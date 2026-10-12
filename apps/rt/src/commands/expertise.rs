use std::path::Path;
use mustard_core::io::expertise;

pub fn run(root: &Path, sync: bool, record: Option<&Path>, review: Option<&str>, export: Option<&str>, host: &str, files: &[String]) {
    let result = (|| {
        if sync { return expertise::sync(root); }
        if let Some(file) = record {
            let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            if bytes.len() > 100_000 { return Err("expertise-manifest-too-large".into()); }
            let profile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            return expertise::record(root, &profile);
        }
        if let Some(value) = review {
            let (name, fingerprint) = value.split_once(':').ok_or("expertise-review-needs-name-and-fingerprint")?;
            return expertise::review(root, name, fingerprint);
        }
        if let Some(name) = export { return expertise::export(root, name, host); }
        expertise::report(root, files)
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(reason) => { println!("{}", serde_json::json!({"ok":false,"reason":reason})); std::process::exit(1); }
    }
}
