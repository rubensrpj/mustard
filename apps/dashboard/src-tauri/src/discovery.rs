use mustard_core::io::fs;
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub db_path: Option<String>,
    pub last_activity_ms: Option<u64>,
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "target",
    ".next",
    "vendor",
    ".obsidian",
];

const MAX_DEPTH: u32 = 5;

pub fn discover(root: &Path) -> Result<Vec<Project>, String> {
    if !root.exists() || !root.is_dir() {
        return Ok(vec![]);
    }
    let mut results: Vec<Project> = Vec::new();
    let mut queue: VecDeque<(PathBuf, u32)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        let db_path = dir.join(".claude").join(".harness").join("mustard.db");
        let json_path = dir.join("mustard.json");
        let has_db = db_path.is_file();
        let has_json = json_path.is_file();
        if has_db || has_json {
            let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let canonical_str = canonical.to_string_lossy().to_string();
            let id = fnv1a_hex(canonical_str.as_bytes());
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| canonical_str.clone());
            // last_activity_ms: prefer mustard.db mtime (canonical store);
            // fall back to mustard.json mtime when only the JSON config exists.
            let last_activity_ms = if has_db {
                mtime_ms(&db_path)
            } else {
                mtime_ms(&json_path)
            };
            let db_path_value = if has_db {
                Some(db_path.to_string_lossy().to_string())
            } else {
                None
            };
            results.push(Project {
                id,
                name,
                path: dir.to_string_lossy().to_string(),
                db_path: db_path_value,
                last_activity_ms,
            });
            continue;
        }

        if depth >= MAX_DEPTH {
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            if SKIP_DIRS.iter().any(|s| OsStr::new(s) == OsStr::new(&entry.file_name)) {
                continue;
            }
            queue.push_back((entry.path, depth + 1));
        }
    }

    Ok(results)
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

fn mtime_ms(p: &Path) -> Option<u64> {
    fs::modified(p)
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}
