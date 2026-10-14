//! Explicit plugin directories supplied to a Claude process in this project.
//! Diagnostic-only: no polling, execution, transcript or environment capture.
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn snapshot(root: &Path) -> Value {
    #[cfg(target_os="linux")]
    { inspect(Path::new("/proc"), root) }
    #[cfg(not(target_os="linux"))]
    { let _=root; json!({"supported":false,"observations":[],"status":"not-observed-on-this-platform"}) }
}

#[cfg(target_os="linux")]
fn inspect(processes: &Path, root: &Path) -> Value {
    use std::io::Read;
    let mut observations=Vec::new();
    let Ok(root)=root.canonicalize() else {return json!({"supported":true,"observations":observations,"status":"project-unavailable"})};
    for process in std::fs::read_dir(processes).into_iter().flatten().filter_map(Result::ok) {
        let path=process.path();
        let Some(pid)=process.file_name().to_str().and_then(|name|name.parse::<u32>().ok()) else {continue};
        let Ok(cwd)=path.join("cwd").canonicalize() else {continue};
        if !cwd.starts_with(&root) {continue;}
        let Ok(executable)=path.join("exe").canonicalize() else {continue};
        let name=executable.file_name().and_then(|name|name.to_str()).unwrap_or_default();
        let directory=executable.parent().and_then(|parent|parent.file_name()).and_then(|name|name.to_str());
        if name!="claude" && !matches!(directory,Some("ccd-cli"|"versions")) {continue;}
        let Ok(file)=std::fs::File::open(path.join("cmdline")) else {continue};
        let mut raw=Vec::new();
        if file.take(262_145).read_to_end(&mut raw).is_err() || raw.len()>262_144 {continue;}
        let args:Vec<_>=raw.split(|byte|*byte==0).collect();
        let mut seen=std::collections::BTreeSet::new();
        for (index,arg) in args.iter().enumerate() {
            let value=if *arg==b"--plugin-dir" {args.get(index+1).copied()}
                else {arg.strip_prefix(b"--plugin-dir=")};
            let Some(value)=value.and_then(|value|std::str::from_utf8(value).ok()) else {continue};
            let plugin=Path::new(value);
            if !plugin.is_absolute() || !seen.insert(value) {continue;}
            let manifest=plugin.join(".claude-plugin/plugin.json");
            if !manifest.metadata().is_ok_and(|meta|meta.len()<=65_536) {continue;}
            let Some(metadata)=std::fs::read(manifest).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok()) else {continue};
            if metadata["name"]!="mustard" {continue;}
            let version=metadata["version"].as_str().filter(|version|version.len()<=120 && version.chars().all(|ch|ch.is_ascii_alphanumeric() || ".-+".contains(ch)));
            observations.push(json!({"pid":pid,"plugin_root":value,"plugin_version":version,
                "matches_running_version":version.map(|version|version==mustard_core::harness_version()),
                "mods_module_present":plugin.join("hooks/register.js").is_file(),
                "source":"explicit-process-plugin-dir","module_execution_verified":false}));
        }
    }
    observations.sort_by_key(|entry|(entry["pid"].as_u64(),entry["plugin_root"].as_str().unwrap_or_default().to_owned()));
    let omitted=observations.len().saturating_sub(32);observations.truncate(32);
    json!({"supported":true,"status":if observations.is_empty(){"not-observed"}else{"observed"},
        "observations":observations,"omitted":omitted,"model_calls":0,"network_requests":0,
        "scope":"Explicit process arguments in this project, not proof that a module loaded or a Desktop pane rendered."})
}

#[cfg(all(test,target_os="linux"))]
mod tests {
    use super::*;
    #[test]
    fn diagnoses_old_ssh_plugin_without_capturing_prompts_or_other_projects() {
        use std::os::unix::fs::symlink;
        let dir=tempfile::tempdir().unwrap();let root=dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let plugin=dir.path().join("plugin");std::fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
        std::fs::write(plugin.join(".claude-plugin/plugin.json"),r#"{"name":"mustard","version":"0.1.68"}"#).unwrap();
        let executable=dir.path().join("ccd-cli/2.1.295");std::fs::create_dir_all(executable.parent().unwrap()).unwrap();std::fs::write(&executable,"").unwrap();
        let proc=dir.path().join("proc");
        for pid in ["11","12"] {
            let p=proc.join(pid);std::fs::create_dir_all(&p).unwrap();
            symlink(if pid=="11"{&root}else{dir.path()},p.join("cwd")).unwrap();symlink(&executable,p.join("exe")).unwrap();
            std::fs::write(p.join("cmdline"),format!("claude\0--plugin-dir\0{}\0--prompt\0PRIVATE-PROMPT-123\0--token=PRIVATE-KEY-123\0",plugin.display())).unwrap();
        }
        let result=inspect(&proc,&root);let rows=result["observations"].as_array().unwrap();
        assert_eq!(rows.len(),1);assert_eq!(rows[0]["pid"],11);assert_eq!(rows[0]["plugin_version"],"0.1.68");
        assert_eq!(rows[0]["mods_module_present"],false);assert_eq!(rows[0]["matches_running_version"],false);
        assert!(!result.to_string().contains("PRIVATE-"));
        std::fs::create_dir(plugin.join("hooks")).unwrap();std::fs::write(plugin.join("hooks/register.js"),"export default () => {};").unwrap();
        assert_eq!(inspect(&proc,&root)["observations"][0]["mods_module_present"],true);
    }
}
