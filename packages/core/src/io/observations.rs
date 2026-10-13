//! Immutable receipts of a host tool result. A receipt proves what was
//! observed, not that the model's interpretation of it is true.
use crate::io::sha256::Sha256;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub spec: String,
    pub dispatch: u64,
    pub wave: u64,
    pub host: String,
    pub actor: String,
    pub call: String,
    pub tool: String,
    pub source: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub exit_code: Option<i64>,
    pub result_status: String,
    pub observed_at: String,
}

pub fn digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.hex_digest()
}

fn open(root: &Path, write: bool) -> Result<Connection, String> {
    let path = root.join(".claude/mustard/observations.db");
    let mut ancestor = path.clone();
    while !ancestor.exists() {
        if !ancestor.pop() {
            return Err("observation-outside-project".into());
        }
    }
    if !ancestor
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .is_some_and(|(p, r)| p.starts_with(r))
        || path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("observation-outside-project".into());
    }
    if write {
        std::fs::create_dir_all(path.parent().ok_or("observation-missing-directory")?)
            .map_err(|e| e.to_string())?;
    }
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection =
        Connection::open_with_flags(path, flags).map_err(|_| "observation-store-unavailable")?;
    connection
        .busy_timeout(Duration::from_millis(15))
        .map_err(|e| e.to_string())?;
    if write {
        connection.execute_batch("CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, spec TEXT NOT NULL, dispatch INTEGER NOT NULL, observed_at TEXT NOT NULL, payload TEXT NOT NULL); CREATE INDEX IF NOT EXISTS receipts_scope ON receipts(spec,dispatch,observed_at);").map_err(|e|e.to_string())?;
    }
    Ok(connection)
}

/// Retrying the same native call never renews its observation time. A changed
/// payload for the same call is a conflict, not another observation.
pub fn record(root: &Path, receipt: &Receipt) -> Result<(String, Receipt), String> {
    let id = digest(
        &serde_json::to_vec(&(&receipt.host, &receipt.actor, &receipt.call))
            .map_err(|e| e.to_string())?,
    );
    let connection = open(root, true)?;
    let payload = serde_json::to_string(receipt).map_err(|e| e.to_string())?;
    let dispatch = i64::try_from(receipt.dispatch).map_err(|_| "observation-invalid-dispatch")?;
    connection
        .execute(
            "INSERT OR IGNORE INTO receipts VALUES(?1,?2,?3,?4,?5)",
            params![id, receipt.spec, dispatch, receipt.observed_at, payload],
        )
        .map_err(|e| e.to_string())?;
    let old: String = connection
        .query_row("SELECT payload FROM receipts WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    let prior: Receipt = serde_json::from_str(&old).map_err(|_| "observation-corrupt-receipt")?;
    let mut expected = receipt.clone();
    expected.observed_at.clone_from(&prior.observed_at);
    if serde_json::to_value(&expected).ok() != serde_json::to_value(&prior).ok() {
        return Err("observation-conflicting-tool-result".into());
    }
    Ok((id, prior))
}

pub fn read(root: &Path, id: &str) -> Result<Receipt, String> {
    if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("observation-invalid-receipt-id".into());
    }
    let connection = open(root, false)?;
    let payload: String = connection
        .query_row(
            "SELECT payload FROM receipts WHERE id=?1 AND length(payload)<=8192",
            [id],
            |row| row.get(0),
        )
        .map_err(|_| "observation-receipt-not-found")?;
    serde_json::from_str(&payload).map_err(|_| "observation-corrupt-receipt".into())
}

pub fn page(root: &Path, spec: &str, dispatch: u64, before: Option<&str>) -> Result<Value, String> {
    let dispatch = i64::try_from(dispatch).map_err(|_| "observation-invalid-dispatch")?;
    let cursor = before
        .map(|id| {
            read(root, id).and_then(|receipt| {
                if receipt.spec != spec || receipt.dispatch != dispatch as u64 {
                    return Err("observation-cursor-belongs-to-another-dispatch".into());
                }
                Ok((receipt.observed_at, id))
            })
        })
        .transpose()?;
    let connection = open(root, false)?;
    let mut statement=connection.prepare("SELECT id,payload FROM receipts WHERE spec=?1 AND dispatch=?2 AND (?3 IS NULL OR observed_at<?3 OR (observed_at=?3 AND id<?4)) ORDER BY observed_at DESC,id DESC LIMIT 33").map_err(|e|e.to_string())?;
    let rows = statement
        .query_map(
            params![
                spec,
                dispatch,
                cursor.as_ref().map(|c| &c.0),
                cursor.as_ref().map(|c| c.1)
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    for row in rows {
        let (id, payload) = row.map_err(|e| e.to_string())?;
        let receipt: Receipt =
            serde_json::from_str(&payload).map_err(|_| "observation-corrupt-receipt")?;
        output.push(json!({"receipt":id,"observed_at":receipt.observed_at,"dispatch":receipt.dispatch,
            "tool":receipt.tool,"source":receipt.source,"input_sha256":receipt.input_sha256,"output_sha256":receipt.output_sha256,
            "exit_code":receipt.exit_code,"result_status":receipt.result_status,"claim_verified":false}));
    }
    let next_before = (output.len() > 32).then(|| output[31]["receipt"].clone());
    output.truncate(32);
    Ok(json!({"receipts":output,"next_before":next_before}))
}

pub fn list(root: &Path, spec: &str, dispatch: u64) -> Result<Vec<Value>, String> {
    Ok(page(root, spec, dispatch, None)?["receipts"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// The only accepted assertion of freshness is an immutable native receipt
/// from this exact dispatch. Historical references retain the original date.
pub fn resolve(
    root: &Path,
    spec: &str,
    dispatch: u64,
    sent_at: &str,
    entries: &Value,
) -> Result<Value, String> {
    let entries = entries
        .as_array()
        .filter(|e| e.len() <= 16)
        .ok_or("observations-expect-at-most-16-references")?;
    let since = chrono::DateTime::parse_from_rfc3339(sent_at)
        .map_err(|_| "observation-unknown-dispatch-time")?;
    let mut output = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("observation-invalid-reference")?;
        if object
            .keys()
            .any(|key| !["receipt", "mode", "text"].contains(&key.as_str()))
        {
            return Err("observation-reference-accepts-only-receipt-mode-text".into());
        }
        let id = entry["receipt"]
            .as_str()
            .ok_or("observation-missing-receipt")?;
        let mode = entry["mode"]
            .as_str()
            .filter(|m| ["current", "historical"].contains(m))
            .ok_or("observation-mode-must-be-current-or-historical")?;
        let text = entry["text"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 2000)
            .ok_or("observation-needs-short-reported-claim")?;
        let receipt = read(root, id)?;
        let observed = chrono::DateTime::parse_from_rfc3339(&receipt.observed_at)
            .map_err(|_| "observation-invalid-time")?;
        if receipt.version != 1 || receipt.spec != spec {
            return Err("observation-belongs-to-another-spec".into());
        }
        if mode == "current"
            && (receipt.dispatch != dispatch || observed < since || observed > chrono::Utc::now())
        {
            return Err("observation-historical-source-cannot-be-current; recheck with a tool or use historical".into());
        }
        output.push(json!({"receipt":id,"mode":mode,"text":text,"observed_at":receipt.observed_at,
            "dispatch":receipt.dispatch,"wave":receipt.wave,"host":receipt.host,"tool":receipt.tool,"source":receipt.source,
            "exit_code":receipt.exit_code,"result_status":receipt.result_status,"input_sha256":receipt.input_sha256,"output_sha256":receipt.output_sha256,
            "claim_status":"reported-interpretation","tool_result_observed":true,"claim_verified":false}));
    }
    Ok(json!(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> Receipt {
        Receipt {
            version: 1,
            spec: "demo".into(),
            dispatch: 10,
            wave: 1,
            host: "claude".into(),
            actor: "hashed-actor".into(),
            call: "hashed-call".into(),
            tool: "Bash".into(),
            source: "read-only probe".into(),
            input_sha256: digest(b"probe"),
            output_sha256: digest(b"denied"),
            exit_code: Some(1),
            result_status: "failed".into(),
            observed_at: "2026-01-01T01:00:00Z".into(),
        }
    }
    #[test]
    fn a_replayed_call_keeps_its_date_and_conflicting_output_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = receipt();
        let (id, saved) = record(dir.path(), &first).unwrap();
        first.observed_at = "2026-01-02T01:00:00Z".into();
        assert_eq!(
            record(dir.path(), &first).unwrap().1.observed_at,
            saved.observed_at
        );
        first.output_sha256 = digest(b"allowed");
        assert!(
            record(dir.path(), &first)
                .unwrap_err()
                .contains("conflicting")
        );
        first.output_sha256 = digest(b"denied");
        first.dispatch = 11;
        assert!(
            record(dir.path(), &first)
                .unwrap_err()
                .contains("conflicting")
        );
        assert_eq!(read(dir.path(), &id).unwrap().exit_code, Some(1));
    }
    #[test]
    fn an_old_dispatch_is_historical_and_cannot_be_dated_as_current() {
        let dir = tempfile::tempdir().unwrap();
        let (id, _) = record(dir.path(), &receipt()).unwrap();
        let entry =
            |mode| json!([{"receipt":id,"mode":mode,"text":"The bucket was inaccessible."}]);
        assert!(
            resolve(
                dir.path(),
                "demo",
                20,
                "2026-01-02T00:00:00Z",
                &entry("current")
            )
            .is_err()
        );
        let history = resolve(
            dir.path(),
            "demo",
            20,
            "2026-01-02T00:00:00Z",
            &entry("historical"),
        )
        .unwrap();
        assert_eq!(history[0]["observed_at"], "2026-01-01T01:00:00Z");
        assert_eq!(history[0]["claim_verified"], false);
        assert!(
            resolve(
                dir.path(),
                "other",
                10,
                "2026-01-01T00:00:00Z",
                &entry("current")
            )
            .is_err()
        );
        let forged =
            json!([{"receipt":id,"mode":"current","text":"Accessible","observed_at":"2026-01-03"}]);
        assert!(resolve(dir.path(), "demo", 10, "2026-01-01T00:00:00Z", &forged).is_err());
    }
    #[test]
    fn every_receipt_remains_accessible_without_loading_the_whole_history() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..35 {
            let mut row = receipt();
            row.call = format!("call-{n}");
            record(dir.path(), &row).unwrap();
        }
        let first = page(dir.path(), "demo", 10, None).unwrap();
        assert_eq!(first["receipts"].as_array().unwrap().len(), 32);
        let cursor = first["next_before"].as_str().unwrap();
        let last = page(dir.path(), "demo", 10, Some(cursor)).unwrap();
        assert_eq!(last["receipts"].as_array().unwrap().len(), 3);
        assert!(last["next_before"].is_null());
        let ids: std::collections::BTreeSet<_> = first["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .chain(last["receipts"].as_array().unwrap())
            .map(|r| r["receipt"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 35);
        assert!(page(dir.path(), "other", 10, Some(cursor)).is_err());
        assert!(page(dir.path(), "demo", 11, Some(cursor)).is_err());
    }
}
