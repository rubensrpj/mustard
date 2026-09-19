//! `wave_alive_observer` — o sinal de vida de uma onda.
//!
//! ## Por quê
//!
//! O agente de onda trabalha sozinho, numa cópia separada; a rodada só volta
//! a vê-lo quando ele termina, pausa ou o Claude Code dele fecha. Entre uma
//! chamada e outra, nada dizia se ele ainda estava ali, trabalhando. Este
//! observador roda depois de cada ferramenta, nunca barra e grava a hora de
//! agora, sempre que a pasta de trabalho da chamada está dentro da cópia de
//! uma onda (`.claude/worktrees/mustard-<spec>-<onda>`), num arquivo por onda
//! sob a pasta da spec. A rodada ([`crate::commands::flow::round::queue`]) lê
//! essa hora para avisar quando uma onda passa 40 minutos sem nenhuma.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use serde_json::Value;

/// O observador do sinal de vida.
pub struct WaveAliveObserver;

/// A onda dona da pasta `folder` (só o nome dela, sem o caminho em volta),
/// pelo nome que a rodada dá à cópia de uma onda:
/// `mustard-<spec>-<onda>`.
fn parse_wave_folder(folder: &str) -> Option<(String, u64)> {
    let name = folder.strip_prefix("mustard-")?;
    let (spec, wave) = name.rsplit_once('-')?;
    Some((spec.to_string(), wave.parse().ok()?))
}

/// A onda dona da cópia em `cwd`, quando `cwd` está dentro de
/// `<root>/.claude/worktrees/mustard-<spec>-<onda>` — o mesmo nome que
/// [`crate::commands::flow::stuck`] reconhece como cópia de onda. Também lida
/// por [`crate::hooks::session::conversation_size`], para achar a conversa do
/// agente de onda.
pub(crate) fn wave_of_copy(root: &Path, cwd: &Path) -> Option<(String, u64)> {
    let worktrees = root.join(".claude").join("worktrees");
    let rel = cwd.strip_prefix(&worktrees).ok()?;
    let folder = rel.components().next()?.as_os_str().to_str()?;
    parse_wave_folder(folder)
}

/// A onda citada num caminho de cópia dentro do texto `command` — o comando
/// do Bash —, pelo mesmo nome de pasta que [`wave_of_copy`] reconhece.
fn wave_in_command(command: &str) -> Option<(String, u64)> {
    let marker = "worktrees/mustard-";
    let start = command.find(marker)? + "worktrees/".len();
    let rest = &command[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(rest.len());
    parse_wave_folder(&rest[..end])
}

/// A onda dona da chamada `input`, sob `root` — uma leitura só, usada pelo
/// sinal de vida e pela pausa aos 200 mil. O agente de onda lê e edita os
/// arquivos da cópia pelo caminho, sem mudar de pasta: a pasta de trabalho da
/// chamada dele continua sendo a do projeto, não a da cópia. Por isso a
/// leitura tenta, nesta ordem: (1) a pasta de trabalho (`cwd`); (2) o caminho
/// do arquivo no pedido da ferramenta (`file_path`, `notebook_path` ou
/// `path`); (3) um caminho de cópia citado no comando do Bash.
pub(crate) fn wave_of_call(root: &Path, input: &HookInput) -> Option<(String, u64)> {
    if let Some(found) =
        input.cwd.as_deref().filter(|c| !c.is_empty()).and_then(|cwd| wave_of_copy(root, Path::new(cwd)))
    {
        return Some(found);
    }
    for key in ["file_path", "notebook_path", "path"] {
        if let Some(found) =
            input.tool_input.get(key).and_then(Value::as_str).and_then(|p| wave_of_copy(root, Path::new(p)))
        {
            return Some(found);
        }
    }
    let command = input.tool_input.get("command").and_then(Value::as_str)?;
    wave_in_command(command)
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
    use tempfile::tempdir;

    fn ctx(dir: &str, trigger: Trigger) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(trigger))
    }

    fn input_with_cwd(cwd: &Path) -> HookInput {
        HookInput { cwd: Some(cwd.to_string_lossy().into_owned()), ..HookInput::default() }
    }

    /// A pasta de trabalho dentro da cópia de uma onda grava a hora ali,
    /// sob a pasta da spec, no arquivo desta onda.
    #[test]
    fn a_tool_call_inside_a_wave_copy_records_the_time_there() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = root.join(".claude").join("worktrees").join("mustard-x-3").join("src");
        std::fs::create_dir_all(&copy).unwrap();

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
        let copy = root.join(".claude").join("worktrees").join("mustard-x-3");
        std::fs::create_dir_all(copy.join("src")).unwrap();

        let by_file = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "file_path": copy.join("src").join("lib.rs").to_string_lossy() }),
            ..HookInput::default()
        };
        WaveAliveObserver.observe(&by_file, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", 3)).expect("achou pelo file_path");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");

        std::fs::remove_file(alive_path(root, "x", 3)).ok();
        let by_command = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": "cargo test --manifest-path .claude/worktrees/mustard-x-3/Cargo.toml" }),
            ..HookInput::default()
        };
        WaveAliveObserver.observe(&by_command, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", 3)).expect("achou pelo comando do Bash");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");
    }

    /// Um evento que não é `PostToolUse` — mesmo com a pasta de trabalho na
    /// cópia — não escreve nada: o registro só chama este observador ali.
    #[test]
    fn a_non_post_tool_use_trigger_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = root.join(".claude").join("worktrees").join("mustard-x-3");
        std::fs::create_dir_all(&copy).unwrap();

        WaveAliveObserver.observe(&input_with_cwd(&copy), &ctx(root.to_str().unwrap(), Trigger::PreToolUse));

        assert!(!alive_path(root, "x", 3).exists());
    }
}
