//! A pasta do plugin do Mustard que cada processo do Claude deste projeto usa.
//!
//! A pasta vem primeiro do registro de uso das pastas do cache de plugins
//! (`<configuração do Claude>/plugins/cache/<catálogo>/mustard/<versão>/.in_use/<pid>`),
//! que o Claude Code grava para cada processo que carrega a pasta. Só sem ele
//! entram os `--plugin-dir` dos argumentos: o aplicativo passa por ali pastas
//! sincronizadas que o processo recebe sem rodar gancho nenhum delas. Uma
//! pasta sem `bin/mustard-rt` nunca é dada como a do processo, porque é esse
//! binário que o gancho roda.
//!
//! Só lê: nada de prompt, transcrição ou ambiente vai para a resposta. A única
//! execução é o `--version` do binário achado, que só o diagnóstico pede
//! ([`add_binary_versions`]); o painel lê a mesma resposta sem ela.
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn snapshot(root: &Path) -> Value {
    #[cfg(target_os="linux")]
    { inspect(Path::new("/proc"), mustard_core::claude_config_dir().as_deref(), root) }
    #[cfg(not(target_os="linux"))]
    { let _=root; json!({"supported":false,"observations":[],"status":"not-observed-on-this-platform"}) }
}

/// Acrescenta a cada observação de `report` a linha que o `bin/mustard-rt`
/// dela responde a `--version` (`rt_version`, nula quando ele não abre ou não
/// responde no prazo). Roda fora do projeto e sem passar a chamada a outro
/// binário: a versão lida é a do binário que o gancho executa.
pub(crate) fn add_binary_versions(report: &mut Value) {
    for row in report.get_mut("observations").and_then(Value::as_array_mut).into_iter().flatten() {
        let version=row["rt_binary"].as_str()
            .and_then(|binary|crate::shared::development_build::version_of(Path::new(binary)))
            .and_then(|text|text.lines().next().map(str::to_owned));
        row["rt_version"]=json!(version);
    }
}

/// As observações de cada processo do Claude cujo diretório de trabalho fica
/// em `root`, lidas da pasta de processos `processes` e do cache de plugins da
/// pasta de configuração `config`.
#[cfg(target_os="linux")]
pub(crate) fn inspect(processes: &Path, config: Option<&Path>, root: &Path) -> Value {
    use std::io::Read;
    let mut observations=Vec::new();
    let Ok(root)=root.canonicalize() else {return json!({"supported":true,"observations":observations,"status":"project-unavailable"})};
    let cached=cached_plugins(config);
    for process in std::fs::read_dir(processes).into_iter().flatten().filter_map(Result::ok) {
        let path=process.path();
        let Some(pid)=process.file_name().to_str().and_then(|name|name.parse::<u32>().ok()) else {continue};
        let Ok(cwd)=path.join("cwd").canonicalize() else {continue};
        if !cwd.starts_with(&root) {continue;}
        let Ok(executable)=path.join("exe").canonicalize() else {continue};
        let name=executable.file_name().and_then(|name|name.to_str()).unwrap_or_default();
        let directory=executable.parent().and_then(|parent|parent.file_name()).and_then(|name|name.to_str());
        if name!="claude" && !matches!(directory,Some("ccd-cli"|"versions")) {continue;}
        let started=start_time(&path);
        let mut rows:Vec<Value>=cached.iter().filter(|plugin|registered(plugin,pid,started.as_deref()))
            .filter_map(|plugin|observation(pid,plugin,"plugin-cache-in-use")).collect();
        if rows.is_empty() {
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
                rows.extend(observation(pid,plugin,"explicit-process-plugin-dir"));
            }
        }
        observations.extend(rows);
    }
    observations.sort_by_key(|entry|(entry["pid"].as_u64(),entry["plugin_root"].as_str().unwrap_or_default().to_owned()));
    let omitted=observations.len().saturating_sub(32);observations.truncate(32);
    json!({"supported":true,"status":if observations.is_empty(){"not-observed"}else{"observed"},
        "observations":observations,"omitted":omitted,"model_calls":0,"network_requests":0,
        "scope":"The plugin folder each Claude process of this project uses: the plugin cache's in-use record first, explicit --plugin-dir arguments only without it, and only a folder holding bin/mustard-rt. Not proof that a module loaded or a Desktop pane rendered."})
}

/// As pastas instaladas do plugin no cache da configuração do Claude, uma por
/// catálogo e versão, em ordem.
#[cfg(target_os="linux")]
fn cached_plugins(config: Option<&Path>) -> Vec<std::path::PathBuf> {
    let Some(cache)=config.map(|config|config.join("plugins").join("cache")) else {return Vec::new()};
    let mut found:Vec<_>=std::fs::read_dir(cache).into_iter().flatten().filter_map(Result::ok)
        .flat_map(|catalog|std::fs::read_dir(catalog.path().join(mustard_core::PLUGIN_NAME)).into_iter().flatten().filter_map(Result::ok))
        .map(|install|install.path()).collect();
    found.sort();found
}

/// O instante em que o processo de `process` começou, como `/proc/<pid>/stat`
/// o conta (o 22º campo): o mesmo número que o registro de uso guarda em
/// `procStart`.
#[cfg(target_os="linux")]
fn start_time(process: &Path) -> Option<String> {
    let stat=std::fs::read_to_string(process.join("stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19).map(str::to_owned)
}

/// Se o registro de uso de `plugin` traz o processo `pid` que começou em
/// `started`. O registro que um processo morto deixou para trás, com um número
/// que outro processo herdou depois, guarda outro começo e não conta.
#[cfg(target_os="linux")]
fn registered(plugin: &Path, pid: u32, started: Option<&str>) -> bool {
    let entry=plugin.join(".in_use").join(pid.to_string());
    if !entry.metadata().is_ok_and(|meta|meta.is_file() && meta.len()<=4_096) {return false;}
    let recorded=std::fs::read(&entry).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok());
    match recorded.as_ref().and_then(|record|record["procStart"].as_str()) {
        Some(recorded)=>started==Some(recorded),
        None=>true,
    }
}

/// O que a pasta `plugin` diz do processo `pid`, ou nada quando ela não é do
/// Mustard ou não traz o `bin/mustard-rt` que o gancho roda.
#[cfg(target_os="linux")]
fn observation(pid: u32, plugin: &Path, source: &str) -> Option<Value> {
    let manifest=plugin.join(".claude-plugin/plugin.json");
    if !manifest.metadata().is_ok_and(|meta|meta.len()<=65_536) {return None;}
    let metadata=std::fs::read(manifest).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok())?;
    if metadata["name"]!=mustard_core::PLUGIN_NAME {return None;}
    let binary=plugin.join("bin").join("mustard-rt");
    if !binary.is_file() {return None;}
    let version=metadata["version"].as_str().filter(|version|version.len()<=120 && version.chars().all(|ch|ch.is_ascii_alphanumeric() || ".-+".contains(ch)));
    Some(json!({"pid":pid,"plugin_root":plugin.display().to_string(),"plugin_version":version,
        "matches_running_version":version.map(|version|version==mustard_core::harness_version()),
        "mods_module_present":plugin.join("hooks/register.js").is_file(),
        "rt_binary":binary.display().to_string(),
        "source":source,"module_execution_verified":false}))
}

#[cfg(all(test,target_os="linux"))]
mod tests {
    use super::*;
    /// Sem registro de uso no cache, a pasta vem dos `--plugin-dir` do processo
    /// do projeto, e só a que traz o `bin/mustard-rt`; nenhum outro argumento
    /// nem outro projeto entra na resposta.
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
        let config=dir.path().join("config");
        assert_eq!(inspect(&proc,Some(&config),&root)["observations"],json!([]),"a pasta sem o binário do gancho não é a do processo");
        std::fs::create_dir(plugin.join("bin")).unwrap();std::fs::write(plugin.join("bin/mustard-rt"),"").unwrap();
        let result=inspect(&proc,Some(&config),&root);let rows=result["observations"].as_array().unwrap();
        assert_eq!(rows.len(),1);assert_eq!(rows[0]["pid"],11);assert_eq!(rows[0]["plugin_version"],"0.1.68");
        assert_eq!(rows[0]["source"],"explicit-process-plugin-dir");
        assert_eq!(rows[0]["rt_binary"],plugin.join("bin/mustard-rt").display().to_string());
        assert_eq!(rows[0]["mods_module_present"],false);assert_eq!(rows[0]["matches_running_version"],false);
        assert!(!result.to_string().contains("PRIVATE-"));
        std::fs::create_dir(plugin.join("hooks")).unwrap();std::fs::write(plugin.join("hooks/register.js"),"export default () => {};").unwrap();
        assert_eq!(inspect(&proc,Some(&config),&root)["observations"][0]["mods_module_present"],true);
    }
}
