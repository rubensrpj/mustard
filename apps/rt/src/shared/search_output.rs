//! Faithful post-execution search projection. Occurrences come exclusively
//! from the executed tool. Unknown/compound/truncated output passes through;
//! no search is rerun and literal occurrences require no remote judgement.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};
use serde_json::{Value, json};

pub(crate) fn after_search(input: &HookInput, ctx: &Ctx) -> Verdict {
    updated(input, ctx).map(|tool_output| Verdict::ToolOutput { tool_output }).unwrap_or(Verdict::Allow)
}

fn updated(input: &HookInput, ctx: &Ctx) -> Option<Value> {
    if input.tool_name.as_deref() != Some("Bash") || !ctx.config.search_answer() {
        return None;
    }
    let command = input.tool_input.get("command")?.as_str()?;
    let segments = crate::hooks::bash::lex::segments(command);
    if segments.len() != 1 {
        return None;
    }
    let segment = segments.first()?;
    if segment.name() != "rg" || segment.piped || !segment.redirects.is_empty() || !segment.leading.is_empty() {
        return None;
    }
    // Strict output contract: unsupported options, substitutions and wrappers
    // are never transformed merely because their output resembles a hit.
    if segment.program.raw != "rg" || command.contains(['$', '`', '\n', ';', '|', '&', '<', '>']) {
        return None;
    }
    let allowed = ["-n", "--line-number", "--no-heading", "-F", "--fixed-strings", "-i", "--ignore-case", "-w", "--word-regexp", "--"];
    if segment.args.iter().any(|arg| arg.text.starts_with('-') && !allowed.contains(&arg.text.as_str())) {
        return None;
    }
    if !segment.args.iter().any(|arg| matches!(arg.text.as_str(), "-n" | "--line-number")) {
        return None;
    }
    let response = input.raw.get("tool_response")?.as_object()?;
    if response.get("interrupted")?.as_bool()? || response.get("isImage")?.as_bool()? {
        return None;
    }
    let stdout = response.get("stdout")?.as_str()?;
    let stderr = response.get("stderr")?.as_str()?;
    if !stderr.is_empty() || stdout.is_empty() || response.get("truncated").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let base = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let projected = windows(base, stdout)?;
    // Do not enlarge a short result or inject repeated discovery.
    if projected.len() >= stdout.len() {
        return None;
    }
    let mut out = response.clone();
    out.insert("stdout".into(), json!(projected));
    Some(Value::Object(out))
}

/// All occurrences remain labelled as hits. Adjacent windows are merged,
/// and coverage labels describe only the lines actually delivered.
fn windows(base: &Path, stdout: &str) -> Option<String> {
    let hit = regex::Regex::new(r"^(.+?):([0-9]+):(.*)$").ok()?;
    let mut last: Option<(String, usize)> = None;
    let mut order = Vec::new();
    let mut files: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for line in stdout.lines() {
        let captures = hit.captures(line)?;
        let file = captures.get(1)?.as_str();
        let number = captures.get(2)?.as_str().parse::<usize>().ok()?.checked_sub(1)?;
        // Preserve execution order and every occurrence. Unusual interleaved
        // or repeated output cannot be faithfully coalesced by this schema.
        if let Some((prior, at)) = &last {
            if prior == file && number <= *at {
                return None;
            }
            if prior != file && files.contains_key(file) {
                return None;
            }
        }
        if !files.contains_key(file) {
            order.push(file.to_string());
        }
        last = Some((file.into(), number));
        files.entry(file.into()).or_default().push((number, captures.get(3)?.as_str().into()));
    }
    let mut result = String::new();
    for file in order {
        let hits = files.remove(&file)?;
        let path = base.join(&file);
        let canonical = path.canonicalize().ok()?;
        if !canonical.starts_with(base.canonicalize().ok()?) || crate::shared::paths::sensitive_pattern(&file).is_some() {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let lines: Vec<_> = text.lines().collect();
        for (at, found) in &hits {
            if lines.get(*at).copied() != Some(found.as_str()) {
                return None;
            }
        }
        let mut ranges: Vec<(usize, usize)> = hits.iter().map(|(at, _)| (at.saturating_sub(3), (at + 4).min(lines.len()))).collect();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        for (start, end) in merged {
            let _ = writeln!(result, "{file}:{}-{end}", start + 1);
            for (at, line) in lines.iter().enumerate().take(end).skip(start) {
                let is_hit = hits.iter().any(|(found, _)| *found == at);
                let _ = writeln!(result, "{} {} | {line}", if is_hit { "hit" } else { "   " }, at + 1);
            }
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_include_a_hit_at_the_end_of_long_code_and_fail_open_on_stale_or_unknown_output() {
        let dir = tempfile::tempdir().unwrap();
        let text = (1..=120).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.path().join("new.txt"), text).unwrap();
        let projected = windows(dir.path(), "new.txt:119:line 119\n").unwrap();
        assert!(projected.contains("hit 119 | line 119"));
        assert!(projected.contains("new.txt:116-120"));
        assert!(!projected.contains("line 1\n"));
        assert!(windows(dir.path(), "new.txt:119:stale\n").is_none());
        assert!(windows(dir.path(), "{\"type\":\"match\"}\n").is_none());
        assert!(windows(dir.path(), "../outside:1:x\n").is_none());
    }
}
