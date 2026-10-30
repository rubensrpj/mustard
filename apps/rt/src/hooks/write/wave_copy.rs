//! Bind wave writes to the exact native dispatch, never to inherited cwd.
use crate::hooks::bash::lex::Segment;
use mustard_core::{
    domain::model::contract::{Ctx, HookInput, Verdict},
    io::wave_prompt,
};
use std::path::{Path, PathBuf};

pub(crate) fn check(root: &Path, input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !input.is_subagent() {
        return None;
    }
    let host = crate::shared::host_observation::caller_host(root, input.session_id.as_deref());
    let binding = crate::shared::host_observation::tool_binding(root, host, input);
    let supplied = input.file_path()?;
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let path = Path::new(&supplied);
    let absolute = lexical(&if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) });
    let copies = wave_prompt::copies_dir(root);
    if !absolute.starts_with(root) && !absolute.starts_with(&copies) {
        return None;
    }
    if binding.is_none() && !is_wave_agent(input) && !absolute.starts_with(&copies) {
        return None;
    }
    let Some(binding) = binding else {
        return Some(Verdict::Deny{reason:"Mustard: cannot identify this wave's native dispatch. Read the recorded wave request before editing; do not choose a copy from inherited cwd.".into()});
    };
    // O envio já veio lido com a ligação: o arquivo de eventos não é lido de novo.
    let (spec, wave) = (&binding.spec, binding.wave);
    let copy = Path::new(binding.send.str_field("copy")?);
    let parent = absolute.ancestors().find(|path| path.exists()).and_then(|path| path.canonicalize().ok());
    let expected = copy.canonicalize().ok();
    if parent.zip(expected).is_some_and(|(parent, copy)| parent.starts_with(copy)) {
        return None;
    }
    Some(Verdict::Deny {
        reason: format!(
            "Mustard: wave {wave} of {spec} may write only in {}. The requested path is in another checkout. Use the exact copy from the native request.",
            wave_prompt::shown(copy)
        ),
    })
}

/// O agente foi lançado como o agente de onda do Mustard, com ou sem o nome
/// do plugin na frente.
pub(crate) fn is_wave_agent(input: &HookInput) -> bool {
    input.agent_type.as_deref().is_some_and(wave_agent_kind)
}

/// O tipo de agente `kind` é o agente de onda do Mustard, com ou sem o nome
/// do plugin na frente.
pub(crate) fn wave_agent_kind(kind: &str) -> bool {
    kind == "mustard-wave" || kind.ends_with(":mustard-wave")
}

/// A pasta onde roda o comando do terminal de um agente de onda, ou `None`
/// fora dele. O agente de onda é o subagente ligado ao envio de uma onda, o
/// lançado como agente de onda ou o que trabalha na pasta das cópias. Com a
/// ligação, o comando roda na cópia do envio: a pasta atual quando está nela,
/// e a cópia quando a troca do fim o leva até ela.
pub(crate) fn wave_shell_folder(root: &Path, input: &HookInput, ctx: &Ctx) -> Option<std::path::PathBuf> {
    if !input.is_subagent() {
        return None;
    }
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let host = crate::shared::host_observation::caller_host(root, input.session_id.as_deref());
    if let Some(binding) = crate::shared::host_observation::tool_binding(root, host, input) {
        let copy = binding.send.str_field("copy").map(Path::new);
        let inside = |copy: &Path| cwd.canonicalize().ok().zip(copy.canonicalize().ok()).is_some_and(|(cwd, copy)| cwd.starts_with(copy));
        return Some(copy.filter(|copy| !inside(copy)).unwrap_or(cwd).to_path_buf());
    }
    let copies = wave_prompt::copies_dir(root);
    let in_copies = cwd.canonicalize().ok().zip(copies.canonicalize().ok()).is_some_and(|(cwd, copies)| cwd.starts_with(copies));
    (is_wave_agent(input) || in_copies).then(|| cwd.to_path_buf())
}

/// `path` sem os `.` e com cada `..` tirando a pasta de antes, sem consultar o
/// disco.
pub(crate) fn lexical(path: &Path) -> std::path::PathBuf {
    let mut absolute = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                absolute.pop();
            }
            std::path::Component::CurDir => {}
            other => absolute.push(other.as_os_str()),
        }
    }
    absolute
}

/// O terminal de uma linha de comando, seguido como o terminal segue os `cd`
/// dela: a pasta atual, a anterior e a pasta pessoal.
pub(crate) struct Shell {
    here: PathBuf,
    previous: PathBuf,
    home: Option<PathBuf>,
}

impl Shell {
    /// O terminal em `here`, vindo de `previous`, com a pasta pessoal `home`.
    pub(crate) fn new(here: PathBuf, previous: PathBuf, home: Option<PathBuf>) -> Self {
        Self { here, previous, home }
    }

    /// A pasta atual.
    pub(crate) fn here(&self) -> &Path {
        &self.here
    }

    /// O caminho que `text` nomeia a partir da pasta atual, com `~`, `~/…` e
    /// as variáveis (`$PWD`, `$OLDPWD`, `$HOME` e as do ambiente) trocadas,
    /// ou `None` quando ele depende do que só o terminal sabe (`$(…)`,
    /// variável sem valor aqui).
    pub(crate) fn path(&self, text: &str) -> Option<PathBuf> {
        if text.is_empty() {
            return None;
        }
        let path = if text == "~" {
            self.home.clone()?
        } else if let Some(rest) = text.strip_prefix("~/") {
            self.home.as_ref()?.join(rest)
        } else if let Some(variable) = text.strip_prefix('$') {
            let braced = variable.strip_prefix('{').and_then(|inner| inner.split_once('}'));
            let (name, rest) = braced.unwrap_or_else(|| {
                let end = variable.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(variable.len());
                variable.split_at(end)
            });
            let value = match name {
                "" => return None,
                "PWD" => self.here.clone(),
                "OLDPWD" => self.previous.clone(),
                "HOME" => self.home.clone()?,
                _ => PathBuf::from(std::env::var_os(name)?),
            };
            PathBuf::from(format!("{}{rest}", value.to_string_lossy()))
        } else {
            PathBuf::from(text)
        };
        Some(lexical(&if path.is_absolute() { path } else { self.here.join(path) }))
    }

    /// Segue o `cd` `segment` e devolve a pasta em que o terminal entra:
    /// `cd -` volta à anterior, e `cd` sem pasta vai à pessoal. Quando o
    /// destino não se sabe, devolve `None` e a pasta atual fica.
    pub(crate) fn cd(&mut self, segment: &Segment) -> Option<PathBuf> {
        let arg = segment.args.iter().map(|arg| arg.text.as_str()).find(|text| *text == "-" || !text.starts_with('-'));
        let path = match arg {
            Some("-") => self.previous.clone(),
            None => self.path("~")?,
            Some(text) => self.path(text)?,
        };
        self.previous = std::mem::replace(&mut self.here, path.clone());
        Some(path)
    }
}

pub(crate) fn guard_bash(input: &HookInput, ctx: &Ctx, verdict: Verdict) -> Verdict {
    let mut adjusted = input.clone();
    let previous_note = match &verdict {
        Verdict::Allow => None,
        Verdict::Rewrite { tool_input, note } => {
            adjusted.tool_input = tool_input.clone();
            note.clone()
        }
        _ => return verdict,
    };
    match bash(&adjusted, ctx, mustard_core::platform::harness::home_dir().as_deref()) {
        Some(Verdict::Rewrite { tool_input, note }) => Verdict::Rewrite {
            tool_input,
            note: match (previous_note, note) {
                (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
                (first, second) => first.or(second),
            },
        },
        Some(other) => other,
        None => verdict,
    }
}

/// `home` é a pasta pessoal, destino de `cd` sem pasta e de `cd ~/<caminho>`.
fn bash(input: &HookInput, ctx: &Ctx, home: Option<&Path>) -> Option<Verdict> {
    if !input.is_subagent() {
        return None;
    }
    let root = Path::new(&ctx.project_dir);
    let host = crate::shared::host_observation::caller_host(root, input.session_id.as_deref());
    // O envio já veio lido com a ligação: o arquivo de eventos não é lido de novo.
    let binding = crate::shared::host_observation::tool_binding(root, host, input)?;
    let wave = binding.wave;
    let expected = Path::new(binding.send.str_field("copy")?);
    let real = expected.canonicalize().ok()?;
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir));
    let command = input.tool_input["command"].as_str()?;
    // O destino de cada `cd` é comparado pelo caminho real no disco, nunca
    // pelo texto: `<checkout>/..` traz o checkout no texto e fica fora dele.
    let on_disk = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let (copies, checkout) = (on_disk(&wave_prompt::copies_dir(root)), on_disk(root));
    let in_copy = cwd.canonicalize().is_ok_and(|cwd| cwd.starts_with(&real));
    // O `cd` relativo parte de onde o terminal vai estar: da cópia, quando o
    // comando é levado a ela (a troca no fim), e depois da pasta do `cd`
    // anterior, como no terminal. A pasta de antes, destino de `cd -`, é a de
    // onde o comando parte, que o `cd` da troca para a cópia deixa como
    // anterior.
    let start = if in_copy { cwd } else { expected };
    let mut shell = Shell::new(start.to_path_buf(), cwd.to_path_buf(), home.map(Path::to_path_buf));
    for segment in crate::hooks::bash::lex::segments(command).iter().filter(|segment| segment.name() == "cd") {
        let Some(path) = shell.cd(segment) else { continue };
        let Some(entered) = path.canonicalize().ok().filter(|entered| !entered.starts_with(&real)) else { continue };
        if entered.starts_with(&copies) || entered.starts_with(&checkout) {
            // A pasta de cópias pode morar dentro do projeto: ela decide
            // primeiro. A pasta dela acima da cópia desta onda só guarda
            // cópias: não é a cópia de onda nenhuma.
            let another_copy = entered.starts_with(&copies);
            let base = (another_copy && real.starts_with(&entered)).then(|| wave_prompt::shown(&path));
            let shown = wave_prompt::shown(expected);
            let reason = match ctx.config.language().text_or_default() {
                mustard_core::SupportedLocale::PtBr => format!(
                    "Mustard: a onda {wave} usa somente {shown}. O comando tenta entrar {}; rode-o de dentro da cópia, sem `cd` para fora dela.",
                    match &base {
                        Some(base) => format!("em {base}, a pasta-base das cópias, que não é a cópia de nenhuma onda"),
                        None if another_copy => "na cópia de outra onda".into(),
                        None => "no checkout principal".into(),
                    }
                ),
                mustard_core::SupportedLocale::EnUs => format!(
                    "Mustard: wave {wave} uses only {shown}. The command tries to enter {}; run it inside the copy, without a `cd` out of it.",
                    match &base {
                        Some(base) => format!("{base}, the base folder of the copies, which is no wave's copy"),
                        None if another_copy => "another wave's copy".into(),
                        None => "the main checkout".into(),
                    }
                ),
            };
            return Some(Verdict::Deny { reason });
        }
    }
    if in_copy {
        return None;
    }
    let mut tool_input = input.tool_input.clone();
    tool_input["command"] = serde_json::json!(format!("cd -- '{}' && (\n{command}\n)", wave_prompt::shown(expected).replace('\'', "'\\''")));
    Some(Verdict::Rewrite { tool_input, note: Some(format!("Mustard: o comando da onda {wave} foi direcionado à cópia registrada no pedido.")) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events;
    use serde_json::json;
    /// O agente `worker`, despachado para a onda 1 da spec `work` com a cópia
    /// `a`, escrevendo de dentro da cópia `b`, de outra vaga.
    fn dispatched_worker(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, Ctx, HookInput) {
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let a = wave_prompt::slot_path(root, "work", 0);
        let b = wave_prompt::slot_path(root, "work", 1);
        for path in [&a, &b] {
            std::fs::create_dir_all(path).unwrap();
        }
        let path = spec_events::spec_file(root, "work").unwrap();
        let sent = spec_events::write(
            &path,
            "send",
            json!({"wave":1,"role":"wave","lines":1,"chars":1,"mustard":"test","copy":a,"host":"claude","text":"task"}).as_object().unwrap().clone(),
            &[],
        )
        .unwrap();
        let log = spec_events::read(&path).unwrap().unwrap();
        let folder = root.join(".claude/mustard/host-transcripts");
        std::fs::create_dir_all(&folder).unwrap();
        let digest = |s: &str| {
            let mut hash = mustard_core::io::sha256::Sha256::new();
            hash.update(s.as_bytes());
            hash.hex_digest()
        };
        std::fs::write(
            folder.join(format!("claude-{}-{}.json", digest("session"), digest("worker"))),
            json!({"host":"claude","spec":"work","wave":1,"dispatch":log.dispatch_position(sent.id)}).to_string(),
        )
        .unwrap();
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(mustard_core::domain::model::contract::Trigger::PreToolUse));
        let input:HookInput=serde_json::from_value(json!({"agent_id":"worker","agent_type":"mustard-wave","session_id":"session","cwd":b,"tool_name":"Write","tool_input":{"file_path":b.join("file.rs")}})).unwrap();
        (a, b, ctx, input)
    }

    #[test]
    fn dispatched_agent_cannot_write_or_enter_another_waves_copy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, b, ctx, mut input) = dispatched_worker(root);
        assert!(matches!(check(root, &input, &ctx), Some(Verdict::Deny { .. })));
        input.tool_input["file_path"] = json!(a.join("new/folder/file.rs"));
        assert!(check(root, &input, &ctx).is_none(), "new directories inside the assigned copy are allowed");
        input.tool_input["file_path"] = json!(a.join("new/../../b/file.rs"));
        assert!(matches!(check(root, &input, &ctx), Some(Verdict::Deny { .. })), "missing parent directories cannot hide a traversal into another copy");
        input.tool_name = Some("Bash".into());
        input.tool_input = json!({"command":"pwd"});
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Rewrite { .. }), "inherited cwd is repaired before execution");
        input.tool_input = json!({"command":format!("cd '{}' && write-something",b.display())});
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Deny { .. }));
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Deny { reason: "existing protection".into() }), Verdict::Deny { .. }));
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// O `cd` para o checkout principal é recusado dizendo que vai ao checkout
    /// principal, e o `cd` para outra cópia, dizendo que vai à cópia de outra
    /// onda.
    #[test]
    fn a_wave_agent_entering_the_main_checkout_is_told_it_is_the_main_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, b, ctx, mut input) = dispatched_worker(root);
        input.cwd = Some(a.to_string_lossy().into_owned());
        input.tool_name = Some("Bash".into());
        let mut refusal = |command: String| {
            input.tool_input = json!({ "command": command });
            match guard_bash(&input, &ctx, Verdict::Allow) {
                Verdict::Deny { reason } => reason,
                other => panic!("{command}: {other:?}"),
            }
        };
        let main = refusal(format!("cd -- '{}' && (\ncd -- '{}' &&\ngrep -n quartz src\n)", root.display(), a.display()));
        assert!(main.contains("no checkout principal") && !main.contains("outra onda"), "{main}");
        let other = refusal(format!("cd '{}' && pwd", b.display()));
        assert!(other.contains("na cópia de outra onda"), "{other}");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// O `cd` para a pasta-base das cópias, que a sessão do agente recebe
    /// como pasta de trabalho, e para a pasta das cópias da spec é recusado
    /// nomeando a pasta-base, e não como a cópia de outra onda.
    #[test]
    fn a_wave_agent_entering_the_copies_base_is_told_it_is_the_copies_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, _, ctx, mut input) = dispatched_worker(root);
        input.cwd = Some(a.to_string_lossy().into_owned());
        input.tool_name = Some("Bash".into());
        for base in [wave_prompt::copies_dir(root), wave_prompt::spec_copies_dir(root, "work")] {
            input.tool_input = json!({ "command": format!("cd '{}' && mustard-rt run read request-1", base.display()) });
            let Verdict::Deny { reason } = guard_bash(&input, &ctx, Verdict::Allow) else { panic!("{}: not refused", base.display()) };
            assert!(reason.contains(&format!("em {}, a pasta-base das cópias", wave_prompt::shown(&base))), "{reason}");
            assert!(!reason.contains("outra onda") && !reason.contains("checkout principal"), "{reason}");
        }
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// A pasta temporária que a sessão do agente recebe traz no nome o
    /// caminho do projeto em hífens. O agente entra nela, cria uma pasta, entra
    /// e volta com `cd ..`, e entra nela por um caminho escrito a partir do
    /// checkout principal: o comando segue para a cópia, sem recusa. Entrar no
    /// checkout principal continua recusado.
    #[test]
    fn a_wave_agent_may_enter_the_session_temp_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (_, _, ctx, mut input) = dispatched_worker(root);
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("claude-1000").join(root.to_string_lossy().replace(['/', '\\', ':'], "-")).join("session");
        let scratch = session.join("scratchpad");
        std::fs::create_dir_all(scratch.join("dockrepo")).unwrap();
        let through_root = root.join("..").join(scratch.strip_prefix(root.parent().unwrap()).unwrap_or(&scratch));
        input.cwd = Some(root.to_string_lossy().into_owned());
        input.tool_name = Some("Bash".into());
        // O atalho dentro do checkout que leva à pasta da sessão: o texto está
        // no checkout, o caminho real não.
        #[cfg(unix)]
        let link = {
            std::os::unix::fs::symlink(&scratch, root.join("scratch-link")).unwrap();
            Some(format!("cd '{}' && ls", root.join("scratch-link").display()))
        };
        #[cfg(not(unix))]
        let link = None;
        let commands = [
            format!("cd '{}' && mkdir -p dockrepo && cd dockrepo && git init -q && cd .. && ls", scratch.display()),
            format!("cd '{}' && cd .. && ls", through_root.display()),
        ];
        for command in commands.into_iter().chain(link) {
            input.tool_input = json!({ "command": command });
            assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Rewrite { .. }), "{command}");
        }
        input.tool_input = json!({ "command": format!("cd '{}' && cd '{}'", scratch.display(), root.display()) });
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Deny { .. }), "the main checkout is still refused");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// A sessão do agente troca no meio da onda: a escrita na cópia certa é
    /// recusada até ele reler o pedido da onda, e relido volta a valer. Outro
    /// agente que lê o mesmo pedido não ganha a cópia.
    #[test]
    fn a_new_session_rebinds_the_copy_when_the_agent_rereads_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, _, ctx, mut input) = dispatched_worker(root);
        input.session_id = Some("session-2".into());
        input.tool_input = json!({ "file_path": a.join("file.rs") });
        assert!(matches!(check(root, &input, &ctx), Some(Verdict::Deny { .. })), "the new session starts unbound");
        let as_agent = |agent: &str| {
            let mut other = input.clone();
            other.agent_id = Some(agent.into());
            other
        };
        let reread = |agent: &str| {
            let mut bash = as_agent(agent);
            bash.tool_name = Some("Bash".into());
            bash.tool_input = json!({ "command": format!("mustard-rt run read request-1 --root {} --spec work", wave_prompt::shown(root)) });
            crate::shared::host_observation::observe(root, "claude", "PreToolUse", &bash);
        };
        reread("stranger");
        assert!(matches!(check(root, &as_agent("stranger"), &ctx), Some(Verdict::Deny { .. })), "another agent gains no copy");
        reread("worker");
        assert!(check(root, &input, &ctx).is_none(), "the reread request binds the copy again");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// A recusa de editar pelo terminal vale para os três agentes de onda: o
    /// ligado ao envio, cujo comando roda na cópia mesmo vindo de fora dela;
    /// o lançado como agente de onda; e o subagente de outro tipo que trabalha
    /// na pasta das cópias. O subagente de outro tipo fora das cópias passa
    /// livre.
    #[test]
    fn the_shell_edit_refusal_reaches_every_wave_agent_and_frees_other_subagents() {
        use crate::hooks::bash::command_guard::CommandGuard;
        use mustard_core::domain::model::contract::Check;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let (_, b, ctx, mut input) = dispatched_worker(root);
        input.tool_name = Some("Bash".into());
        input.tool_input = json!({ "command": "echo 'pub mod x;' >> lib.rs" });
        let mut refused = |session: &str, kind: &str, cwd: &Path| {
            input.session_id = Some(session.into());
            input.agent_type = Some(kind.into());
            input.cwd = Some(cwd.to_string_lossy().into_owned());
            matches!(CommandGuard.evaluate(&input, &ctx).unwrap(), Verdict::Deny { reason } if reason.contains("`Edit`/`Write`"))
        };
        assert!(refused("session", "general-purpose", outside.path()), "the agent bound to the dispatch writes in its copy");
        assert!(refused("session-2", "mustard-wave", root), "the agent launched as the wave agent");
        assert!(refused("session-2", "general-purpose", &b), "a subagent working in the copies folder");
        assert!(!refused("session-2", "general-purpose", root), "another subagent outside the copies passes");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// `cd ~/<caminho>`, `cd ~` e `cd` sem pasta seguem a pasta pessoal, e
    /// `cd -` volta à pasta de antes: o agente que entra assim no checkout
    /// principal é recusado como no `cd` escrito por inteiro. O `cd -` que
    /// volta à cópia passa.
    #[test]
    fn home_and_previous_folder_cds_are_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, _, ctx, mut input) = dispatched_worker(root);
        std::fs::create_dir_all(a.join("apps")).unwrap();
        input.tool_name = Some("Bash".into());
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let mut verdict = |cwd: &Path, command: String, home: &Path| {
            input.cwd = Some(cwd.to_string_lossy().into_owned());
            input.tool_input = json!({ "command": command });
            bash(&input, &ctx, Some(home))
        };
        for (command, home) in [(format!("cd ~/{name} && ls"), root.parent().unwrap()), ("cd ~ && ls".into(), root), ("cd && ls".into(), root)] {
            let Some(Verdict::Deny { reason }) = verdict(&a, command.clone(), home) else { panic!("{command}: not refused") };
            assert!(reason.contains("no checkout principal"), "{command}: {reason}");
        }
        assert!(verdict(&a, "cd apps && cd - && ls".into(), root).is_none(), "back to the copy");
        let Some(Verdict::Deny { reason }) = verdict(root, "cd - && ls".into(), root.parent().unwrap()) else { panic!("cd - back to the checkout") };
        assert!(reason.contains("no checkout principal"), "{reason}");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }

    /// O `cd` relativo parte de onde o terminal vai estar: da cópia, quando o
    /// comando é levado a ela, e da pasta do `cd` anterior. A pasta que existe
    /// também no checkout principal não é tomada por ele; o `cd ..` que sai
    /// da cópia é recusado nomeando a pasta-base das cópias.
    #[test]
    fn a_relative_cd_starts_where_the_shell_will_be() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (a, _, ctx, mut input) = dispatched_worker(root);
        for checkout in [root, a.as_path()] {
            std::fs::create_dir_all(checkout.join("apps")).unwrap();
        }
        input.tool_name = Some("Bash".into());
        input.cwd = Some(root.to_string_lossy().into_owned());
        input.tool_input = json!({ "command": "cd apps && ls" });
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Rewrite { .. }), "runs in the copy's apps");
        input.cwd = Some(a.to_string_lossy().into_owned());
        input.tool_input = json!({ "command": format!("cd '{}' && cd apps && ls", a.display()) });
        assert!(matches!(guard_bash(&input, &ctx, Verdict::Allow), Verdict::Allow), "the copy's apps");
        input.tool_input = json!({ "command": "cd apps && cd ../.. && ls" });
        let Verdict::Deny { reason } = guard_bash(&input, &ctx, Verdict::Allow) else { panic!("leaving the copy is refused") };
        let base = wave_prompt::spec_copies_dir(root, "work");
        assert!(reason.contains(&format!("em {}, a pasta-base das cópias", wave_prompt::shown(&base))), "{reason}");
        std::fs::remove_dir_all(wave_prompt::copies_dir(root)).unwrap();
    }
}
