//! Faithful post-execution search projection. Occurrences come exclusively
//! from the executed tool. Unknown/compound/truncated output passes through;
//! no search is rerun and literal occurrences require no remote judgement.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};
use serde_json::{Value, json};

pub(crate) fn after_search(input: &HookInput, ctx: &Ctx) -> Verdict {
    let context=executed_context(input,ctx);
    match (updated(input,ctx),context) {
        (Some(tool_output),context)=>Verdict::ToolOutput {tool_output,context},
        (None,Some(context))=>Verdict::Inject {context},
        (None,None)=>Verdict::Allow,
    }
}

/// Join only occurrences actually returned by a known tool output. Never run
/// the search again, reinterpret flags, or turn a partial index into absence.
fn executed_context(input:&HookInput,ctx:&Ctx)->Option<String> {
    if !ctx.config.search_answer() {return None;}
    let response=input.raw.get("tool_response")?;
    if response.get("truncated").and_then(Value::as_bool)==Some(true) {return None;}
    let stdout=match input.tool_name.as_deref()? {
        "Bash"=>{
            let command=input.tool_input.get("command")?.as_str()?;
            let parts=crate::hooks::bash::lex::segments(command);
            let part=parts.first().filter(|_|parts.len()==1)?;
            if part.name()!="rg" || part.piped || !part.redirects.is_empty() || !part.leading.is_empty()
                || command.contains(['$', '`', '\n', ';', '|', '&', '<', '>'])
                || !part.args.iter().any(|arg|matches!(arg.text.as_str(),"-n"|"--line-number"))
                || response.get("interrupted")?.as_bool()? || response.get("isImage")?.as_bool()?
                || !response.get("stderr")?.as_str()?.is_empty() {return None;}
            response.get("stdout")?.as_str()?
        }
        "Grep"=>{
            if input.tool_input.get("output_mode")?.as_str()?!="content" || input.tool_input.get("-n")?.as_bool()!=Some(true)
                || input.tool_input.get("multiline").and_then(Value::as_bool)==Some(true) {return None;}
            response.get("content")?.as_str()?
        }
        _=>return None,
    };
    if stdout.is_empty() || stdout.len()>128*1024 {return None;}
    let parser=regex::Regex::new(r"^(.+?):([0-9]+):(.*)$").ok()?;
    let root=Path::new(&ctx.project_dir).canonicalize().ok()?;
    let base=input.cwd.as_deref().unwrap_or(&ctx.project_dir);
    let mut tree=None;
    let mut found=Vec::new();
    for line in stdout.lines() {
        let capture=parser.captures(line)?;
        let path=super::code_route::project_path(root.to_str()?,base,capture.get(1)?.as_str())?;
        let canonical=path.abs.canonicalize().ok()?;
        let current_tree=path.tree.canonicalize().ok()?;
        if !canonical.starts_with(&current_tree) || tree.as_ref().is_some_and(|previous|previous!=&current_tree) {return None;}
        tree=Some(current_tree);
        found.push((path.rel,capture.get(2)?.as_str().parse::<u64>().ok()?,capture.get(3)?.as_str()));
    }
    let hits:Vec<_>=found.iter().map(|(file,line,text)|mustard_core::io::knowledge::investigation::Occurrence {file,line:*line,text}).collect();
    let report=mustard_core::io::knowledge::investigation::cross_hits(&root,&tree?,&hits).ok()?;
    if report["symbols"].as_array()?.is_empty() {return None;}
    let mut context=format!("Mustard: current static owners of the executed search hits. Other occurrences remain in the original result. Static ownership does not prove runtime behavior.\n{report}");
    if context.len()>3500 {return None;}
    if let Some(description)=input.tool_description().filter(|text|!text.trim().is_empty()) {
        let _=write!(context,"\nSearch purpose supplied by agent: {}",description.chars().take(300).collect::<String>());
    }
    // Reuse evidence within this agent/session, invalidated by source hashes.
    if let Some(memory)=evidence_memory(&root,input.session_id.as_deref(),input.agent_id.as_deref()) {
        let mut digest=mustard_core::io::sha256::Sha256::new();digest.update(report.to_string().as_bytes());let key=digest.hex_digest();
        let mut seen=std::fs::read_to_string(&memory).unwrap_or_default();
        if seen.lines().any(|line|line==key) {return None;}
        if seen.len()>16*1024 {seen.clear();}
        seen.push_str(&key);seen.push('\n');
        let _=mustard_core::io::fs::write_atomic(memory,seen.as_bytes());
    }
    Some(context)
}

fn evidence_memory(root:&Path,session:Option<&str>,agent:Option<&str>)->Option<std::path::PathBuf> {
    let plain=|text:&str| !text.is_empty() && text.chars().all(|c|c.is_ascii_alphanumeric() || matches!(c,'-'|'_'));
    let session=session.filter(|name|plain(name) && *name!="unknown")?;
    let name=match agent {Some(name) if plain(name)=>format!("native-evidence-{name}"),Some(_)=>return None,None=>"native-evidence".into()};
    Some(mustard_core::ClaudePaths::for_project(root).ok()?.claude_dir().join(".session").join(session).join(name))
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

    fn indexed() -> tempfile::TempDir {
        let dir=tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let source="pub fn quartz() { restore(); }\n";
        std::fs::write(dir.path().join("src/main.rs"),source).unwrap();
        let mut digest=mustard_core::io::sha256::Sha256::new();digest.update(source.as_bytes());
        let mut raw=json!({"modules":[{"path":"src/main.rs","analysis":{"content_sha256":digest.hex_digest(),"parse_complete":true},"declarations":[{"name":"quartz","kind":"function","line":1,"end_line":1,"signature":"pub fn quartz()"}]}]});
        mustard_core::domain::knowledge::enrich(&mut raw);
        mustard_core::io::project_map::write_text(dir.path(),&raw.to_string()).unwrap();
        dir
    }

    fn executed(root:&Path,command:&str,stdout:&str)->(HookInput,Ctx) {
        let input=serde_json::from_value(json!({"tool_name":"Bash","cwd":root,"session_id":"search-test",
            "tool_input":{"command":command,"description":"find restoration"},
            "tool_response":{"stdout":stdout,"stderr":"","interrupted":false,"isImage":false}})).unwrap();
        (input,Ctx::for_test(root.to_str().unwrap(),None))
    }

    #[test]
    fn actual_occurrences_gain_current_owners_once_per_agent_and_never_replace_flags() {
        let dir=indexed();let root=dir.path();
        let (input,ctx)=executed(root,"rg -n -i -w restore src","src/main.rs:1:pub fn quartz() { restore(); }\n");
        let first=executed_context(&input,&ctx).unwrap();
        assert!(first.contains("quartz"));
        assert!(first.contains("current-static-owner"));
        assert!(executed_context(&input,&ctx).is_none(),"same source evidence is reused");
        let mut other=input.clone();other.agent_id=Some("other-agent".into());
        assert!(executed_context(&other,&ctx).is_some());
        // Reading a current occurrence does not validate the old source owner.
        std::fs::write(root.join("src/main.rs"),"pub fn quartz() { changed(); }\n").unwrap();
        assert!(executed_context(&other,&ctx).is_none());
        assert_eq!(input.tool_input["command"],"rg -n -i -w restore src");
    }

    #[test]
    fn unknown_count_context_compound_and_truncated_outputs_pass_through() {
        let dir=indexed();let root=dir.path();
        for (command,stdout) in [("rg -c restore src","src/main.rs:1\n"),("rg -n -C 2 restore src","src/main.rs-1-context\n"),
            ("rg -n restore src | head","src/main.rs:1:pub fn quartz() { restore(); }\n"),("rg -n restore src","{\"type\":\"match\"}\n")] {
            let (input,ctx)=executed(root,command,stdout);
            assert!(executed_context(&input,&ctx).is_none(),"{command}");
            assert!(updated(&input,&ctx).is_none(),"{command}");
        }
        let (mut input,ctx)=executed(root,"rg -n restore src","src/main.rs:1:pub fn quartz() { restore(); }\n");
        input.raw["tool_response"]["truncated"]=json!(true);
        assert!(matches!(after_search(&input,&ctx),Verdict::Allow));
        assert!(evidence_memory(root,Some("../escape"),None).is_none());
        assert!(evidence_memory(root,Some("search-test"),Some("../escape")).is_none());
    }

    #[test]
    fn search_hits_in_linked_worktrees_use_the_copy_and_subdirectory_paths() {
        let dir=indexed();let root=dir.path();
        let git=|args:&[&str]|mustard_core::platform::git::run(root,args);
        assert!(git(&["init","-q"]).ok);assert!(git(&["add","src"]).ok);
        assert!(git(&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-qm","seed"]).ok);
        let copy=tempfile::tempdir().unwrap();let tree=copy.path().join("copy");
        assert!(git(&["worktree","add","--quiet","--detach",tree.to_str().unwrap()]).ok);
        let (mut input,ctx)=executed(root,"rg -n restore .","main.rs:1:pub fn quartz() { restore(); }\n");
        input.cwd=Some(tree.join("src").to_str().unwrap().into());
        assert!(executed_context(&input,&ctx).unwrap().contains("quartz"));
        // Same line in a divergent copy must not acquire the main tree's owner.
        std::fs::write(tree.join("src/main.rs"),"pub fn quartz() { restore(); }\n// divergent copy\n").unwrap();
        input.agent_id=Some("copy-after-change".into());
        assert!(executed_context(&input,&ctx).is_none());
        assert_eq!(std::fs::read_to_string(root.join("src/main.rs")).unwrap(),"pub fn quartz() { restore(); }\n");
    }

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
