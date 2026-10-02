//! `mustard-rt run measure`: roda uma régua (um teste ignorado) compilando o
//! código certo.
//!
//! Escolher à mão o programa de teste, pelo mais novo da pasta de compilação,
//! já trouxe número de programa errado. Aqui ninguém escolhe programa:
//!
//! - cada código tem a sua pasta de compilação, `<medida>/<commit12>[-sujo-<resumo>]`,
//!   com o `target/` dentro: o mesmo código reaproveita a compilação, e
//!   código diferente nunca divide pasta;
//! - o programa de teste vem do JSON do cargo (`--message-format=json`), nunca
//!   da data de um arquivo;
//! - a régua roda com o commit, o sujo e o resumo do que falta comitar em
//!   variáveis `MUSTARD_MEASURE_*`, e com o `scan` compilado do mesmo código ao
//!   lado do programa (a régua recusa sem eles);
//! - o mapa de cada projeto da pasta de árvores (`--trees`, ou `SPEND_TREES`
//!   em `--env`) é refeito a cada medida, com esse `scan`, no mesmo passo do
//!   mapa das sessões: o banco velho sai antes, o scan que falha recusa a
//!   medida, e a leitura da história de cada declaração roda depois dele e
//!   acaba antes de a régua começar (a que falha, ou que outra leitura do
//!   mesmo mapa impede, recusa a medida); a linha `PECAS` diz se a história
//!   chegou;
//! - a prova diz também qual versão do gancho as sessões do usuário rodam: o
//!   commit que o `mustard-rt` do plugin instalado carimbou em si;
//! - ao terminar, só as três pastas de medida usadas por último ficam.
//!
//! O comando só mede o código-fonte do Mustard: em outro projeto não há o que
//! compilar e ele recusa.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use mustard_core::domain::scan::Scan;
use mustard_core::io::measure_proof::{HOOK_NOT_INSTALLED, HOOK_WITHOUT_COMMIT, OUT_VAR, built_commit, measure_vars, rebuild_map};
use mustard_core::io::tree_state::tree_state;
use mustard_core::platform::git;
use mustard_core::platform::harness::{home_dir, installed_plugin_rt, installed_plugin_rt_in};
use serde_json::Value;

/// Quantas pastas de medida ficam depois de cada rodada.
const KEEP_FOLDERS: usize = 3;
/// O arquivo que cada pasta de medida leva e que se reescreve a cada uso: a
/// hora dele diz qual pasta foi usada por último, e só pasta com ele é nossa.
const USED_MARK: &str = ".usada";
/// Troca a pasta onde as compilações de medida moram.
const BASE_VAR: &str = "MUSTARD_MEASURE_DIR";
/// Quantos caracteres do commit nomeiam a pasta e carimbam o programa.
const COMMIT_CHARS: usize = 12;
/// O prefixo das variáveis que só o comando põe.
const RESERVED_PREFIX: &str = "MUSTARD_MEASURE_";
/// A linha que a régua imprime para provar a versão que usou.
const PROOF_PREFIX: &str = "PROVA ";
/// O começo da linha com o estado de cada peça da busca num mapa.
const PIECES_PREFIX: &str = "PECAS ";
/// A variável da régua do gasto que diz a pasta com a árvore de cada projeto.
const TREES_VAR: &str = "SPEND_TREES";

/// O que o usuário pediu em `mustard-rt run measure`.
#[derive(Debug)]
pub struct MeasureOpts {
    /// O nome da régua: o teste ignorado a rodar.
    pub test: String,
    /// Medir o código deste commit, numa cópia própria, em vez da pasta atual.
    pub commit: Option<String>,
    /// O pacote que tem a régua; sem ele, o comando a procura no código.
    pub package: Option<String>,
    /// Variáveis de ambiente para a régua, no formato `CHAVE=VALOR`.
    pub env: Vec<String>,
    /// Onde a régua grava o resultado.
    pub out: Option<PathBuf>,
    /// A pasta com a árvore de cada projeto da régua: o mapa de cada uma é
    /// refeito antes da medida. Sem ela, vale `SPEND_TREES` de `--env`.
    pub trees: Option<PathBuf>,
}

/// O que o comando pede à máquina e o teste troca por um falso.
#[derive(Debug, Default)]
struct Host {
    /// O cargo que compila; sem ele, o do ambiente.
    cargo: Option<PathBuf>,
    /// A pasta de configuração do Claude Code, onde o plugin do gancho mora;
    /// sem ela, a do usuário.
    config: Option<PathBuf>,
}

/// O código que a medida compila: o commit, o que falta comitar nele e a pasta
/// de onde sai.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Code {
    sha: String,
    dirty: bool,
    diff: String,
    date: Option<String>,
}

impl Code {
    /// O commit curto que nomeia a pasta e carimba o programa.
    fn short(&self) -> String {
        self.sha.chars().take(COMMIT_CHARS).collect()
    }
}

/// O programa de teste que o cargo compilou e a pasta do pacote dele.
#[derive(Debug, PartialEq, Eq)]
struct TestBinary {
    executable: PathBuf,
    manifest_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Funções puras
// ---------------------------------------------------------------------------

/// O nome da pasta de medida de um código: o commit curto e, quando havia
/// código por comitar, o resumo dele. Mesmo código, mesmo nome; código
/// diferente, nome diferente.
fn folder_name(commit: &str, diff: &str) -> String {
    let short: String = commit.chars().take(COMMIT_CHARS).collect();
    if diff.is_empty() { short } else { format!("{short}-sujo-{diff}") }
}

/// As variáveis `CHAVE=VALOR` do usuário. As `MUSTARD_MEASURE_*` são do
/// comando: a régua as lê como prova, e uma delas posta à mão a falsificaria.
fn parse_env(pairs: &[String]) -> Result<Vec<(String, String)>, String> {
    pairs
        .iter()
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .filter(|(key, _)| !key.is_empty() && !key.contains(char::is_whitespace))
                .ok_or_else(|| format!("`--env {pair}` não está no formato CHAVE=VALOR"))?;
            if key.starts_with(RESERVED_PREFIX) {
                return Err(format!("`{key}` é posta pelo comando de medida e não se passa em `--env`"));
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

/// O nome do pacote que um `package_id` do cargo traz. As duas formas: a
/// antiga, `nome 0.1.0 (path+file:///x)`, e a nova, `path+file:///x#nome@0.1.0`
/// ou `path+file:///x#0.1.0`, em que o nome é o da última pasta do caminho.
fn package_name_of_id(id: &str) -> Option<&str> {
    if id.contains(' ') {
        return id.split_whitespace().next();
    }
    let (url, fragment) = id.split_once('#')?;
    match fragment.split_once('@') {
        Some((name, _)) => Some(name),
        None => url.rsplit('/').next().filter(|name| !name.is_empty()),
    }
}

/// O programa de teste de unidade do pacote nas linhas JSON do cargo: o
/// `executable` do alvo que roda os testes de dentro do `src/`, e nenhum
/// outro. Recusa quando o cargo não compilou nenhum e quando compilou mais de
/// um, porque escolher entre dois seria adivinhar.
fn find_test_binary(cargo_json: &str, package: &str) -> Result<TestBinary, String> {
    let mut found: Vec<TestBinary> = Vec::new();
    for line in cargo_json.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else { continue };
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        if message["package_id"].as_str().and_then(package_name_of_id) != Some(package) {
            continue;
        }
        // O alvo de teste de integração, o de exemplo e o de benchmark também
        // têm programa; a régua mora no `src/`.
        let outside_src = message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| matches!(kind.as_str(), Some("test" | "bench" | "example" | "custom-build"))));
        if outside_src {
            continue;
        }
        let (Some(executable), Some(manifest)) = (message["executable"].as_str(), message["manifest_path"].as_str()) else {
            continue;
        };
        let manifest_dir = Path::new(manifest).parent().map(Path::to_path_buf).unwrap_or_default();
        let binary = TestBinary { executable: PathBuf::from(executable), manifest_dir };
        if !found.contains(&binary) {
            found.push(binary);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("o cargo não compilou programa de teste de unidade para o pacote `{package}`; a régua precisa estar no `src/` da biblioteca dele")),
        count => Err(format!(
            "o cargo compilou {count} programas de teste de unidade para o pacote `{package}` ({}); sem como escolher sem adivinhar",
            found.iter().map(|binary| binary.executable.display().to_string()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// O nome completo da régua, `modulo::caminho::nome`, na lista que o programa
/// de teste dá com `--list --ignored`. Recusa quando nenhum teste ignorado tem
/// esse nome e quando mais de um tem.
fn pick_test(list: &str, test: &str) -> Result<String, String> {
    let suffix = format!("::{test}");
    let names: Vec<&str> =
        list.lines().filter_map(|line| line.strip_suffix(": test")).filter(|name| *name == test || name.ends_with(&suffix)).collect();
    match names.as_slice() {
        [one] => Ok((*one).to_string()),
        [] => Err(format!("o programa de teste não tem teste ignorado chamado `{test}`; a régua é um `#[ignore]`")),
        many => Err(format!("`{test}` é o nome de {} testes ignorados ({}); a régua tem nome só dela", many.len(), many.join(", "))),
    }
}

/// O nome do pacote que o `Cargo.toml` declara.
fn package_name_in(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(rest) = line.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=')?.trim().strip_prefix('"')?;
            return value.split('"').next().map(str::to_string);
        }
    }
    None
}

/// Os arquivos `.rs` de uma pasta, de baixo para cima.
fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => rust_files(&path, into),
            Ok(kind) if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") => into.push(path),
            _ => {}
        }
    }
}

/// O pacote que tem a régua `test`: o único cujo `src/` declara `fn <teste>(`
/// em `apps/*` e `packages/*`. Recusa quando nenhum a tem e quando mais de um
/// a tem (`--package` desfaz).
fn package_of_test(root: &Path, test: &str) -> Result<String, String> {
    let needle = format!("fn {test}(");
    let mut packages: Vec<String> = Vec::new();
    for family in ["apps", "packages"] {
        let Ok(members) = std::fs::read_dir(root.join(family)) else { continue };
        for member in members.flatten() {
            let dir = member.path();
            let Some(name) = std::fs::read_to_string(dir.join("Cargo.toml")).ok().and_then(|text| package_name_in(&text)) else {
                continue;
            };
            let mut files = Vec::new();
            rust_files(&dir.join("src"), &mut files);
            if files.iter().any(|file| std::fs::read_to_string(file).is_ok_and(|text| text.contains(&needle))) && !packages.contains(&name) {
                packages.push(name);
            }
        }
    }
    packages.sort_unstable();
    match packages.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("não achei a régua `{test}` no código; diga o pacote dela com `--package`")),
        many => Err(format!("a régua `{test}` está em mais de um pacote ({}); diga qual com `--package`", many.join(", "))),
    }
}

/// A pasta é o repositório do Mustard: o `Cargo.toml` dos três pacotes que a
/// medida compila diz o nome esperado.
fn is_mustard_source(root: &Path) -> bool {
    [("apps/rt", "mustard-rt"), ("apps/scan", "scan"), ("packages/core", "mustard-core")].iter().all(|(dir, name)| {
        std::fs::read_to_string(root.join(dir).join("Cargo.toml")).ok().and_then(|text| package_name_in(&text)).as_deref() == Some(*name)
    })
}

/// O que sai das pastas de medida: tudo menos as `keep` usadas por último, a
/// que está em uso entre elas. Só devolve pasta que é filha direta de `base`:
/// nada fora dela, nem ela mesma, nem caminho com `..`.
fn pick_to_remove(entries: &[(PathBuf, SystemTime)], keep: usize, base: &Path, in_use: &Path) -> Vec<PathBuf> {
    let mut inside: Vec<&(PathBuf, SystemTime)> =
        entries.iter().filter(|(path, _)| path.parent() == Some(base) && path.file_name().is_some()).collect();
    // A pasta em uso vem primeiro: nunca sai, mesmo com a hora mais antiga.
    inside.sort_by(|a, b| (b.0 == in_use).cmp(&(a.0 == in_use)).then_with(|| b.1.cmp(&a.1)).then_with(|| a.0.cmp(&b.0)));
    inside.into_iter().skip(keep.max(1)).map(|(path, _)| path.clone()).collect()
}

/// As variáveis da compilação do código: a pasta de compilação e o carimbo do
/// commit, do sujo e do resumo que o `build.rs` põe no programa (`None`
/// tira a variável do ambiente herdado).
fn build_env(code: &Code, target: &Path) -> Vec<(&'static str, Option<String>)> {
    vec![
        ("CARGO_TARGET_DIR", Some(target.display().to_string())),
        ("MUSTARD_GIT_HASH", Some(code.short())),
        // A presença da variável é que diz "sujo": no código limpo ela sai.
        ("MUSTARD_GIT_DIRTY", code.dirty.then(|| "1".to_string())),
        ("MUSTARD_GIT_DIFF", Some(code.diff.clone())),
        ("MUSTARD_GIT_DATE", code.date.clone()),
    ]
}

/// As variáveis da execução da régua: a prova (commit, sujo, resumo e a
/// versão do gancho), o arquivo de resultado, as do usuário e a pasta do
/// pacote.
fn run_env(code: &Code, hook: &str, out: &Path, user: &[(String, String)], manifest_dir: &Path) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> =
        measure_vars(&code.short(), code.dirty, &code.diff, hook).into_iter().map(|(name, value)| (name.to_string(), value)).collect();
    vars.push((OUT_VAR.to_string(), out.display().to_string()));
    vars.extend(user.iter().cloned());
    vars.push(("CARGO_MANIFEST_DIR".to_string(), manifest_dir.display().to_string()));
    vars
}

/// A linha de prova que a régua imprimiu.
fn is_proof_line(line: &str) -> bool {
    line.trim_start().starts_with(PROOF_PREFIX)
}

/// `true` quando entre as linhas que a régua imprimiu não há a de prova: as
/// linhas das peças da busca sozinhas não dizem de que código saiu o número.
fn lacks_proof_line(lines: &[String]) -> bool {
    !lines.iter().any(|line| is_proof_line(line))
}

/// A linha com o estado das peças da busca que a régua imprimiu para um mapa.
fn is_pieces_line(line: &str) -> bool {
    line.trim_start().starts_with(PIECES_PREFIX)
}

/// O que o comando imprime ao fim: as linhas de prova, o caminho do resultado
/// e as pastas que apagou.
fn summary(proof: &[String], out: &Path, written: bool, removed: &[PathBuf]) -> String {
    let state = if written { "gravado" } else { "a régua não gravou o arquivo" };
    let mut lines: Vec<String> = proof.iter().map(|line| line.trim().to_string()).collect();
    lines.push(format!("resultado: {} ({state})", out.display()));
    lines.extend(removed.iter().map(|path| format!("apagada: {}", path.display())));
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Pastas
// ---------------------------------------------------------------------------

/// Onde as pastas de medida moram: `MUSTARD_MEASURE_DIR` ou
/// `~/.cache/mustard/medida`.
fn measure_base() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os(BASE_VAR).filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    home_dir().map(|home| home.join(".cache").join("mustard").join("medida")).ok_or_else(|| "não achei a pasta pessoal para guardar as compilações de medida".to_string())
}

/// As pastas de medida de `base` e a hora do último uso de cada uma. Só conta
/// pasta que o comando marcou: o que não tem a marca não é dele.
fn used_folders(base: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(base) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let used = std::fs::metadata(entry.path().join(USED_MARK)).ok()?.modified().ok()?;
            Some((entry.path(), used))
        })
        .collect()
}

/// Marca a pasta como usada agora, criando-a se for nova.
fn mark_used(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|err| format!("não consegui criar {}: {err}", folder.display()))?;
    std::fs::write(folder.join(USED_MARK), mustard_core::platform::time::now_iso8601())
        .map_err(|err| format!("não consegui marcar {}: {err}", folder.display()))
}

/// Apaga uma pasta de medida; a cópia de um commit sai do git antes, para ele
/// não guardar o registro dela.
fn remove_folder(folder: &Path, repo: &Path) -> bool {
    let copy = folder.join("src");
    if copy.join(".git").exists() {
        let _ = git::run(repo, &["worktree", "remove", "--force", &copy.display().to_string()]);
    }
    match std::fs::remove_dir_all(folder) {
        Ok(()) => true,
        Err(err) => err.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Mantém só as pastas de medida usadas por último; devolve as que apagou.
fn prune(base: &Path, in_use: &Path, repo: &Path) -> Vec<PathBuf> {
    let doomed = pick_to_remove(&used_folders(base), KEEP_FOLDERS, base, in_use);
    let removed: Vec<PathBuf> = doomed.into_iter().filter(|folder| remove_folder(folder, repo)).collect();
    if !removed.is_empty() {
        let _ = git::run(repo, &["worktree", "prune"]);
    }
    removed
}

// ---------------------------------------------------------------------------
// Código a medir
// ---------------------------------------------------------------------------

/// A raiz do repositório de `dir`; a própria pasta fora de um repositório.
fn repo_root(dir: &Path) -> PathBuf {
    git::run(dir, &["rev-parse", "--show-toplevel"]).out().filter(|top| !top.is_empty()).map_or_else(|| dir.to_path_buf(), PathBuf::from)
}

/// O código da pasta atual: o HEAD e o que falta comitar nele.
fn current_code(root: &Path) -> Result<Code, String> {
    let ask = |args: &[&str]| git::run(root, args).out();
    let sha = ask(&["rev-parse", "HEAD"]).filter(|sha| !sha.is_empty()).ok_or("não consegui ler o commit desta pasta: o git não respondeu")?;
    let state = tree_state(&ask).ok_or("não consegui ler o estado desta pasta: o git não respondeu")?;
    let date = ask(&["log", "-1", "--format=%cs"]).filter(|date| !date.is_empty());
    Ok(Code { sha, dirty: state.dirty, diff: state.diff, date })
}

/// O código de um commit, sempre limpo.
fn commit_code(root: &Path, rev: &str) -> Result<Code, String> {
    if rev.starts_with('-') {
        return Err(format!("`{rev}` não é um commit"));
    }
    let sha = git::run(root, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")])
        .out()
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| format!("o commit `{rev}` não existe neste repositório"))?;
    let date = git::run(root, &["log", "-1", "--format=%cs", &sha]).out().filter(|date| !date.is_empty());
    Ok(Code { sha, dirty: false, diff: String::new(), date })
}

/// A cópia do commit na pasta de medida, sem mexer na pasta atual: reaproveita
/// a que já está no commit e limpa, refaz a que mudou.
fn materialize(root: &Path, sha: &str, folder: &Path) -> Result<PathBuf, String> {
    let copy = folder.join("src");
    if copy.join(".git").exists() {
        let ask = |args: &[&str]| git::run(&copy, args).out();
        let intact = ask(&["rev-parse", "HEAD"]).as_deref() == Some(sha) && tree_state(&ask).is_some_and(|state| !state.dirty);
        if intact {
            return Ok(copy);
        }
        let _ = git::run(root, &["worktree", "remove", "--force", &copy.display().to_string()]);
        let _ = std::fs::remove_dir_all(&copy);
    }
    git::run(root, &["worktree", "add", "--detach", &copy.display().to_string(), sha])
        .result()
        .map_err(|err| format!("não consegui separar o código do commit {sha}: {err}"))?;
    Ok(copy)
}

// ---------------------------------------------------------------------------
// Mapas e gancho
// ---------------------------------------------------------------------------

/// A pasta com a árvore de cada projeto da régua: a de `--trees` ou, sem ela,
/// `SPEND_TREES` de `--env`. A régua roda de dentro do pacote: a pasta vai a
/// ela por caminho absoluto, em `SPEND_TREES`, para o mapa que se refaz ser o
/// mesmo que ela abre. `None` quando a medida não traz pasta de árvores.
fn trees_folder(cwd: &Path, opts: &MeasureOpts, user: &mut Vec<(String, String)>) -> Result<Option<PathBuf>, String> {
    let absolute = |path: &Path| if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let from_env = user.iter().find(|(key, _)| key == TREES_VAR).map(|(_, value)| absolute(Path::new(value)));
    let folder = match (opts.trees.as_deref().map(absolute), from_env) {
        (Some(flag), Some(env)) if flag != env => {
            return Err(format!("`--trees {}` e `--env {TREES_VAR}={}` apontam para pastas diferentes", flag.display(), env.display()));
        }
        (Some(folder), _) | (None, Some(folder)) => folder,
        (None, None) => return Ok(None),
    };
    if !folder.is_dir() {
        return Err(format!("a pasta de árvores {} não existe", folder.display()));
    }
    user.retain(|(key, _)| key != TREES_VAR);
    user.push((TREES_VAR.to_string(), folder.display().to_string()));
    Ok(Some(folder))
}

/// Refaz, com `scan`, o mapa de cada projeto da pasta de árvores: um projeto é
/// cada pasta dentro dela, fora as escondidas. Recusa na primeira que o scan
/// não refaz, e quando a pasta não tem projeto nenhum: a régua mediria sem
/// mapa novo.
fn rebuild_trees(trees: &Path, scan: &Scan) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(trees).map_err(|err| format!("não consegui ler a pasta de árvores {}: {err}", trees.display()))?;
    let mut projects: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.')))
        .collect();
    projects.sort();
    if projects.is_empty() {
        return Err(format!("a pasta de árvores {} não tem projeto nenhum para refazer o mapa", trees.display()));
    }
    for project in &projects {
        rebuild_map(project, scan).map_err(|err| err.to_string())?;
        eprintln!("mapa refeito: {}", project.display());
    }
    Ok(projects)
}

/// O que a prova diz do gancho das sessões do usuário: o commit que o
/// `mustard-rt` do plugin instalado (o que o registro do Claude Code aponta)
/// diz em `--version`, com `-dirty` se foi compilado com código por comitar;
/// `não instalado` sem plugin registrado ou sem o programa dele no disco; e
/// `sem commit na versão` quando há plugin, mas ele não diz o commit.
fn hook_version(host: &Host) -> String {
    let installed = match &host.config {
        Some(config) => installed_plugin_rt_in(config),
        None => installed_plugin_rt(),
    };
    let Some(binary) = installed else {
        return HOOK_NOT_INSTALLED.to_string();
    };
    Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| built_commit(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_else(|| HOOK_WITHOUT_COMMIT.to_string())
}

// ---------------------------------------------------------------------------
// Cargo e a régua
// ---------------------------------------------------------------------------

/// O programa do cargo: o do `host`, ou o que o ambiente diz em `CARGO`, o do
/// `PATH` ou o de `~/.cargo/bin`.
fn cargo_program(host: &Host) -> Result<PathBuf, String> {
    if let Some(cargo) = &host.cargo {
        return Ok(cargo.clone());
    }
    let file = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    let from_env = std::env::var_os("CARGO").map(PathBuf::from).filter(|path| path.is_file());
    let from_path = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(file)).find(|path| path.is_file()));
    let from_home = home_dir().map(|home| home.join(".cargo").join("bin").join(file)).filter(|path| path.is_file());
    from_env.or(from_path).or(from_home).ok_or_else(|| "não achei o cargo no ambiente: nem em CARGO, nem no PATH, nem em ~/.cargo/bin".to_string())
}

/// Um comando do cargo na pasta do código, com a compilação nesta pasta de medida.
fn cargo_in(host: &Host, source: &Path, code: &Code, target: &Path) -> Result<Command, String> {
    let mut command = Command::new(cargo_program(host)?);
    command.current_dir(source).stdin(Stdio::null());
    for (name, value) in build_env(code, target) {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    Ok(command)
}

/// Roda o cargo até o fim com a conversa dele na saída de erro; o que ele
/// escreve na saída comum é o JSON, que `capture` pede de volta.
fn run_cargo(mut command: Command, capture: bool) -> Result<String, String> {
    command.stderr(Stdio::inherit()).stdout(if capture { Stdio::piped() } else { Stdio::null() });
    let output = command.output().map_err(|err| format!("não consegui rodar o cargo: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("o cargo falhou ({}); a conversa dele está acima", output.status))
    }
}

/// Lê um fluxo do programa de teste linha a linha: repete cada uma na saída de
/// erro, para o usuário acompanhar, e guarda as de prova e as das peças da
/// busca.
fn echo_and_collect(stream: impl Read, proof: &Mutex<Vec<String>>) {
    for raw in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
        eprintln!("{line}");
        if is_proof_line(&line) || is_pieces_line(&line) {
            proof.lock().unwrap_or_else(PoisonError::into_inner).push(line);
        }
    }
}

/// Roda o programa de teste e devolve se terminou bem e as linhas de prova e de
/// peças que imprimiu, em qualquer das duas saídas.
fn run_ruler(mut command: Command) -> Result<(bool, Vec<String>), String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| format!("não consegui rodar a régua: {err}"))?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        return Err("não consegui ler a saída da régua".to_string());
    };
    let proof = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        scope.spawn(|| echo_and_collect(out, &proof));
        scope.spawn(|| echo_and_collect(err, &proof));
    });
    let status = child.wait().map_err(|err| format!("não consegui esperar a régua: {err}"))?;
    Ok((status.success(), proof.into_inner().unwrap_or_else(PoisonError::into_inner)))
}

/// O que uma medida compila e roda: o pedido, o código, as pastas e as
/// variáveis do usuário.
struct Job<'a> {
    opts: &'a MeasureOpts,
    host: &'a Host,
    source: &'a Path,
    code: &'a Code,
    folder: &'a Path,
    package: &'a str,
    out: &'a Path,
    user: &'a [(String, String)],
    trees: Option<&'a Path>,
}

/// O `scan` que o cargo compilou em `target`, ao lado do `mustard-rt` do
/// mesmo código: achado como o programa acha o dele ([`Scan::located_from`]),
/// e nunca o do `PATH`, que seria de outra compilação.
fn compiled_scan(target: &Path) -> Result<Scan, String> {
    let rt = target.join("release").join(if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" });
    let scan = Scan::located_from(Some(&rt));
    if scan.is_compiled_alongside() {
        Ok(scan)
    } else {
        Err(format!("o cargo não deixou o scan ao lado de {}", rt.display()))
    }
}

/// Compila o código, refaz o mapa de cada árvore e roda a régua; devolve as
/// linhas de prova.
fn compile_and_run(job: &Job<'_>) -> Result<Vec<String>, String> {
    let Job { opts, host, source, code, folder, package, out, user, trees } = job;
    let target = folder.join("target");

    // O `scan` e o `mustard-rt` ficam ao lado do programa de teste: a régua
    // confere o mapa pela marca do scan compilado com este mesmo código.
    let mut build = cargo_in(host, source, code, &target)?;
    build.args(["build", "--release", "--locked", "-p", "scan", "-p", "mustard-rt"]);
    run_cargo(build, false)?;

    // Cada mapa se refaz com esse mesmo scan antes da régua: nenhuma medida
    // abre o mapa que uma compilação velha deixou na árvore.
    if let Some(trees) = trees {
        rebuild_trees(trees, &compiled_scan(&target)?)?;
    }

    let mut compile = cargo_in(host, source, code, &target)?;
    compile.args(["test", "--release", "--locked", "--no-run", "--lib", "--message-format=json", "-p", package]);
    let binary = find_test_binary(&run_cargo(compile, true)?, package)?;

    let list = Command::new(&binary.executable)
        .args(["--list", "--ignored"])
        .current_dir(&binary.manifest_dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("não consegui listar os testes de {}: {err}", binary.executable.display()))?;
    let name = pick_test(&String::from_utf8_lossy(&list.stdout), &opts.test)?;

    let mut ruler = Command::new(&binary.executable);
    ruler.args([name.as_str(), "--exact", "--ignored", "--nocapture"]).current_dir(&binary.manifest_dir);
    ruler.envs(run_env(code, &hook_version(host), out, user, &binary.manifest_dir));
    eprintln!("medindo `{name}` com {}", binary.executable.display());
    let (passed, proof) = run_ruler(ruler)?;
    if !passed {
        return Err(format!("a régua `{name}` falhou; a conversa dela está acima"));
    }
    if lacks_proof_line(&proof) {
        return Err(format!("a régua `{name}` terminou sem imprimir a linha `{}…`: ela não grava a versão que usou, e o número não vale", PROOF_PREFIX.trim()));
    }
    Ok(proof)
}

/// O pedido inteiro, a partir de uma pasta: confere, compila, mede e apaga as
/// pastas de medida velhas. Devolve o que imprimir ao fim.
fn measure_in(cwd: &Path, base: &Path, opts: &MeasureOpts, host: &Host) -> Result<String, String> {
    let mut user = parse_env(&opts.env)?;
    let root = repo_root(cwd);
    if !is_mustard_source(&root) {
        return Err("este comando mede o código-fonte do Mustard; esta pasta não é o repositório dele, e não há o que compilar".to_string());
    }
    let package = match &opts.package {
        Some(package) => package.clone(),
        None => package_of_test(&root, &opts.test)?,
    };
    let trees = trees_folder(cwd, opts, &mut user)?;
    let code = match &opts.commit {
        Some(rev) => commit_code(&root, rev)?,
        None => current_code(&root)?,
    };
    let folder = base.join(folder_name(&code.sha, &code.diff));
    mark_used(&folder)?;
    let out = match &opts.out {
        Some(out) if out.is_absolute() => out.clone(),
        Some(out) => cwd.join(out),
        None => folder.join(format!("{}.json", opts.test)),
    };

    let measured = (|| {
        let source = if opts.commit.is_some() { materialize(&root, &code.sha, &folder)? } else { root.clone() };
        compile_and_run(&Job { opts, host, source: &source, code: &code, folder: &folder, package: &package, out: &out, user: &user, trees: trees.as_deref() })
    })();
    // A faxina vai mesmo quando a medida falha: o disco é o mesmo.
    let removed = prune(base, &folder, &root);
    let proof = measured?;
    Ok(summary(&proof, &out, out.is_file(), &removed))
}

/// `mustard-rt run measure <teste>`: o ponto de entrada.
pub fn run(opts: &MeasureOpts) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let outcome = measure_base().and_then(|base| measure_in(&cwd, &base, opts, &Host::default()));
    match outcome {
        Ok(report) => println!("{report}"),
        Err(refusal) => {
            eprintln!("measure: {refusal}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::measure_proof::MeasureProof;
    use std::time::Duration;
    use tempfile::tempdir;

    fn code(sha: &str, diff: &str) -> Code {
        Code { sha: sha.to_string(), dirty: !diff.is_empty(), diff: diff.to_string(), date: Some("2026-09-30".to_string()) }
    }

    #[test]
    fn the_same_clean_commit_has_the_same_folder() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(folder_name(sha, ""), folder_name(sha, ""));
        assert_eq!(folder_name(sha, ""), "0123456789ab");
        // O commit inteiro e o curto do mesmo commit nomeiam a mesma pasta.
        assert_eq!(folder_name(sha, ""), folder_name(&sha[..12], ""));
    }

    #[test]
    fn the_same_commit_with_another_diff_has_another_folder() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let first = folder_name(sha, "aaaaaaaaaaaa");
        assert_ne!(first, folder_name(sha, "bbbbbbbbbbbb"));
        assert_ne!(first, folder_name(sha, ""));
        assert_eq!(first, "0123456789ab-sujo-aaaaaaaaaaaa");
    }

    #[test]
    fn another_commit_has_another_folder() {
        assert_ne!(folder_name("0123456789abcdef", ""), folder_name("fedcba9876543210", ""));
        assert_ne!(folder_name("0123456789abcdef", "aaaaaaaaaaaa"), folder_name("fedcba9876543210", "aaaaaaaaaaaa"));
    }

    fn artifact(package_id: &str, kind: &str, test: bool, executable: Option<&str>) -> String {
        let executable = executable.map_or("null".to_string(), |path| format!("\"{path}\""));
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"{package_id}","manifest_path":"/w/apps/rt/Cargo.toml","target":{{"kind":["{kind}"],"name":"x"}},"profile":{{"test":{test}}},"executable":{executable}}}"#
        )
    }

    #[test]
    fn the_test_binary_is_the_executable_of_the_package_asked() {
        let json = [
            r#"{"reason":"compiler-message","message":"aviso"}"#.to_string(),
            artifact("scan 0.1.0 (path+file:///w/apps/scan)", "lib", true, Some("/t/deps/scan-1")),
            artifact("mustard-rt 0.1.0 (path+file:///w/apps/rt)", "bin", false, Some("/t/mustard-rt")),
            artifact("mustard-rt 0.1.0 (path+file:///w/apps/rt)", "test", true, Some("/t/deps/integracao-2")),
            artifact("mustard-rt 0.1.0 (path+file:///w/apps/rt)", "lib", true, Some("/t/deps/mustard_rt-3")),
            "linha que nem é JSON".to_string(),
        ]
        .join("\n");
        let found = find_test_binary(&json, "mustard-rt").unwrap();
        assert_eq!(found, TestBinary { executable: PathBuf::from("/t/deps/mustard_rt-3"), manifest_dir: PathBuf::from("/w/apps/rt") });
    }

    #[test]
    fn the_package_name_comes_from_both_forms_of_the_cargo_id() {
        assert_eq!(package_name_of_id("mustard-rt 0.1.0 (path+file:///w/apps/rt)"), Some("mustard-rt"));
        assert_eq!(package_name_of_id("path+file:///w/apps/rt#mustard-rt@0.1.0"), Some("mustard-rt"));
        assert_eq!(package_name_of_id("path+file:///w/apps/scan#0.1.0"), Some("scan"));
        let new_form = artifact("path+file:///w/apps/rt#mustard-rt@0.1.0", "lib", true, Some("/t/deps/mustard_rt-3"));
        assert!(find_test_binary(&new_form, "mustard-rt").is_ok());
        assert!(find_test_binary(&new_form, "scan").is_err());
    }

    #[test]
    fn two_test_binaries_or_none_are_refused() {
        let id = "mustard-rt 0.1.0 (path+file:///w/apps/rt)";
        let two = [artifact(id, "lib", true, Some("/t/deps/a-1")), artifact(id, "bin", true, Some("/t/deps/b-2"))].join("\n");
        let refusal = find_test_binary(&two, "mustard-rt").unwrap_err();
        assert!(refusal.contains("2 programas de teste"), "{refusal}");
        // O mesmo programa dito duas vezes é um só.
        let same = [artifact(id, "lib", true, Some("/t/deps/a-1")), artifact(id, "lib", true, Some("/t/deps/a-1"))].join("\n");
        assert!(find_test_binary(&same, "mustard-rt").is_ok());

        let none = [artifact(id, "bin", false, Some("/t/mustard-rt")), artifact(id, "lib", true, None)].join("\n");
        assert!(find_test_binary(&none, "mustard-rt").unwrap_err().contains("não compilou programa de teste"));
        assert!(find_test_binary("", "mustard-rt").is_err());
    }

    #[test]
    fn the_ruler_is_the_only_ignored_test_with_that_name() {
        let list = "mod_a::tests::other: test\nshared::ruler::measure_the_spend: test\nshared::ruler::measure_the_spend_again: test\n";
        assert_eq!(pick_test(list, "measure_the_spend").unwrap(), "shared::ruler::measure_the_spend");
        assert!(pick_test(list, "missing").unwrap_err().contains("não tem teste ignorado"));
        let twice = "a::measure_it: test\nb::measure_it: test\n";
        assert!(pick_test(twice, "measure_it").unwrap_err().contains("2 testes ignorados"));
        // Um teste comum, que `--list --ignored` não lista, não é régua.
        assert!(pick_test("", "measure_it").is_err());
    }

    fn entry(base: &Path, name: &str, age_secs: u64) -> (PathBuf, SystemTime) {
        (base.join(name), SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_secs))
    }

    #[test]
    fn the_three_most_recent_folders_stay_and_the_rest_go() {
        let base = Path::new("/c/medida");
        let entries = vec![entry(base, "d", 400), entry(base, "a", 10), entry(base, "c", 300), entry(base, "b", 20), entry(base, "e", 500)];
        let doomed = pick_to_remove(&entries, 3, base, &base.join("a"));
        assert_eq!(doomed, vec![base.join("d"), base.join("e")]);
    }

    #[test]
    fn nothing_is_removed_when_the_folders_fit() {
        let base = Path::new("/c/medida");
        let entries = vec![entry(base, "a", 10), entry(base, "b", 20), entry(base, "c", 30)];
        assert!(pick_to_remove(&entries, 3, base, &base.join("a")).is_empty());
        assert!(pick_to_remove(&[], 3, base, &base.join("a")).is_empty());
    }

    #[test]
    fn the_folder_in_use_stays_even_with_the_oldest_time() {
        let base = Path::new("/c/medida");
        let entries = vec![entry(base, "velha", 900), entry(base, "a", 10), entry(base, "b", 20), entry(base, "c", 30)];
        let doomed = pick_to_remove(&entries, 3, base, &base.join("velha"));
        assert_eq!(doomed, vec![base.join("c")]);
    }

    #[test]
    fn nothing_outside_the_measure_folder_is_ever_removed() {
        let base = Path::new("/c/medida");
        let entries = vec![
            entry(base, "a", 10),
            (PathBuf::from("/c/outra/b"), SystemTime::UNIX_EPOCH),
            (PathBuf::from("/c/medida/a/fundo"), SystemTime::UNIX_EPOCH),
            (base.to_path_buf(), SystemTime::UNIX_EPOCH),
            (base.join(".."), SystemTime::UNIX_EPOCH),
            (PathBuf::from("/"), SystemTime::UNIX_EPOCH),
        ];
        // Com `keep` em 1, tudo o que passa do primeiro iria embora.
        let doomed = pick_to_remove(&entries, 1, base, &base.join("a"));
        assert!(doomed.is_empty(), "{doomed:?}");
    }

    #[test]
    fn only_folders_the_command_marked_count_as_measure_folders() {
        let base = tempdir().unwrap();
        mark_used(&base.path().join("minha")).unwrap();
        std::fs::create_dir_all(base.path().join("alheia")).unwrap();
        std::fs::write(base.path().join("arquivo"), "x").unwrap();
        let listed: Vec<PathBuf> = used_folders(base.path()).into_iter().map(|(path, _)| path).collect();
        assert_eq!(listed, vec![base.path().join("minha")]);
    }

    #[test]
    fn pruning_deletes_the_old_folders_and_leaves_the_others_alone() {
        let base = tempdir().unwrap();
        for (at, name) in ["um", "dois", "tres", "quatro", "cinco"].iter().enumerate() {
            let folder = base.path().join(name);
            mark_used(&folder).unwrap();
            std::fs::write(folder.join("target"), "x").unwrap();
            let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + at as u64);
            std::fs::File::options().write(true).open(folder.join(USED_MARK)).unwrap().set_modified(when).unwrap();
        }
        std::fs::create_dir_all(base.path().join("alheia")).unwrap();
        let removed = prune(base.path(), &base.path().join("cinco"), base.path());
        let mut left: Vec<String> =
            std::fs::read_dir(base.path()).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["alheia", "cinco", "quatro", "tres"]);
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn user_variables_are_pairs_and_never_the_ones_of_the_proof() {
        let pairs = parse_env(&["A=1".to_string(), "B=x=y".to_string(), "C=".to_string()]).unwrap();
        assert_eq!(pairs, vec![("A".into(), "1".into()), ("B".into(), "x=y".into()), ("C".into(), String::new())]);
        for bad in ["SEMIGUAL", "=v", "A B=1", "MUSTARD_MEASURE_COMMIT=abc", "MUSTARD_MEASURE_OUT=/x", "MUSTARD_MEASURE_DIRTY=0"] {
            assert!(parse_env(&[bad.to_string()]).is_err(), "{bad} devia ser recusada");
        }
    }

    /// As variáveis que o comando põe são exatamente as que a prova lê, no
    /// código sujo e no limpo.
    #[test]
    fn the_variables_the_command_sets_are_the_ones_the_proof_reads() {
        let exe = std::env::current_exe().unwrap();
        for (diff, dirty) in [("abcdef012345", true), ("", false)] {
            let code = code("0123456789abcdef0123456789abcdef01234567", diff);
            let vars = run_env(&code, "10d66039a5b1", Path::new("/o/saida.json"), &[("K".into(), "v".into())], Path::new("/w/apps/rt"));
            let read = |name: &str| vars.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
            let proof = MeasureProof::from_vars(&read, &exe).unwrap();
            assert_eq!(proof.commit, "0123456789ab");
            assert_eq!(proof.dirty, dirty);
            assert_eq!(proof.diff, diff);
            assert_eq!(proof.hook, "10d66039a5b1");
            assert_eq!(read("MUSTARD_MEASURE_OUT").as_deref(), Some("/o/saida.json"));
            assert_eq!(read("K").as_deref(), Some("v"));
            assert_eq!(read("CARGO_MANIFEST_DIR").as_deref(), Some("/w/apps/rt"));
        }
    }

    /// O carimbo do programa diz "sujo" pela presença da variável: no código
    /// limpo ela sai do ambiente herdado, e no sujo ela vai com o resumo.
    #[test]
    fn the_build_stamps_dirty_by_the_presence_of_the_variable() {
        let get = |env: &[(&'static str, Option<String>)], name: &str| env.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
        let dirty = build_env(&code("0123456789abcdef", "abcdef012345"), Path::new("/m/x/target"));
        assert_eq!(get(&dirty, "MUSTARD_GIT_DIRTY"), Some(Some("1".to_string())));
        assert_eq!(get(&dirty, "MUSTARD_GIT_DIFF"), Some(Some("abcdef012345".to_string())));
        assert_eq!(get(&dirty, "MUSTARD_GIT_HASH"), Some(Some("0123456789ab".to_string())));
        assert_eq!(get(&dirty, "CARGO_TARGET_DIR"), Some(Some("/m/x/target".to_string())));

        let clean = build_env(&code("0123456789abcdef", ""), Path::new("/m/x/target"));
        assert_eq!(get(&clean, "MUSTARD_GIT_DIRTY"), Some(None), "limpo tira a variável do ambiente herdado");
        assert_eq!(get(&clean, "MUSTARD_GIT_DIFF"), Some(Some(String::new())));
    }

    #[test]
    fn a_manifest_names_its_package_and_only_the_package() {
        let manifest = "[workspace]\nname = \"outro\"\n\n[package]\nname = \"mustard-rt\" # o pacote\nversion = \"1\"\n\n[[bin]]\nname = \"bin\"\n";
        assert_eq!(package_name_in(manifest).as_deref(), Some("mustard-rt"));
        assert_eq!(package_name_in("[lib]\nname = \"x\"\n"), None);
    }

    /// Um repositório de mentira com os três pacotes do Mustard.
    fn fake_source(root: &Path, with_ruler_in: &[(&str, &str)]) {
        for (dir, name) in [("apps/rt", "mustard-rt"), ("apps/scan", "scan"), ("packages/core", "mustard-core"), ("apps/cli", "mustard-cli")] {
            std::fs::create_dir_all(root.join(dir).join("src")).unwrap();
            std::fs::write(root.join(dir).join("Cargo.toml"), format!("[package]\nname = \"{name}\"\n")).unwrap();
            std::fs::write(root.join(dir).join("src").join("lib.rs"), "fn other() {}\n").unwrap();
        }
        for (dir, test) in with_ruler_in {
            std::fs::write(root.join(dir).join("src").join(format!("ruler_{test}.rs")), format!("#[ignore]\nfn {test}() {{}}\n")).unwrap();
        }
    }

    #[test]
    fn a_folder_with_the_three_mustard_packages_is_the_mustard_source() {
        let dir = tempdir().unwrap();
        assert!(!is_mustard_source(dir.path()));
        fake_source(dir.path(), &[]);
        assert!(is_mustard_source(dir.path()));
        std::fs::write(dir.path().join("apps/scan/Cargo.toml"), "[package]\nname = \"outro\"\n").unwrap();
        assert!(!is_mustard_source(dir.path()));
    }

    #[test]
    fn the_package_of_the_ruler_comes_from_the_code_or_is_refused() {
        let dir = tempdir().unwrap();
        fake_source(dir.path(), &[("packages/core", "measure_it"), ("apps/rt", "measure_both"), ("packages/core", "measure_both")]);
        assert_eq!(package_of_test(dir.path(), "measure_it").unwrap(), "mustard-core");
        assert!(package_of_test(dir.path(), "measure_none").unwrap_err().contains("não achei a régua"));
        // O mesmo nome em dois pacotes não se escolhe sozinho.
        let refusal = package_of_test(dir.path(), "measure_both").unwrap_err();
        assert!(refusal.contains("mais de um pacote") && refusal.contains("mustard-core") && refusal.contains("mustard-rt"), "{refusal}");
    }

    fn opts(test: &str) -> MeasureOpts {
        MeasureOpts { test: test.to_string(), commit: None, package: None, env: Vec::new(), out: None, trees: None }
    }

    #[test]
    fn a_folder_that_is_not_the_mustard_source_is_refused() {
        let base = tempdir().unwrap();
        // Uma pasta qualquer e uma que é repositório de outro projeto.
        let plain = tempdir().unwrap();
        let other = tempdir().unwrap();
        assert!(git::run(other.path(), &["init", "-q"]).ok);
        std::fs::write(other.path().join("Cargo.toml"), "[package]\nname = \"outro\"\n").unwrap();
        for dir in [plain.path(), other.path()] {
            let refusal = measure_in(dir, base.path(), &opts("measure_it"), &Host::default()).unwrap_err();
            assert!(refusal.contains("código-fonte do Mustard"), "{refusal}");
        }
        assert!(std::fs::read_dir(base.path()).unwrap().next().is_none(), "recusar não cria pasta de medida");
    }

    #[test]
    fn a_reserved_variable_is_refused_before_anything_is_built() {
        let base = tempdir().unwrap();
        let mut asked = opts("measure_it");
        asked.env = vec!["MUSTARD_MEASURE_COMMIT=abc".to_string()];
        let refusal = measure_in(base.path(), base.path(), &asked, &Host::default()).unwrap_err();
        assert!(refusal.contains("posta pelo comando"), "{refusal}");
    }

    #[test]
    fn the_summary_lists_the_proof_lines_the_result_and_what_was_removed() {
        let proof = vec![
            "PROVA commit=0123456789ab sujo=nao diff=- sha=aaaaaaaaaaaa mapas=1".to_string(),
            "PECAS /m/a: compilado=ligada historico=ainda-nao-ligada".to_string(),
        ];
        let text = summary(&proof, Path::new("/o/saida.json"), true, &[PathBuf::from("/m/velha")]);
        assert_eq!(
            text,
            "PROVA commit=0123456789ab sujo=nao diff=- sha=aaaaaaaaaaaa mapas=1\nPECAS /m/a: compilado=ligada historico=ainda-nao-ligada\nresultado: /o/saida.json (gravado)\napagada: /m/velha"
        );
        assert!(!lacks_proof_line(&proof));
        assert!(lacks_proof_line(&proof[1..]), "the pieces line alone does not prove the version");
        assert!(lacks_proof_line(&[]));
        assert!(is_pieces_line("PECAS /m/a: compilado=ligada"));
        assert!(!is_pieces_line("PECAS-JSON {}"));
        assert!(!is_proof_line("PECAS /m/a: compilado=ligada"), "the pieces line is not the proof line");
        assert!(summary(&proof, Path::new("/o/x.json"), false, &[]).contains("a régua não gravou o arquivo"));
        assert!(is_proof_line("PROVA commit=1"));
        assert!(!is_proof_line("PROVA-JSON {}"));
        assert!(!is_proof_line("outra coisa"));
    }

    /// A régua que imprime a linha de prova numa saída e a das peças na outra
    /// tem as duas colhidas, e as outras linhas ficam de fora.
    #[cfg(unix)]
    #[test]
    fn the_ruler_run_collects_the_proof_and_the_pieces_lines_from_both_outputs() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo 'conversa qualquer'; echo 'PROVA commit=0123456789ab'; echo 'PECAS /m/a: compilado=ligada' >&2"]);
        let (passed, lines) = run_ruler(command).expect("the ruler runs");
        assert!(passed);
        let mut lines = lines;
        lines.sort();
        assert_eq!(lines, ["PECAS /m/a: compilado=ligada", "PROVA commit=0123456789ab"]);
    }
    // -----------------------------------------------------------------------
    // Mapas refeitos e a versão do gancho
    // -----------------------------------------------------------------------

    /// Registra, no registro de plugins da pasta de configuração `config`, uma
    /// instalação do plugin do mercado `market` na versão `version`, na pasta
    /// que a função devolve (sem programa dentro).
    #[cfg(unix)]
    fn register_plugin(config: &Path, market: &str, version: &str) -> PathBuf {
        let install = config.join("plugins").join("cache").join(market).join("mustard").join(version);
        std::fs::create_dir_all(install.join("bin")).unwrap();
        let registry = config.join("plugins").join("installed_plugins.json");
        let mut doc: Value = std::fs::read_to_string(&registry).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_else(|| serde_json::json!({ "version": 2, "plugins": {} }));
        let record = serde_json::json!({ "scope": "user", "installPath": install, "version": version });
        let records = doc["plugins"].as_object_mut().unwrap().entry(format!("mustard@{market}")).or_insert_with(|| serde_json::json!([]));
        records.as_array_mut().unwrap().push(record);
        std::fs::write(&registry, doc.to_string()).unwrap();
        install
    }

    /// Instala um plugin do mercado `market` na versão `version`, cujo
    /// `mustard-rt` diz `says` em `--version`.
    #[cfg(unix)]
    fn plugin_with(config: &Path, market: &str, version: &str, says: &str) {
        let install = register_plugin(config, market, version);
        crate::executable::write_executable(&install.join("bin").join("mustard-rt"), &format!("#!/bin/sh\necho '{says}'\n"));
    }

    fn host_with_config(config: &Path) -> Host {
        Host { cargo: None, config: Some(config.to_path_buf()) }
    }

    /// O gancho é o commit que o `mustard-rt` da instalação mais nova do
    /// registro diz: `0.2.10` vence `0.2.4`, que a ordem do texto daria ao
    /// contrário, e o `-dirty` do carimbo fica.
    #[cfg(unix)]
    #[test]
    fn the_hook_is_the_commit_stamped_in_the_newest_registered_plugin_version() {
        let config = tempdir().unwrap();
        plugin_with(config.path(), "mustard-local", "0.2.4", "mustard-rt 0.2.4 (build dev, gaaaaaaaaaaaa 2026-09-25)");
        plugin_with(config.path(), "outro-mercado", "0.2.10", "mustard-rt 0.2.10 (build dev, gbbbbbbbbbbbb-dirty 2026-09-30)");
        assert_eq!(hook_version(&host_with_config(config.path())), "bbbbbbbbbbbb-dirty");
    }

    /// Sem plugin registrado, ou com o registro apontando para uma pasta sem o
    /// programa, o gancho é `não instalado`; com plugin que não carimba o
    /// commit, a prova diz isso e não inventa um.
    #[cfg(unix)]
    #[test]
    fn without_a_plugin_the_hook_is_not_installed_and_a_version_without_commit_says_so() {
        let config = tempdir().unwrap();
        assert_eq!(hook_version(&host_with_config(config.path())), "não instalado");
        // O registro aponta para uma instalação sem o programa dentro.
        register_plugin(config.path(), "mustard-local", "0.2.4");
        assert_eq!(hook_version(&host_with_config(config.path())), "não instalado");

        plugin_with(config.path(), "mustard-local", "0.2.4", "mustard-rt 0.2.4");
        assert_eq!(hook_version(&host_with_config(config.path())), "sem commit na versão");
    }

    /// O gancho vem do programa que o leitor do instalador aponta
    /// ([`installed_plugin_rt_in`]): o que roda em `--version` é o mesmo
    /// caminho que ele devolve, e onde ele não devolve nada o gancho não está
    /// instalado.
    #[cfg(unix)]
    #[test]
    fn the_hook_runs_the_program_the_installer_reader_points_to() {
        let config = tempdir().unwrap();
        let ran = config.path().join("quem-rodou");
        assert_eq!((installed_plugin_rt_in(config.path()), hook_version(&host_with_config(config.path()))), (None, "não instalado".to_string()));

        register_plugin(config.path(), "mustard-local", "0.2.4");
        assert_eq!((installed_plugin_rt_in(config.path()), hook_version(&host_with_config(config.path()))), (None, "não instalado".to_string()), "the registry without the program");

        let newest = register_plugin(config.path(), "outro-mercado", "0.2.10");
        crate::executable::write_executable(
            &newest.join("bin").join("mustard-rt"),
            &format!("#!/bin/sh\necho \"$0\" > '{}'\necho 'mustard-rt 0.2.10 (build dev, geeeeeeeeeeee 2026-09-30)'\n", ran.display()),
        );
        let pointed = installed_plugin_rt_in(config.path()).expect("the reader finds the program");
        assert_eq!(hook_version(&host_with_config(config.path())), "eeeeeeeeeeee");
        assert_eq!(std::fs::read_to_string(&ran).unwrap().trim(), pointed.display().to_string(), "the hook ran the program the reader points to");
    }

    /// Um programa na pasta de cache que o registro não aponta não é o gancho
    /// das sessões: só vale o que o registro diz.
    #[cfg(unix)]
    #[test]
    fn a_program_in_the_cache_that_the_registry_does_not_point_to_is_not_the_hook() {
        let config = tempdir().unwrap();
        let loose = config.path().join("plugins/cache/mustard-local/mustard/0.9.0/bin");
        std::fs::create_dir_all(&loose).unwrap();
        crate::executable::write_executable(&loose.join("mustard-rt"), "#!/bin/sh\necho 'mustard-rt 0.9.0 (build dev, gcccccccccccc 2026-09-30)'\n");
        assert_eq!(hook_version(&host_with_config(config.path())), "não instalado");
    }

    /// O scan que refaz o mapa é o que o cargo deixou ao lado do `mustard-rt`
    /// da compilação; sem ele a medida recusa, e nunca roda o do `PATH`, que
    /// seria de outra compilação.
    #[cfg(unix)]
    #[test]
    fn the_scan_that_rebuilds_the_map_is_the_one_built_beside_the_program_and_never_the_one_on_the_path() {
        let target = tempdir().unwrap();
        let refusal = compiled_scan(target.path()).unwrap_err();
        assert!(refusal.contains("o cargo não deixou o scan"), "{refusal}");

        let release = target.path().join("release");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("scan"), "").unwrap();
        let scan = compiled_scan(target.path()).expect("the scan beside the program is found");
        assert!(scan.is_compiled_alongside());
        assert_eq!(format!("{scan:?}"), format!("{:?}", Scan::new(release.join("scan").to_string_lossy())));
    }

    /// A pasta de árvores vem de `--trees` ou de `SPEND_TREES` em `--env`, e
    /// vai à régua por caminho absoluto, no lugar do que veio: a régua roda de
    /// outra pasta, e o mapa refeito tem de ser o que ela abre.
    #[test]
    fn the_trees_folder_comes_from_the_flag_or_the_variable_and_reaches_the_ruler_as_an_absolute_path() {
        let cwd = tempdir().unwrap();
        std::fs::create_dir_all(cwd.path().join("arvores")).unwrap();

        let mut asked = opts("measure_it");
        asked.trees = Some(PathBuf::from("arvores"));
        let mut user = Vec::new();
        assert_eq!(trees_folder(cwd.path(), &asked, &mut user).unwrap(), Some(cwd.path().join("arvores")));
        assert_eq!(user, vec![("SPEND_TREES".to_string(), cwd.path().join("arvores").display().to_string())]);

        let mut user = vec![("SPEND_TREES".to_string(), "arvores".to_string()), ("K".to_string(), "v".to_string())];
        assert_eq!(trees_folder(cwd.path(), &opts("measure_it"), &mut user).unwrap(), Some(cwd.path().join("arvores")));
        assert_eq!(user.iter().filter(|(key, _)| key == "SPEND_TREES").count(), 1);
        assert!(user.contains(&("SPEND_TREES".to_string(), cwd.path().join("arvores").display().to_string())));
        assert!(user.contains(&("K".to_string(), "v".to_string())));

        assert_eq!(trees_folder(cwd.path(), &opts("measure_it"), &mut Vec::new()).unwrap(), None);

        let mut user = vec![("SPEND_TREES".to_string(), "outras".to_string())];
        let refusal = trees_folder(cwd.path(), &asked, &mut user).unwrap_err();
        assert!(refusal.contains("pastas diferentes"), "{refusal}");

        asked.trees = Some(PathBuf::from("nao-existe"));
        let refusal = trees_folder(cwd.path(), &asked, &mut Vec::new()).unwrap_err();
        assert!(refusal.contains("não existe"), "{refusal}");
    }

    /// Um scan de mentira: anota em `log` cada chamada, com o comando e a
    /// árvore; no comando `scan` grava `novo` no banco que recebe em `--out` e
    /// falha quando o arquivo `fail` existe; a leitura da história
    /// (`history-all`) só é anotada.
    #[cfg(unix)]
    fn fake_scan(path: &Path, log: &Path, fail: &Path) {
        crate::executable::write_executable(
            path,
            &format!(
                "#!/bin/sh\necho \"$1 $2\" >> '{log}'\n[ \"$1\" = history-all ] && exit 0\n[ -e '{fail}' ] && {{ echo 'o scan quebrou' >&2; exit 3; }}\nmkdir -p \"$(dirname \"$4\")\" && printf novo > \"$4\"\necho '{{\"full\":true,\"read\":[],\"files\":1}}'\n",
                log = log.display(),
                fail = fail.display()
            ),
        );
    }

    /// As linhas que `fake_scan` anotou em `log`. A medida só volta depois de a
    /// leitura da história terminar, então todas já estão lá: sem esperar.
    #[cfg(unix)]
    fn scan_calls(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// Uma pasta de árvores com os projetos `names`, cada um com o mapa velho.
    #[cfg(unix)]
    fn trees_with(dir: &Path, names: &[&str]) -> PathBuf {
        let trees = dir.join("arvores");
        for name in names {
            let claude = trees.join(name).join(".claude");
            std::fs::create_dir_all(&claude).unwrap();
            std::fs::write(claude.join("grain.db"), "velho").unwrap();
            std::fs::write(claude.join("grain.db-wal"), "velho").unwrap();
        }
        trees
    }

    /// Cada projeto da pasta de árvores, em ordem, perde o banco velho e ganha
    /// o do scan; as pastas escondidas e os arquivos soltos não são projeto.
    #[cfg(unix)]
    #[test]
    fn every_project_of_the_trees_gets_a_new_map_and_the_hidden_folders_are_left_alone() {
        let dir = tempdir().unwrap();
        let (log, fail) = (dir.path().join("log"), dir.path().join("fail"));
        fake_scan(&dir.path().join("scan"), &log, &fail);
        let trees = trees_with(dir.path(), &["b", "a", ".escondida"]);
        std::fs::write(trees.join("solto.txt"), "x").unwrap();

        let rebuilt = rebuild_trees(&trees, &Scan::new(dir.path().join("scan").to_string_lossy())).unwrap();

        assert_eq!(rebuilt, vec![trees.join("a"), trees.join("b")]);
        let calls = scan_calls(&log);
        let (a, b) = (trees.join("a").display().to_string(), trees.join("b").display().to_string());
        let of = |command: &str| {
            let mut found: Vec<String> = calls.iter().filter(|call| call.starts_with(command)).cloned().collect();
            found.sort();
            found
        };
        assert_eq!(of("scan "), [format!("scan {a}"), format!("scan {b}")]);
        assert_eq!(of("history-all "), [format!("history-all {a}"), format!("history-all {b}")], "the history of each map is read after its scan, as in the sessions: {calls:?}");
        for name in ["a", "b"] {
            let claude = trees.join(name).join(".claude");
            assert_eq!(std::fs::read_to_string(claude.join("grain.db")).unwrap(), "novo");
            assert!(!claude.join("grain.db-wal").exists(), "the side file of the old database goes too");
        }
        assert_eq!(std::fs::read_to_string(trees.join(".escondida/.claude/grain.db")).unwrap(), "velho");
    }

    /// O scan que falha num projeto recusa, dizendo qual; uma pasta sem
    /// projeto também recusa, porque a régua mediria sem mapa novo.
    #[cfg(unix)]
    #[test]
    fn a_scan_that_fails_or_a_folder_without_projects_refuses() {
        let dir = tempdir().unwrap();
        let (log, fail) = (dir.path().join("log"), dir.path().join("fail"));
        fake_scan(&dir.path().join("scan"), &log, &fail);
        let scan = Scan::new(dir.path().join("scan").to_string_lossy());
        let trees = trees_with(dir.path(), &["a"]);
        std::fs::write(&fail, "").unwrap();

        let refusal = rebuild_trees(&trees, &scan).unwrap_err();
        assert!(refusal.contains("o scan não refez o mapa") && refusal.contains(&trees.join("a").display().to_string()), "{refusal}");
        assert!(refusal.contains("o scan quebrou"), "{refusal}");

        let empty = tempdir().unwrap();
        let refusal = rebuild_trees(empty.path(), &scan).unwrap_err();
        assert!(refusal.contains("nenhum"), "{refusal}");
    }

    /// Um mundo de mentira para a medida inteira: o repositório do Mustard com
    /// um commit, uma pasta de árvores com o mapa velho, um cargo que não
    /// compila nada e deixa um scan e um programa de teste falsos, e a pasta de
    /// configuração do Claude Code.
    #[cfg(unix)]
    struct World {
        _dir: tempfile::TempDir,
        repo: PathBuf,
        base: PathBuf,
        trees: PathBuf,
        log: PathBuf,
        fail: PathBuf,
        host: Host,
    }

    #[cfg(unix)]
    fn world() -> World {
        let dir = tempdir().unwrap();
        let (repo, base, config, bin) = (dir.path().join("repo"), dir.path().join("base"), dir.path().join("config"), dir.path().join("bin"));
        for folder in [&repo, &base, &config, &bin] {
            std::fs::create_dir_all(folder).unwrap();
        }
        fake_source(&repo, &[("packages/core", "measure_it")]);
        let git = |args: &[&str]| assert!(git::run(&repo, args).ok, "git {args:?}");
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "semente"]);

        let (log, fail) = (dir.path().join("log"), dir.path().join("fail"));
        fake_scan(&bin.join("scan"), &log, &fail);
        // O programa de teste: lista a régua e, ao rodar, imprime a prova com o
        // gancho e o conteúdo do banco da árvore que a variável aponta.
        let ruler = bin.join("ruler");
        crate::executable::write_executable(
            &ruler,
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n--list) echo 'mod::measure_it: test' ;;\n*) echo ruler >> '{log}'\n   echo \"PROVA commit=$MUSTARD_MEASURE_COMMIT gancho=$MUSTARD_MEASURE_HOOK home=$HOME mapa=$(cat \"$SPEND_TREES/p/.claude/grain.db\")\" ;;\nesac\n",
                log = log.display()
            ),
        );
        let artifact = format!(
            r#"{{"reason":"compiler-artifact","package_id":"mustard-core 0.1.0 (path+file:///w/packages/core)","manifest_path":"{manifest}","target":{{"kind":["lib"],"name":"x"}},"profile":{{"test":true}},"executable":"{ruler}"}}"#,
            manifest = repo.join("packages/core/Cargo.toml").display(),
            ruler = ruler.display()
        );
        let cargo = bin.join("cargo");
        crate::executable::write_executable(
            &cargo,
            &format!(
                "#!/bin/sh\necho \"cargo $1\" >> '{log}'\ncase \"$1\" in\nbuild) mkdir -p \"$CARGO_TARGET_DIR/release\" && cp '{scan}' \"$CARGO_TARGET_DIR/release/scan\" ;;\ntest) printf '%s\\n' '{artifact}' ;;\nesac\n",
                log = log.display(),
                scan = bin.join("scan").display()
            ),
        );
        plugin_with(&config, "mustard-local", "0.2.4", "mustard-rt 0.2.4 (build dev, g10d66039a5b1 2026-09-25)");
        let trees = trees_with(dir.path(), &["p"]);
        World { host: Host { cargo: Some(cargo), config: Some(config) }, _dir: dir, repo, base, trees, log, fail }
    }

    /// Os passos que a medida deu, na ordem, sem a leitura da história, que
    /// o scan de mentira anota junto dos outros passos.
    #[cfg(unix)]
    fn logged(world: &World) -> Vec<String> {
        let all = std::fs::read_to_string(&world.log).unwrap_or_default();
        all.lines().filter(|line| !line.starts_with("history-all ")).map(str::to_string).collect()
    }

    /// A medida inteira: antes da régua o cargo compila, o scan compilado
    /// refaz o mapa de cada árvore (o banco velho não chega à régua), a árvore
    /// vai à régua por `SPEND_TREES`, e a prova que ela imprime traz o gancho
    /// do plugin instalado.
    #[cfg(unix)]
    #[test]
    fn the_measure_rebuilds_the_maps_before_the_ruler_and_the_proof_carries_the_hook() {
        let world = world();
        let mut asked = opts("measure_it");
        asked.trees = Some(world.trees.clone());
        // O `HOME` que a medida recebe em `--env` vai só à régua: o gancho
        // sai do registro de plugins, que não anda com ele.
        asked.env = vec!["HOME=/pasta/vazia".to_string()];

        let report = measure_in(&world.repo, &world.base, &asked, &world.host).unwrap();

        assert!(report.contains("gancho=10d66039a5b1") && report.contains("home=/pasta/vazia"), "{report}");
        assert!(report.contains("mapa=novo") && !report.contains("velho"), "the ruler read the map the scan wrote: {report}");
        assert_eq!(logged(&world), ["cargo build", &format!("scan {}", world.trees.join("p").display()), "cargo test", "ruler"]);
        let reading = format!("history-all {}", world.trees.join("p").display());
        assert!(scan_calls(&world.log).contains(&reading), "the history of the rebuilt map is read, as in the sessions");

        // Sem plugin instalado a mesma medida diz que o gancho não está.
        let bare = tempdir().unwrap();
        let host = Host { cargo: world.host.cargo.clone(), config: Some(bare.path().to_path_buf()) };
        let report = measure_in(&world.repo, &world.base, &asked, &host).unwrap();
        assert!(report.contains("gancho=não instalado"), "{report}");
    }

    /// Troca o scan do mundo por um cuja leitura da história faz `history`,
    /// depois de anotar o pedido; o scan grava o banco novo como o de sempre.
    #[cfg(unix)]
    fn replace_history_reading(world: &World, history: &str) {
        let scan = world.host.cargo.as_ref().expect("the world has a cargo").with_file_name("scan");
        crate::executable::write_executable(
            &scan,
            &format!(
                "#!/bin/sh\necho \"$1 $2\" >> '{log}'\nif [ \"$1\" = history-all ]; then {history}; exit 0; fi\nmkdir -p \"$(dirname \"$4\")\" && printf novo > \"$4\"\necho '{{\"full\":true,\"read\":[],\"files\":1}}'\n",
                log = world.log.display()
            ),
        );
    }

    /// A régua só começa depois de a leitura da história de cada mapa
    /// acabar: o scan de mentira demora para terminar a leitura e anota o fim,
    /// e o fim vem antes do teste e da régua.
    #[cfg(unix)]
    #[test]
    fn the_ruler_starts_only_after_the_history_of_each_map_was_read_to_the_end() {
        let world = world();
        replace_history_reading(&world, &format!("sleep 1; echo history-finished >> '{}'", world.log.display()));
        let mut asked = opts("measure_it");
        asked.trees = Some(world.trees.clone());

        measure_in(&world.repo, &world.base, &asked, &world.host).unwrap();

        let tree = world.trees.join("p").display().to_string();
        let steps: Vec<String> = std::fs::read_to_string(&world.log).unwrap().lines().map(str::to_string).collect();
        assert_eq!(steps, ["cargo build".to_string(), format!("scan {tree}"), format!("history-all {tree}"), "history-finished".into(), "cargo test".into(), "ruler".into()]);
    }

    /// A leitura da história que falha recusa a medida, dizendo a árvore e o
    /// que o scan disse: nem teste nem régua rodam sobre o mapa pela metade.
    #[cfg(unix)]
    #[test]
    fn a_history_that_fails_to_be_read_refuses_the_measure_before_the_ruler_runs() {
        let world = world();
        replace_history_reading(&world, "echo 'git quebrou' >&2; exit 3");
        let mut asked = opts("measure_it");
        asked.trees = Some(world.trees.clone());

        let refusal = measure_in(&world.repo, &world.base, &asked, &world.host).unwrap_err();

        assert!(refusal.contains("o scan não leu a história do mapa") && refusal.contains(&world.trees.join("p").display().to_string()) && refusal.contains("git quebrou"), "{refusal}");
        let steps = logged(&world);
        assert!(!steps.contains(&"ruler".to_string()) && !steps.contains(&"cargo test".to_string()), "{steps:?}");
    }

    /// O scan que falha recusa a medida: nenhuma régua roda sobre o banco que
    /// não foi refeito, e a recusa diz qual árvore.
    #[cfg(unix)]
    #[test]
    fn a_scan_that_fails_refuses_the_measure_before_the_ruler_runs() {
        let world = world();
        std::fs::write(&world.fail, "").unwrap();
        let mut asked = opts("measure_it");
        asked.trees = Some(world.trees.clone());

        let refusal = measure_in(&world.repo, &world.base, &asked, &world.host).unwrap_err();

        assert!(refusal.contains("o scan não refez o mapa") && refusal.contains(&world.trees.join("p").display().to_string()), "{refusal}");
        let steps = logged(&world);
        assert!(!steps.contains(&"ruler".to_string()) && !steps.contains(&"cargo test".to_string()), "{steps:?}");
    }
}
