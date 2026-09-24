//! `wave_alive_observer` — o sinal de vida de uma onda.
//!
//! ## Por quê
//!
//! O agente de onda trabalha sozinho, numa cópia separada; a rodada só volta
//! a vê-lo quando ele termina, pausa ou o Claude Code dele fecha. Entre uma
//! chamada e outra, nada dizia se ele ainda estava ali, trabalhando. Este
//! observador roda depois de cada ferramenta, nunca barra e grava a hora de
//! agora, sempre que a chamada mexe dentro da cópia de uma onda
//! (`<spec>-<onda>`, na pasta das cópias do projeto, fora dele), num arquivo
//! por onda sob a pasta da spec. A rodada
//! ([`crate::commands::flow::round::queue`]) lê essa hora para avisar quando
//! uma onda passa 40 minutos sem nenhuma.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use mustard_core::io::wave_prompt::{copies_dir, shown};
use serde_json::Value;

/// O observador do sinal de vida.
pub struct WaveAliveObserver;

/// A onda dona da pasta `folder` (só o nome dela, sem o caminho em volta),
/// pelo nome que a rodada dá à cópia de uma onda: `<spec>-<onda>`. A cópia
/// do revisor (`<spec>-<onda>-review`) e a do revisor final não são de onda.
fn parse_wave_folder(folder: &str) -> Option<(String, u64)> {
    let (spec, wave) = folder.rsplit_once('-').filter(|(spec, _)| !spec.is_empty())?;
    Some((spec.to_string(), wave.parse().ok()?))
}

/// A onda dona da cópia em `cwd`, quando `cwd` está dentro de
/// `<copies>/<spec>-<onda>` — a pasta das cópias do projeto
/// ([`copies_dir`]), a mesma que [`crate::commands::flow::stuck`] lê.
pub(crate) fn wave_of_copy(copies: &Path, cwd: &Path) -> Option<(String, u64)> {
    let rel = cwd.strip_prefix(copies).ok()?;
    let folder = rel.components().next()?.as_os_str().to_str()?;
    parse_wave_folder(folder)
}

/// A onda citada num caminho de cópia dentro do texto `command` — o comando
/// do Bash —, pela pasta das cópias do projeto (`copies`) e pelo mesmo nome
/// de pasta que [`wave_of_copy`] reconhece. A barra invertida do Windows
/// conta como a barra normal.
fn wave_in_command(copies: &Path, command: &str) -> Option<(String, u64)> {
    let marker = format!("{}/", shown(copies));
    let command = command.replace('\\', "/");
    let start = command.find(&marker)? + marker.len();
    let rest = &command[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(rest.len());
    parse_wave_folder(&rest[..end])
}

/// A onda dona da chamada `input`, sob `root` — uma leitura só, usada pelo
/// sinal de vida. O agente de onda lê e edita os arquivos da cópia pelo
/// caminho, sem mudar de pasta: a pasta de trabalho da chamada dele continua
/// sendo a do projeto, não a da cópia. Por isso a leitura tenta, nesta ordem:
/// (1) a pasta de trabalho (`cwd`); (2) o caminho do arquivo no pedido da
/// ferramenta (`file_path`, `notebook_path` ou `path`); (3) um caminho de
/// cópia citado no comando do Bash.
pub(crate) fn wave_of_call(root: &Path, input: &HookInput) -> Option<(String, u64)> {
    let copies = copies_dir(root);
    if let Some(found) =
        input.cwd.as_deref().filter(|c| !c.is_empty()).and_then(|cwd| wave_of_copy(&copies, Path::new(cwd)))
    {
        return Some(found);
    }
    for key in ["file_path", "notebook_path", "path"] {
        if let Some(found) =
            input.tool_input.get(key).and_then(Value::as_str).and_then(|p| wave_of_copy(&copies, Path::new(p)))
        {
            return Some(found);
        }
    }
    let command = input.tool_input.get("command").and_then(Value::as_str)?;
    wave_in_command(&copies, command)
}

/// O arquivo que guarda a hora da última ação da onda `wave` da spec `spec`,
/// sob a pasta dela. [`crate::commands::flow::round::queue::silent_minutes`]
/// é quem lê.
#[must_use]
pub(crate) fn alive_path(root: &Path, spec: &str, wave: u64) -> PathBuf {
    root.join(".claude").join("spec").join(spec).join("waves").join(format!("{wave}.alive"))
}

impl Observer for WaveAliveObserver {
    /// Depois de qualquer ferramenta, com a pasta de trabalho dentro da cópia
    /// de uma onda, grava a hora de agora ali. Qualquer outra chamada — outra
    /// pasta, ou sem `cwd` — não escreve nada; nunca barra.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some((spec, wave)) = wave_of_call(&root, input) else { return };
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let _ = fs::write_atomic(alive_path(&root, &spec, wave), now.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::wave_prompt::copy_path;
    use tempfile::tempdir;

    fn ctx(dir: &str, trigger: Trigger) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(trigger))
    }

    fn input_with_cwd(cwd: &Path) -> HookInput {
        HookInput { cwd: Some(cwd.to_string_lossy().into_owned()), ..HookInput::default() }
    }

    /// A chamada que só cita o arquivo `file`, com a pasta de trabalho na do
    /// projeto `root`, como o agente de onda edita.
    fn input_with_file(root: &Path, file: &Path) -> HookInput {
        HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "file_path": file.to_string_lossy() }),
            ..HookInput::default()
        }
    }

    /// A pasta de trabalho dentro da cópia de uma onda grava a hora ali,
    /// sob a pasta da spec, no arquivo desta onda.
    #[test]
    fn a_tool_call_inside_a_wave_copy_records_the_time_there() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = copy_path(root, "x", 3, false).join("src");

        WaveAliveObserver.observe(&input_with_cwd(&copy), &ctx(root.to_str().unwrap(), Trigger::PostToolUse));

        let recorded = std::fs::read_to_string(alive_path(root, "x", 3)).expect("the alive file was written");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");
    }

    /// A pasta de trabalho fora de qualquer cópia de onda não escreve nada.
    #[test]
    fn a_tool_call_outside_any_wave_copy_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();

        WaveAliveObserver.observe(
            &input_with_cwd(&root.join("src")),
            &ctx(root.to_str().unwrap(), Trigger::PostToolUse),
        );

        assert!(!root.join(".claude").join("spec").exists(), "nothing under the spec folder");
    }

    /// O agente de onda lê e edita pelo caminho, sem mudar de pasta: a pasta
    /// de trabalho da chamada é a do projeto, não a da cópia. O gancho acha
    /// a onda mesmo assim, pelo caminho do arquivo pedido (`file_path`) e,
    /// sem ele, por um caminho de cópia citado no comando do Bash.
    #[test]
    fn the_wave_is_found_by_the_paths_of_the_call() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = copy_path(root, "x", 3, false);

        let by_file = input_with_file(root, &copy.join("src").join("lib.rs"));
        WaveAliveObserver.observe(&by_file, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", 3)).expect("achou pelo file_path");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");

        std::fs::remove_file(alive_path(root, "x", 3)).ok();
        let by_command = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({
                "command": format!("cargo test --manifest-path {}/Cargo.toml", shown(&copy))
            }),
            ..HookInput::default()
        };
        WaveAliveObserver.observe(&by_command, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", 3)).expect("achou pelo comando do Bash");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");
    }

    /// A cópia da onda mora fora da pasta do projeto, na pasta das cópias
    /// dele, e o sinal de vida a acha ali — pela pasta de trabalho, pelo
    /// arquivo e pelo comando. O antigo lugar dentro do projeto
    /// (`.claude/worktrees/mustard-<spec>-<onda>`), a pasta de mesmo nome na
    /// cópia de outro projeto e a cópia do revisor não contam como a cópia
    /// de uma onda deste projeto.
    #[test]
    fn the_alive_signal_finds_a_copy_outside_the_project() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let other = tempdir().unwrap();
        let copy = copy_path(root, "x", 3, false);
        let project = std::fs::canonicalize(root).unwrap();
        assert!(!copy.starts_with(root) && !copy.starts_with(&project), "the copy lives outside the project: {copy:?}");
        let observe = |input: &HookInput| {
            WaveAliveObserver.observe(input, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        };
        let alive = |wave: u64| alive_path(root, "x", wave).exists();

        observe(&input_with_cwd(&copy.join("src")));
        assert!(alive(3), "found by the working folder inside the outside copy");
        std::fs::remove_file(alive_path(root, "x", 3)).unwrap();
        observe(&input_with_file(root, &copy.join("src").join("lib.rs")));
        assert!(alive(3), "found by the file inside the outside copy");
        std::fs::remove_file(alive_path(root, "x", 3)).unwrap();
        observe(&HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "command": format!("cd {} && cargo test", shown(&copy)) }),
            ..HookInput::default()
        });
        assert!(alive(3), "found by the command that cites the outside copy");

        let old = root.join(".claude").join("worktrees").join("mustard-x-4").join("src").join("lib.rs");
        observe(&input_with_file(root, &old));
        observe(&HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "command": "cargo test --manifest-path .claude/worktrees/mustard-x-4/Cargo.toml" }),
            ..HookInput::default()
        });
        assert!(!alive(4), "the old place inside the project is not a wave copy");

        observe(&input_with_file(root, &copy_path(other.path(), "x", 5, false).join("lib.rs")));
        assert!(!alive(5), "the same folder name under another project's copies is not this project's wave");
        observe(&input_with_file(root, &copy_path(root, "x", 6, true).join("lib.rs")));
        assert!(!alive(6), "the reviewer's copy is not a wave copy");
    }

    /// Um evento que não é `PostToolUse` — mesmo com a pasta de trabalho na
    /// cópia — não escreve nada: o registro só chama este observador ali.
    #[test]
    fn a_non_post_tool_use_trigger_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = copy_path(root, "x", 3, false);

        WaveAliveObserver.observe(&input_with_cwd(&copy), &ctx(root.to_str().unwrap(), Trigger::PreToolUse));

        assert!(!alive_path(root, "x", 3).exists());
    }
}
