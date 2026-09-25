//! `wave_alive_observer` — o sinal de vida de uma onda.
//!
//! ## Por quê
//!
//! O agente de onda trabalha sozinho, numa cópia separada; a rodada só volta
//! a vê-lo quando ele termina, pausa ou o Claude Code dele fecha. Entre uma
//! chamada e outra, nada dizia se ele ainda estava ali, trabalhando. Este
//! observador roda depois de cada ferramenta, nunca barra e grava a hora de
//! agora, sempre que a chamada mexe dentro de uma vaga — a cópia fixa em que
//! as ondas trabalham, `<spec>/<vaga>` na pasta das cópias do projeto, fora
//! dele —, num arquivo por vaga sob a pasta da spec. Ele não lê o registro
//! da spec: a rodada ([`crate::commands::flow::round::queue`]) acha a vaga de
//! cada onda pela cópia gravada no envio e lê essa hora para avisar quando
//! uma onda passa 40 minutos sem nenhuma.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use mustard_core::io::wave_prompt::{copies_dir, shown};
use serde_json::Value;

/// O observador do sinal de vida.
pub struct WaveAliveObserver;

/// A spec e a vaga donas do caminho relativo `rel`, lido a partir da pasta
/// das cópias do projeto: as duas primeiras partes dele, `<spec>/<vaga>`. O
/// caminho que para na pasta da spec não é de vaga nenhuma.
fn slot_of_parts<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<(String, String)> {
    let spec = parts.next().filter(|part| !part.is_empty() && *part != "." && *part != "..")?;
    let slot = parts.next().filter(|part| !part.is_empty() && *part != "." && *part != "..")?;
    Some((spec.to_string(), slot.to_string()))
}

/// A spec e a vaga da cópia em `cwd`, quando `cwd` está dentro de
/// `<copies>/<spec>/<vaga>` — a pasta das cópias do projeto
/// ([`copies_dir`]), a mesma que [`crate::commands::flow::stuck`] lê.
pub(crate) fn slot_of_copy(copies: &Path, cwd: &Path) -> Option<(String, String)> {
    let rel = cwd.strip_prefix(copies).ok()?;
    slot_of_parts(rel.components().filter_map(|part| part.as_os_str().to_str()))
}

/// A vaga citada num caminho de cópia dentro do texto `command` — o comando
/// do Bash —, pela pasta das cópias do projeto (`copies`). A barra invertida
/// do Windows conta como a barra normal.
fn slot_in_command(copies: &Path, command: &str) -> Option<(String, String)> {
    let marker = format!("{}/", shown(copies));
    let command = command.replace('\\', "/");
    let start = command.find(&marker)? + marker.len();
    let rest = &command[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '"' || c == '\'').unwrap_or(rest.len());
    slot_of_parts(rest[..end].split('/'))
}

/// A spec e a vaga donas da chamada `input`, sob `root` — uma leitura só,
/// usada pelo sinal de vida. O agente de onda lê e edita os arquivos da
/// cópia pelo caminho, sem mudar de pasta: a pasta de trabalho da chamada
/// dele continua sendo a do projeto, não a da cópia. Por isso a leitura
/// tenta, nesta ordem: (1) a pasta de trabalho (`cwd`); (2) o caminho do
/// arquivo no pedido da ferramenta (`file_path`, `notebook_path` ou `path`);
/// (3) um caminho de cópia citado no comando do Bash.
pub(crate) fn slot_of_call(root: &Path, input: &HookInput) -> Option<(String, String)> {
    let copies = copies_dir(root);
    if let Some(found) =
        input.cwd.as_deref().filter(|c| !c.is_empty()).and_then(|cwd| slot_of_copy(&copies, Path::new(cwd)))
    {
        return Some(found);
    }
    for key in ["file_path", "notebook_path", "path"] {
        if let Some(found) =
            input.tool_input.get(key).and_then(Value::as_str).and_then(|p| slot_of_copy(&copies, Path::new(p)))
        {
            return Some(found);
        }
    }
    let command = input.tool_input.get("command").and_then(Value::as_str)?;
    slot_in_command(&copies, command)
}

/// O arquivo que guarda a hora da última ação na vaga `slot` da spec `spec`,
/// sob a pasta dela. [`crate::commands::flow::round::queue::silent_minutes`]
/// é quem lê.
#[must_use]
pub(crate) fn alive_path(root: &Path, spec: &str, slot: &str) -> PathBuf {
    root.join(".claude").join("spec").join(spec).join("waves").join(format!("{slot}.alive"))
}

impl Observer for WaveAliveObserver {
    /// Depois de qualquer ferramenta, com a chamada dentro de uma vaga, grava
    /// a hora de agora ali. Qualquer outra chamada — outra pasta, ou sem
    /// caminho de vaga — não escreve nada; nunca barra.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some((spec, slot)) = slot_of_call(&root, input) else { return };
        if !root.join(".claude").join("spec").join(&spec).is_dir() {
            return;
        }
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let _ = fs::write_atomic(alive_path(&root, &spec, &slot), now.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::wave_prompt::slot_path;
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

    /// Um projeto com a pasta da spec `x`, onde o sinal de vida grava.
    fn project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join("x")).unwrap();
        dir
    }

    /// A pasta de trabalho dentro de uma vaga grava a hora ali, sob a pasta
    /// da spec, no arquivo desta vaga.
    #[test]
    fn a_tool_call_inside_a_slot_records_the_time_there() {
        let dir = project();
        let root = dir.path();
        let copy = slot_path(root, "x", 2).join("src");

        WaveAliveObserver.observe(&input_with_cwd(&copy), &ctx(root.to_str().unwrap(), Trigger::PostToolUse));

        let recorded = std::fs::read_to_string(alive_path(root, "x", "c")).expect("the alive file was written");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");
    }

    /// A pasta de trabalho fora de qualquer vaga não escreve nada, nem a
    /// pasta da spec sem vaga, nem a vaga de uma spec que não existe no
    /// projeto.
    #[test]
    fn a_tool_call_outside_any_slot_writes_nothing() {
        let dir = project();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let observe = |cwd: &Path| {
            WaveAliveObserver.observe(&input_with_cwd(cwd), &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        };

        observe(&root.join("src"));
        observe(&mustard_core::io::wave_prompt::spec_copies_dir(root, "x"));
        observe(&slot_path(root, "sumiu", 0));

        assert!(!root.join(".claude").join("spec").join("x").join("waves").exists(), "nothing under the spec");
        assert!(!root.join(".claude").join("spec").join("sumiu").exists(), "no folder for a missing spec");
    }

    /// O agente de onda lê e edita pelo caminho, sem mudar de pasta: a pasta
    /// de trabalho da chamada é a do projeto, não a da cópia. O gancho acha
    /// a vaga mesmo assim, pelo caminho do arquivo pedido (`file_path`) e,
    /// sem ele, por um caminho de cópia citado no comando do Bash.
    #[test]
    fn the_slot_is_found_by_the_paths_of_the_call() {
        let dir = project();
        let root = dir.path();
        let copy = slot_path(root, "x", 0);

        let by_file = input_with_file(root, &copy.join("src").join("lib.rs"));
        WaveAliveObserver.observe(&by_file, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", "a")).expect("achou pelo file_path");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");

        std::fs::remove_file(alive_path(root, "x", "a")).ok();
        let by_command = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({
                "command": format!("cargo test --manifest-path {}/Cargo.toml", shown(&copy))
            }),
            ..HookInput::default()
        };
        WaveAliveObserver.observe(&by_command, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        let recorded = std::fs::read_to_string(alive_path(root, "x", "a")).expect("achou pelo comando do Bash");
        assert!(chrono::DateTime::parse_from_rfc3339(recorded.trim()).is_ok(), "{recorded}");
    }

    /// A vaga mora fora da pasta do projeto, na pasta das cópias dele, e o
    /// sinal de vida a acha ali — pela pasta de trabalho, pelo arquivo e pelo
    /// comando. O antigo lugar dentro do projeto
    /// (`.claude/worktrees/mustard-<spec>-<onda>`) e a pasta de mesmo nome na
    /// cópia de outro projeto não contam como vaga deste projeto.
    #[test]
    fn the_alive_signal_finds_a_slot_outside_the_project() {
        let dir = project();
        let root = dir.path();
        let other = tempdir().unwrap();
        let copy = slot_path(root, "x", 1);
        let project = std::fs::canonicalize(root).unwrap();
        assert!(!copy.starts_with(root) && !copy.starts_with(&project), "the copy lives outside the project: {copy:?}");
        let observe = |input: &HookInput| {
            WaveAliveObserver.observe(input, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        };
        let alive = |slot: &str| alive_path(root, "x", slot).exists();

        observe(&input_with_cwd(&copy.join("src")));
        assert!(alive("b"), "found by the working folder inside the outside copy");
        std::fs::remove_file(alive_path(root, "x", "b")).unwrap();
        observe(&input_with_file(root, &copy.join("src").join("lib.rs")));
        assert!(alive("b"), "found by the file inside the outside copy");
        std::fs::remove_file(alive_path(root, "x", "b")).unwrap();
        observe(&HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "command": format!("cd {} && cargo test", shown(&copy)) }),
            ..HookInput::default()
        });
        assert!(alive("b"), "found by the command that cites the outside copy");

        let old = root.join(".claude").join("worktrees").join("mustard-x-4").join("src").join("lib.rs");
        observe(&input_with_file(root, &old));
        observe(&HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: serde_json::json!({ "command": "cargo test --manifest-path .claude/worktrees/mustard-x-4/Cargo.toml" }),
            ..HookInput::default()
        });
        assert!(!root.join(".claude").join("spec").join("x").join("waves").join("src.alive").exists());
        let written: Vec<_> = std::fs::read_dir(root.join(".claude").join("spec").join("x").join("waves"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(written, vec!["b.alive"], "the old place inside the project is not a slot");

        observe(&input_with_file(root, &slot_path(other.path(), "x", 4).join("lib.rs")));
        assert!(!alive("e"), "the same folder name under another project's copies is not this project's slot");
    }

    /// No Windows, o comando do Bash escreve o caminho da cópia com barras
    /// invertidas; o sinal de vida acha a vaga nele do mesmo jeito, no `cd` e,
    /// entre aspas, no caminho do manifesto. O mesmo nome de pasta, com as
    /// mesmas barras, sob as cópias de outro projeto não conta.
    #[test]
    fn a_command_with_the_copy_written_in_backslashes_records_the_time() {
        let dir = project();
        let root = dir.path();
        let other = tempdir().unwrap();
        let backslashed = |path: &Path| shown(path).replace('/', "\\");
        let command = |text: String| HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": text }),
            ..HookInput::default()
        };
        let observe = |input: &HookInput| {
            WaveAliveObserver.observe(input, &ctx(root.to_str().unwrap(), Trigger::PostToolUse));
        };

        observe(&command(format!("cd {} && cargo test", backslashed(&slot_path(root, "x", 2)))));
        assert!(alive_path(root, "x", "c").exists(), "found by the copy written in backslashes");
        let manifest = slot_path(root, "x", 6).join("Cargo.toml");
        observe(&command(format!("cargo test --manifest-path \"{}\"", backslashed(&manifest))));
        assert!(alive_path(root, "x", "g").exists(), "found by the quoted copy written in backslashes");

        observe(&command(format!("cd {} && cargo test", backslashed(&slot_path(other.path(), "x", 4)))));
        assert!(!alive_path(root, "x", "e").exists(), "another project's copy is not this project's slot");
    }

    /// Um evento que não é `PostToolUse` — mesmo com a pasta de trabalho na
    /// vaga — não escreve nada: o registro só chama este observador ali.
    #[test]
    fn a_non_post_tool_use_trigger_writes_nothing() {
        let dir = project();
        let root = dir.path();
        let copy = slot_path(root, "x", 0);

        WaveAliveObserver.observe(&input_with_cwd(&copy), &ctx(root.to_str().unwrap(), Trigger::PreToolUse));

        assert!(!alive_path(root, "x", "a").exists());
    }
}
