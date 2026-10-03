//! `conversation_size` — o tamanho da conversa e o que fazer quando ela cresce.
//!
//! Dois ganchos, nenhum que pause o trabalho ou recuse uma chamada:
//!
//! - [`PrecompactNotice`], no evento `PreCompact`: antes de toda compactação
//!   — manual (`/compact`) ou, se a máquina a deixou ligada, a automática —, a
//!   conversa recebe o bloco de retomada
//!   ([`resume_block`](crate::commands::flow::resume::resume_block)), o mesmo
//!   que o início da sessão coloca sozinho depois do resumo. Sem controle de
//!   "já avisado": o próprio `PreCompact` é o degrau, então cada compactação
//!   merece o bloco de novo, e não há como ele ficar velho.
//! - [`SizeNotice`], depois de cada ferramenta, com a conversa que a chamada
//!   mede. **Em quem conduz** (a sessão principal), a conversa que passa de
//!   [`CONDUCTOR_STEP`] tokens recebe, uma vez por degrau, o aviso de limpar
//!   ou compactar, com o bloco de retomada pronto para colar numa janela
//!   limpa; o degrau guardado acompanha a conversa que encolheu, então um
//!   `/compact` de verdade não cala o aviso do próximo degrau. **No agente de
//!   onda** (o subagente cujo primeiro texto é o título de um pedido de onda),
//!   a conversa que passa de [`WAVE_LIMIT`] tokens, sem o resumo da onda
//!   anterior que ele leu, recebe o aviso para terminar a tarefa em curso e
//!   gravar o que falta, e o lembrete a cada [`WAVE_REMINDER_EVERY`] tokens a
//!   mais. O resumo é o salto do tamanho entre a resposta que chama
//!   `run read delivered-<n>` e a resposta seguinte, somado quando o agente lê
//!   mais de um; sem resumo lido, conta a conversa inteira. O agente de onda
//!   nunca recebe o aviso de quem conduz, e quem conduz nunca recebe o da onda.
//!
//! O tamanho é a soma de `input_tokens`, `cache_read_input_tokens` e
//! `cache_creation_input_tokens` do último uso gravado na conversa. A de quem
//! conduz é a de `transcript_path`; a do subagente fica em
//! `<transcript_path sem .jsonl>/subagents/agent-<id>.jsonl`. Sem arquivo
//! legível, sem uso gravado ou sem spec para retomar, nada acontece:
//! [`Verdict::Allow`], porque sem tamanho conhecido não há como decidir.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::wave_prompt::is_wave_title;
use mustard_core::io::transcript::heading_of;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::{translate, ClaudePaths};
use serde_json::Value;

/// O degrau de tamanho da conversa de quem conduz, em tokens: a cada degrau
/// novo, um aviso.
pub(crate) const CONDUCTOR_STEP: u64 = 200_000;

/// O tamanho que a conversa do agente de onda pode ter, em tokens, sem contar
/// o resumo da onda anterior que ele leu: passou, o aviso chega.
pub(crate) const WAVE_LIMIT: u64 = 250_000;

/// De quantos em quantos tokens a mais, depois do aviso, o agente de onda o
/// recebe de novo.
pub(crate) const WAVE_REMINDER_EVERY: u64 = 20_000;

/// A janela do fim do arquivo que a leitura do tamanho olha primeiro, em
/// bytes: a conversa de quem conduz chega a dezenas de megabytes, e o último
/// uso está sempre nas últimas linhas.
const TAIL_BYTES: u64 = 256 * 1024;

/// O aviso, antes de compactar: o bloco de retomada, que volta sozinho
/// depois do resumo. Dispara em toda compactação, sem controle de "já
/// avisado" — o próprio `PreCompact` é o degrau.
pub struct PrecompactNotice;

impl Check for PrecompactNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some(context) = precompact_text(&root, input.session_id.as_deref()) else {
            return Ok(Verdict::Allow);
        };
        Ok(Verdict::Inject { context })
    }
}

/// O aviso antes de compactar: o bloco de retomada da spec atual, dizendo
/// que ele volta sozinho depois do resumo. `None` sem spec atual, sem arquivo
/// de eventos ou com a spec já terminada.
fn precompact_text(root: &Path, session: Option<&str>) -> Option<String> {
    let block = crate::commands::flow::resume::current_block(root, session)?;
    let lang = crate::commands::spec_events::project(root).lang;
    Some(translate("conversation_size.precompact", lang).replace("{block}", &block))
}

/// O aviso de tamanho, depois de cada ferramenta: a quem conduz, o de limpar
/// ou compactar; ao agente de onda, o de parar no limite.
pub struct SizeNotice;

impl Check for SizeNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let context = if input.is_subagent() {
            wave_limit_text(input, &root, ctx.config.language().text_or_default())
        } else {
            conductor_text(input, &root)
        };
        Ok(context.map_or(Verdict::Allow, |context| Verdict::Inject { context }))
    }
}

/// O tamanho de uma conversa: a soma de `input_tokens`,
/// `cache_read_input_tokens` e `cache_creation_input_tokens` do uso `usage`.
fn context_of(usage: &Value) -> u64 {
    let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    field("input_tokens") + field("cache_read_input_tokens") + field("cache_creation_input_tokens")
}

/// O uso gravado numa linha da conversa — em `message.usage`, ou na raiz da
/// linha quando ela já é o uso —, lida só quando a linha o cita.
fn usage_of_line(line: &str) -> Option<(Value, Value)> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let usage = value.get("message").and_then(|m| m.get("usage")).or_else(|| value.get("usage"))?.clone();
    Some((value, usage))
}

/// O tamanho da conversa em `path`: o último uso gravado nela. Lê só o fim do
/// arquivo e, se ali não houver uso, a janela cresce até o arquivo inteiro.
/// `None` sem arquivo legível ou sem nenhum uso gravado.
fn last_context(path: &Path) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut window = TAIL_BYTES;
    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        // A primeira linha da janela pode vir cortada no meio: não é JSON e
        // cai fora na leitura.
        if let Some(found) = text.lines().rev().find_map(usage_of_line) {
            return Some(context_of(&found.1));
        }
        if start == 0 {
            return None;
        }
        window = window.saturating_mul(4);
    }
}

/// O arquivo de estado `name` da sessão `session`, em `.claude/.session/` do
/// projeto `root`. `None` sem sessão de verdade: sem onde guardar o que já
/// foi avisado.
fn mark_path(root: &Path, session: Option<&str>, name: &str) -> Option<PathBuf> {
    let plain = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let session = session.map(str::trim).filter(|session| plain(session) && *session != "unknown")?;
    plain(name).then_some(())?;
    Some(ClaudePaths::for_project(root).ok()?.claude_dir().join(".session").join(session).join(name))
}

/// O número guardado em `path`, ou `None` sem arquivo ou com texto que não é
/// um número.
fn read_mark(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Guarda `value` em `path`. Falhar não derruba o gancho: o aviso volta na
/// chamada seguinte, o que é melhor que calar.
fn write_mark(path: &Path, value: u64) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, value.to_string());
}

/// O aviso de limpar ou compactar a quem conduz: a conversa principal em
/// `input`, sob `root`, passou de um degrau novo de [`CONDUCTOR_STEP`]. O
/// degrau guardado acompanha a conversa que encolheu (depois de um `/compact`
/// de verdade): quando o tamanho atual já está abaixo do degrau avisado, a
/// conta recomeça dali, para o próximo degrau avisar de novo. `None` sem
/// degrau novo ou sem spec para retomar.
fn conductor_text(input: &HookInput, root: &Path) -> Option<String> {
    let tokens = last_context(Path::new(input.transcript_path()?))?;
    let step = tokens / CONDUCTOR_STEP;
    let mark = mark_path(root, input.session_id.as_deref(), "size-step");
    let warned = mark.as_deref().and_then(read_mark).unwrap_or(0);
    if step < warned
        && let Some(mark) = &mark
    {
        write_mark(mark, step);
    }
    if step == 0 || step <= warned {
        return None;
    }
    let block = crate::commands::flow::resume::current_block(root, input.session_id.as_deref())?;
    let lang = crate::commands::spec_events::project(root).lang;
    if let Some(mark) = &mark {
        write_mark(mark, step);
    }
    Some(
        translate("conversation_size.notice", lang)
            .replace("{tokens}", &(step * CONDUCTOR_STEP / 1000).to_string())
            .replace("{block}", &block),
    )
}

/// O que a leitura da conversa de um agente de onda traz.
struct WaveContext {
    /// O tamanho da conversa na última resposta.
    now: u64,
    /// Quanto do tamanho é o resumo da onda anterior que o agente leu.
    summary: u64,
}

/// Se o bloco de conteúdo é uma chamada do terminal que lê o resumo de uma
/// onda entregue (`run read delivered-<n>`).
fn reads_summary(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("tool_use")
        && block.get("name").and_then(Value::as_str) == Some("Bash")
        && block
            .get("input")
            .and_then(|input| input.get("command"))
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains("run read delivered-"))
}

/// O texto da primeira mensagem do usuário numa linha da conversa: corrido,
/// ou o dos blocos de texto juntos. `None` quando a linha não é uma mensagem
/// do usuário.
fn user_text(line: &Value) -> Option<String> {
    let message = line.get("message")?;
    let role = message.get("role").and_then(Value::as_str).or_else(|| line.get("type").and_then(Value::as_str));
    if role != Some("user") {
        return None;
    }
    match message.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            Some(blocks.iter().filter_map(|block| block.get("text")).filter_map(Value::as_str).collect())
        }
        _ => None,
    }
}

/// A leitura da conversa do agente em `path`: `None` quando ela não abre com
/// o título de um pedido de onda no idioma `lang` (um agente qualquer), ou
/// quando ainda não tem nenhum uso gravado. O resumo pesa o salto do tamanho
/// entre a resposta que chama `run read delivered-<n>` e a resposta seguinte.
fn wave_context(path: &Path, lang: Locale) -> Option<WaveContext> {
    use std::io::BufRead;

    let file = std::fs::File::open(path).ok()?;
    let mut opened = false;
    let (mut now, mut summary, mut before) = (None, 0_u64, None::<u64>);
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        if !opened {
            let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
            let Some(first) = user_text(&value) else { continue };
            if !is_wave_title(heading_of(&first), lang) {
                return None;
            }
            opened = true;
            continue;
        }
        let Some((value, usage)) = usage_of_line(&line) else { continue };
        let context = context_of(&usage);
        now = Some(context);
        if let Some(read_at) = before.take() {
            summary += context.saturating_sub(read_at);
        }
        let reads = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|blocks| blocks.iter().any(reads_summary));
        if reads {
            before = Some(context);
        }
    }
    Some(WaveContext { now: now?, summary })
}

/// O aviso ao agente de onda que passou do limite: a conversa do subagente de
/// `input`, sem o resumo que ele leu, passou de [`WAVE_LIMIT`] tokens. Avisa
/// uma vez e lembra a cada [`WAVE_REMINDER_EVERY`] tokens a mais que o último
/// aviso. `None` fora de uma onda, até o limite e entre dois lembretes.
fn wave_limit_text(input: &HookInput, root: &Path, lang: Locale) -> Option<String> {
    let transcript = input.transcript_path()?;
    let name = input.subagent_transcript_name()?;
    let session_dir = transcript.strip_suffix(".jsonl").unwrap_or(transcript);
    let context = wave_context(&Path::new(session_dir).join("subagents").join(&name), lang)?;
    let counted = context.now.saturating_sub(context.summary);
    if counted <= WAVE_LIMIT {
        return None;
    }
    let mark = mark_path(root, input.session_id.as_deref(), &format!("size-limit-{}", name.trim_end_matches(".jsonl")));
    if let Some(last) = mark.as_deref().and_then(read_mark)
        && context.now < last + WAVE_REMINDER_EVERY
    {
        return None;
    }
    if let Some(mark) = &mark {
        write_mark(mark, context.now);
    }
    let thousands = |tokens: u64| (tokens / 1000).to_string();
    Some(
        translate("conversation_size.wave_limit", lang)
            .replace("{now}", &thousands(context.now))
            .replace("{counted}", &thousands(counted))
            .replace("{limit}", &thousands(WAVE_LIMIT)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um projeto instalado, com a spec `spec` aprovada e o checkout parado
    /// na branch dela — o mesmo que a chamada de quem conduz vê.
    fn open_project(spec: &str) -> tempfile::TempDir {
        open_project_in(spec, Locale::PtBr)
    }

    /// [`open_project`] com os textos no idioma `lang`.
    fn open_project_in(spec: &str, lang: Locale) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let text = if lang == Locale::EnUs { "en-US" } else { "pt-BR" };
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, spec);
        crate::commands::spec_events::write::record_open(root, spec, &format!("feature/{spec}"), "dev")
            .expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        dir
    }

    /// Grava a onda 1 da spec `spec`, em `root`, em andamento — o mesmo
    /// pedido que a rodada grava, com o pid deste processo, que segue vivo
    /// durante o teste, para que `waves_in_progress` a conte como rodando, e
    /// a cópia dela em [`copy_of`]. Devolve o número do critério.
    fn seed_running_wave(root: &Path, spec: &str) -> u64 {
        let said =
            crate::shared::spec_state::seed_event(root, spec, "message", serde_json::json!({"author": "user", "text": "o plano"}));
        let crit = crate::shared::spec_state::seed_event(
            root,
            spec,
            "criterion",
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}),
        );
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "wave",
            serde_json::json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}),
        );
        crate::shared::spec_state::seed_event(root, spec, "state", serde_json::json!({"phase": "running", "author": "binary"}));
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "send",
            serde_json::json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary", "copy": copy_of(root, 1),
                "claude_pid": pid, "claude_started": started}),
        );
        crit
    }

    /// A vaga da onda `wave`, como a rodada a grava no envio: a onda n na
    /// n-ésima vaga.
    fn copy_of(root: &Path, wave: u64) -> String {
        let slot = usize::try_from(wave).unwrap() - 1;
        mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::slot_path(root, "x", slot))
    }

    /// A chamada de ferramenta de quem conduz, com a transcrição `transcript`.
    fn conductor_call(root: &Path, transcript: &Path) -> HookInput {
        HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            agent_id: None,
            tool_input: serde_json::json!({ "command": "ls" }),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        }
    }

    /// A transcrição de quem conduz em `transcript`, com o último uso somando
    /// `tokens`.
    fn conductor_transcript_of(transcript: &Path, tokens: u64) {
        let line = serde_json::json!({
            "type": "assistant",
            "message": {"usage": {
                "input_tokens": tokens, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
            }}
        });
        std::fs::write(transcript, format!("{line}\n")).unwrap();
    }

    /// A chamada de ferramenta de quem conduz, depois que ela rodou.
    fn conductor_after_tool(root: &Path, transcript: &Path) -> HookInput {
        HookInput { hook_event_name: Some("PostToolUse".to_string()), ..conductor_call(root, transcript) }
    }

    /// O texto que o gancho de depois da ferramenta injeta para `call`, pelo
    /// despachante e pelo registro. `None` quando nada é injetado.
    fn injected(call: &HookInput) -> Option<String> {
        match crate::dispatch::run_event(Some(Trigger::PostToolUse), call).verdict {
            Verdict::Inject { context } => Some(context),
            Verdict::Allow => None,
            other => panic!("the size notice never blocks: {other:?}"),
        }
    }

    /// Quem conduz é avisado de limpar ou compactar a cada degrau novo de 200
    /// mil tokens, com o bloco de retomada: 199.999 não avisa e 200.000 avisa;
    /// 399.999 não repete o mesmo degrau e 400.000 repete; depois de uma
    /// compactação de verdade (a conversa cai para 70 mil), o degrau de 200 mil
    /// avisa de novo. Nos dois idiomas.
    #[test]
    fn the_conductor_is_told_to_clear_or_compact_once_per_step() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let transcript = root.join("t.jsonl");
            let call = conductor_after_tool(root, &transcript);
            let at = |tokens: u64| {
                conductor_transcript_of(&transcript, tokens);
                injected(&call)
            };

            assert_eq!(at(199_999), None, "{lang:?}: below the step");
            let notice = at(200_000).unwrap_or_else(|| panic!("{lang:?}: the first step warns"));
            for order in ["/clear", "/compact", "200"] {
                assert!(notice.contains(order), "{lang:?}: {order} is missing: {notice}");
            }
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(notice.contains(&block), "{lang:?}: the notice carries the resume block: {notice}");
            assert_eq!(at(399_999), None, "{lang:?}: the same step does not repeat");
            let second = at(400_000).unwrap_or_else(|| panic!("{lang:?}: a new step warns again"));
            assert!(second.contains("400"), "{lang:?}: {second}");
            assert_eq!(at(70_000), None, "{lang:?}: a compacted conversation is below the step");
            assert!(at(200_000).is_some(), "{lang:?}: after a compaction the step warns again");
        }
    }

    /// O aviso de quem conduz nunca chega a um subagente — nem o gasta: a
    /// conversa principal passou de 400 mil tokens, a chamada de um subagente
    /// não recebe nada, e a de quem conduz, logo depois, recebe o aviso por
    /// inteiro. Sem spec para retomar, ninguém recebe aviso.
    #[test]
    fn the_conductor_notice_never_reaches_a_subagent_and_needs_a_spec() {
        let dir = open_project("x");
        let root = dir.path();
        let transcript = root.join("t.jsonl");
        conductor_transcript_of(&transcript, 450_000);
        let subagent = HookInput { agent_id: Some("a1".to_string()), ..conductor_after_tool(root, &transcript) };

        assert_eq!(injected(&subagent), None, "a subagent call is not the conductor's");
        assert!(injected(&conductor_after_tool(root, &transcript)).is_some(), "the subagent call did not use the notice up");

        let bare = tempfile::tempdir().unwrap();
        std::fs::write(bare.path().join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let transcript = bare.path().join("t.jsonl");
        conductor_transcript_of(&transcript, 450_000);
        assert_eq!(injected(&conductor_after_tool(bare.path(), &transcript)), None, "nothing to resume");
    }

    /// Nenhum aviso de tamanho cita valor de compactação automática: nem o de
    /// limpar ou compactar a quem conduz, nem o de antes de compactar. Nos dois
    /// idiomas.
    #[test]
    fn no_size_notice_cites_a_compaction_value() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let transcript = root.join("t.jsonl");
            conductor_transcript_of(&transcript, 200_000);
            let notice = injected(&conductor_after_tool(root, &transcript)).expect("the notice");
            let (_, precompact) = hook_contexts(root, lang);
            for text in [notice, precompact] {
                for value in ["AUTOCOMPACT", "autocompact", "settings.json", "%"] {
                    assert!(!text.contains(value), "{lang:?}: cites a compaction value ({value}): {text}");
                }
            }
        }
    }

    /// Uma resposta do agente com o contexto somando `tokens`, e a ação que
    /// ela pede: nenhuma, a leitura do pedido pelo terminal, a leitura do
    /// resumo de uma onda entregue, ou uma edição.
    fn agent_reply(tokens: u64, action: Option<&str>) -> String {
        let mut content = vec![serde_json::json!({"type": "text", "text": "certo"})];
        match action {
            Some("read") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read request-1 --root /r --spec x"}})),
            Some("summary") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read delivered-5 --root /r --spec x"}})),
            Some("edit") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Edit",
                "input": {"file_path": "/r/a.rs"}})),
            _ => {}
        }
        serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": content, "usage": {
                "input_tokens": 10, "cache_read_input_tokens": tokens - 2_010, "cache_creation_input_tokens": 2_000,
                "output_tokens": 300,
            }}
        })
        .to_string()
    }

    /// A conversa do subagente `a1` da sessão `t`, em `root`: a mensagem de
    /// abertura `opening` e as respostas `replies`.
    fn write_agent(root: &Path, opening: &str, replies: &[String]) {
        let dir = root.join("t").join("subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let first = serde_json::json!({"type": "user", "message": {"role": "user", "content": opening}});
        let mut text = format!("{first}\n");
        for reply in replies {
            text.push_str(reply);
            text.push('\n');
        }
        std::fs::write(dir.join("agent-a1.jsonl"), text).unwrap();
    }

    /// A chamada de ferramenta do subagente `a1`, depois que ela rodou; a
    /// conversa principal fica em `t.jsonl`, abaixo do degrau de quem conduz.
    fn agent_after_tool(root: &Path) -> HookInput {
        let transcript = root.join("t.jsonl");
        conductor_transcript_of(&transcript, 50_000);
        HookInput { agent_id: Some("a1".to_string()), ..conductor_after_tool(root, &transcript) }
    }

    /// O pedido de uma onda, com o título dela, no idioma `lang`.
    fn wave_request(lang: Locale) -> String {
        format!("{}\n\nO pedido.", mustard_core::domain::wave_prompt::wave_title("x", 1, lang))
    }

    /// O agente de onda é avisado uma vez ao passar de 250 mil tokens de
    /// conversa, e de novo a cada 20 mil a mais: 250.000 exatos não avisam e
    /// 250.001 avisam; a chamada seguinte com o mesmo tamanho (outra
    /// ferramenta da mesma resposta) e 19.999 a mais não repetem; 20.000 a
    /// mais repetem. O texto traz a marca do Mustard, o tamanho, o valor sem
    /// o resumo e o limite. Sem resumo lido, conta a conversa inteira — a
    /// leitura do pedido inclusive. Nos dois idiomas.
    #[test]
    fn a_wave_agent_is_warned_once_over_the_limit_and_again_every_twenty_thousand() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let call = agent_after_tool(root);
            let mut replies = vec![agent_reply(30_000, Some("read")), agent_reply(80_000, Some("edit"))];
            let mut at = |tokens: u64| {
                replies.push(agent_reply(tokens, Some("edit")));
                write_agent(root, &wave_request(lang), &replies);
                injected(&call)
            };

            assert_eq!(at(250_000), None, "{lang:?}: exactly the limit does not warn");
            let first = at(250_001).unwrap_or_else(|| panic!("{lang:?}: over the limit warns"));
            assert!(first.starts_with("[Mustard]"), "{lang:?}: the mark comes first: {first}");
            assert!(first.contains("`undone`") && first.contains("250"), "{lang:?}: {first}");
            assert_eq!(at(250_001), None, "{lang:?}: the same size does not repeat");
            assert_eq!(at(270_000), None, "{lang:?}: 19,999 more does not repeat");
            let again = at(270_001).unwrap_or_else(|| panic!("{lang:?}: 20,000 more warns again"));
            assert!(again.contains("270"), "{lang:?}: the size now is told: {again}");
            assert_eq!(at(280_000), None, "{lang:?}: the reminders count from the last warning");
            assert!(at(290_001).is_some(), "{lang:?}: and again 20,000 later");
        }
    }

    /// O resumo da onda anterior que o agente leu sai da conta: o salto do
    /// tamanho entre a resposta que chama `run read delivered-<n>` e a
    /// seguinte. Com um resumo de 100 mil, 350.000 de conversa contam 250.000
    /// e não avisam, e 350.001 avisam, dizendo o tamanho, o valor sem o resumo
    /// e o limite. Dois resumos somam os dois saltos. Ler o pedido
    /// (`run read request-<n>`) não tira nada, e o crescimento depois do
    /// salto conta inteiro.
    #[test]
    fn the_jump_after_reading_a_summary_comes_off_the_count() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let call = agent_after_tool(root);
        let mut replies = vec![agent_reply(40_000, Some("summary")), agent_reply(140_000, Some("edit"))];
        let mut at = |tokens: u64| {
            replies.push(agent_reply(tokens, Some("edit")));
            write_agent(root, &wave_request(Locale::PtBr), &replies);
            injected(&call)
        };
        assert_eq!(at(350_000), None, "350,000 less a 100,000 summary is exactly the limit");
        let warned = at(350_001).expect("one token over the limit without the summary warns");
        assert!(warned.contains("350 mil"), "the size is told: {warned}");
        let again = at(400_000).expect("20,000 more warns again");
        assert!(
            again.contains("400 mil") && again.contains("300 mil") && again.contains("250 mil"),
            "the size, the value without the summary and the limit are told apart: {again}"
        );

        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let call = agent_after_tool(root);
        let mut replies = vec![
            agent_reply(40_000, Some("summary")),
            agent_reply(100_000, Some("summary")),
            agent_reply(150_000, Some("edit")),
        ];
        let mut at = |tokens: u64| {
            replies.push(agent_reply(tokens, Some("edit")));
            write_agent(root, &wave_request(Locale::PtBr), &replies);
            injected(&call)
        };
        assert_eq!(at(360_000), None, "two summaries, 60,000 and 50,000, come off: 250,000 left");
        assert!(at(360_001).is_some(), "and one more token warns");

        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let call = agent_after_tool(root);
        let replies = vec![agent_reply(30_000, Some("read")), agent_reply(280_000, Some("edit"))];
        write_agent(root, &wave_request(Locale::PtBr), &replies);
        assert!(injected(&call).is_some(), "reading the request is not reading a summary: the whole size counts");
    }

    /// O aviso do limite fica calado onde não é de uma onda: na sessão
    /// principal, que só tem o aviso de quem conduz (e abaixo do degrau dele,
    /// nada), num subagente cujo primeiro texto não é o título de um pedido de
    /// onda — por maior que a conversa dele esteja —, e num pedido de onda no
    /// idioma que o projeto não usa.
    #[test]
    fn the_limit_notice_is_quiet_outside_a_wave() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let replies = vec![agent_reply(40_000, Some("edit")), agent_reply(300_000, Some("edit"))];

        write_agent(root, &wave_request(Locale::PtBr), &replies);
        let main = conductor_after_tool(root, &root.join("t.jsonl"));
        conductor_transcript_of(&root.join("t.jsonl"), 150_000);
        assert_eq!(injected(&main), None, "the main session is not a wave agent");

        for opening in ["Explore o repositório e conte os arquivos.", "# x — wave 1\n\nThe request.", "olá\n# x — onda 1"] {
            write_agent(root, opening, &replies);
            assert_eq!(injected(&agent_after_tool(root)), None, "{opening:?} is not a wave request");
        }
        write_agent(root, &wave_request(Locale::PtBr), &replies);
        assert!(injected(&agent_after_tool(root)).is_some(), "the same conversation, opened by a wave request, warns");
    }

    /// O aviso de compactar chega pelo gancho de `PreCompact`, com o bloco de
    /// retomada — spec, fase e o próximo passo —, com onda rodando ou sem: a
    /// linha das ondas em andamento é uma parte do bloco, não um substituto.
    /// E nenhuma chamada de ferramenta é mais recusada por tamanho: o mesmo
    /// registro inteiro, com uma transcrição de 200 mil tokens (o antigo
    /// degrau), deixa passar um `PreToolUse` comum.
    #[test]
    fn compaction_warning_arrives_in_the_hook_and_nobody_else_is_blocked_by_size() {
        let dir = open_project("x");
        let root = dir.path();

        // O caminho de verdade: o evento `PreCompact` inteiro, pelo
        // despachante e pelo registro — não a função auxiliar direto — é
        // quem precisa injetar o bloco.
        let precompact = HookInput {
            hook_event_name: Some("PreCompact".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("antes de compactar, o bloco de retomada é injetado");
        };
        assert!(context.contains('x'), "o bloco traz a spec: {context}");
        assert!(context.contains("fase"), "o bloco traz a fase: {context}");

        // Com uma onda em andamento, o bloco continua saindo, com a onda
        // citada.
        seed_running_wave(root, "x");
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("com onda em andamento, o bloco continua saindo");
        };
        assert!(context.contains("fase"), "o bloco não vira só a frase das ondas no ar: {context}");
        assert!(context.contains('1'), "o bloco cita a onda em andamento: {context}");

        // Uma transcrição de 200 mil tokens — o antigo degrau — não recusa
        // mais nenhuma chamada de ferramenta de quem conduz: o mesmo
        // despachante, no `PreToolUse`, deixa passar.
        let transcript = root.join("t.jsonl");
        std::fs::write(
            &transcript,
            serde_json::json!({
                "message": {"usage": {
                    "input_tokens": 200_000, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
                }}
            })
            .to_string(),
        )
        .unwrap();
        let outcome = crate::dispatch::run_event(Some(Trigger::PreToolUse), &conductor_call(root, &transcript));
        assert_eq!(outcome.verdict, Verdict::Allow, "sem degrau de tokens, nada barra mais a chamada");
    }

    /// Grava na spec `x`, em `root`, o evento `event_type` com `body` e
    /// devolve o número dele.
    fn seed(root: &Path, event_type: &str, body: serde_json::Value) -> u64 {
        crate::shared::spec_state::seed_event(root, "x", event_type, body)
    }

    /// Grava a onda `n` da spec `x` no plano, com o critério `crit`.
    fn plan_wave(root: &Path, n: u64, crit: u64, said: u64) {
        seed(
            root,
            "wave",
            serde_json::json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "x",
                "origin": said}),
        );
    }

    /// Grava a volta da onda `n` pedindo mudança de plano, à espera da rodada.
    fn seed_replan_return(root: &Path, n: u64) {
        seed(
            root,
            "delivered",
            serde_json::json!({"wave": n, "text": "O plano não fecha.", "replan": format!("Dividir a onda {n}."), "changes_decision": "A ordem escolhida.",
                "returned": true, "author": "wave"}),
        );
    }

    /// Grava a última rodada que deu certo e, depois dela, `count` decisões;
    /// devolve o código de cada uma, na ordem.
    fn seed_decisions_after_round(root: &Path, said: u64, count: usize) -> Vec<String> {
        seed(root, "call", serde_json::json!({"command": "round", "ms": 3, "result": "ok", "author": "binary"}));
        let ids: Vec<u64> = (0..count)
            .map(|at| {
                seed(
                    root,
                    "decision",
                    serde_json::json!({"text": format!("Decisão {at}."), "keys": ["retomada"],
                        "why": "o usuário pediu", "origin": said}),
                )
            })
            .collect();
        let codes = read_log(root).codes();
        ids.iter().map(|id| codes[id].clone()).collect()
    }

    /// O arquivo de eventos da spec `x`, em `root`.
    fn read_log(root: &Path) -> mustard_core::domain::spec_events::SpecLog {
        mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
            .unwrap()
            .unwrap()
    }

    /// O texto que o início da sessão depois da compactação e o aviso antes
    /// de compactar injetam em `root`, pelo evento do gancho, nessa ordem.
    fn hook_contexts(root: &Path, lang: Locale) -> (String, String) {
        let event = |name: &str, trigger: Trigger, raw: serde_json::Value| {
            let input = HookInput {
                hook_event_name: Some(name.to_string()),
                session_id: Some("s1".to_string()),
                cwd: Some(root.to_string_lossy().into_owned()),
                raw,
                ..HookInput::default()
            };
            match crate::dispatch::run_event(Some(trigger), &input).verdict {
                Verdict::Inject { context } => context,
                other => panic!("{lang:?}: {name} injects nothing: {other:?}"),
            }
        };
        (
            event("SessionStart", Trigger::SessionStart, serde_json::json!({"source": "compact"})),
            event("PreCompact", Trigger::PreCompact, serde_json::json!({})),
        )
    }

    /// O que o bloco em `context` mostra na vaga `slot` do modelo do bloco em
    /// `lang`: o trecho entre o rótulo que antecede a vaga e o texto que a
    /// segue até a vaga seguinte.
    fn slot_value<'a>(context: &'a str, lang: Locale, slot: &str) -> &'a str {
        let template = translate("conversation_size.block", lang);
        let at = template.find(slot).expect("the slot is in the template");
        let before = &template[..at];
        let label = &before[before.rfind('}').map_or(0, |end| end + 1)..];
        let after = &template[at + slot.len()..];
        let tail = &after[..after.find('{').unwrap_or(after.len())];
        let start = context.find(label).unwrap_or_else(|| panic!("{lang:?}: no {slot} label: {context}")) + label.len();
        let len = context[start..].find(tail).unwrap_or_else(|| panic!("{lang:?}: no end of {slot}: {context}"));
        &context[start..start + len]
    }

    /// A lista `items` cortada depois de `kept` itens, como o bloco a mostra:
    /// os primeiros e quantos ficaram de fora.
    fn cut_list(items: &[String], kept: usize, lang: Locale) -> String {
        let mut shown = items[..kept].to_vec();
        if kept < items.len() {
            shown.push(translate("conversation_size.more", lang).replace("{count}", &(items.len() - kept).to_string()));
        }
        shown.join(", ")
    }

    /// A lista `items` que o bloco `block` mostra na vaga `slot` cortada no
    /// teto: guarda os primeiros itens e diz quantos ficaram de fora, e um item
    /// a mais já não caberia. Devolve quantos ficaram.
    fn assert_cut_at_the_cap(block: &str, lang: Locale, slot: &str, items: &[String]) -> usize {
        use crate::hooks::session::session_start_inject::MAX_BYTES;
        let shown = slot_value(block, lang, slot);
        let kept = (0..items.len())
            .find(|kept| shown == cut_list(items, *kept, lang))
            .unwrap_or_else(|| panic!("{lang:?}: {slot} is not the first items and how many were left out: {shown}"));
        let one_more = block.replacen(shown, &cut_list(items, kept + 1, lang), 1);
        assert!(block.len() <= MAX_BYTES, "{lang:?}: {} bytes", block.len());
        assert!(one_more.len() > MAX_BYTES, "{lang:?}: {slot} keeps {kept}, but {} bytes still fit", one_more.len());
        kept
    }

    /// Depois da compactação, o início da sessão coloca sozinho o bloco de
    /// retomada, pelo evento do gancho: quantas ondas foram entregues, a onda em andamento
    /// com a pasta da cópia dela, a onda cuja volta espera a rodada pedindo
    /// mudança de plano ainda sem o clique, a onda parada no limite de
    /// consertos, e o código da decisão gravada depois da última rodada — e
    /// não o da gravada antes dela. O aviso antes de compactar traz o mesmo
    /// bloco e não pede para colá-lo. Nos dois idiomas, e dentro do teto do
    /// início da sessão.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block() {
        use crate::hooks::session::session_start_inject::MAX_BYTES;
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let crit = seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "mais ondas"}));
            plan_wave(root, 2, crit, said);
            let (pid, started) = crate::commands::flow::stuck::this_process();
            seed(root, "send", json!({"wave": 2, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary", "copy": copy_of(root, 2),
                "claude_pid": pid, "claude_started": started}));
            seed_replan_return(root, 2);
            // A onda 5 também pede mudança de plano, mas não troca decisão do
            // usuário: não espera clique nenhum, e o bloco a lista sem aviso.
            plan_wave(root, 5, crit, said);
            seed(root, "delivered", json!({"wave": 5, "text": "Parei.", "replan": "Trocar o nome da função.",
                "returned": true, "author": "wave"}));
            // A onda 3 entregue, e a 4 reprovada uma vez e depois de cada uma
            // das duas rodadas de conserto: parada no limite.
            plan_wave(root, 3, crit, said);
            seed(root, "delivered", json!({"wave": 3, "text": "Pronta.", "files": ["src/tres.rs"], "author": "wave"}));
            plan_wave(root, 6, crit, said);
            seed(root, "delivered", json!({"wave": 6, "text": "Pronta.", "files": ["src/seis.rs"], "author": "wave"}));
            plan_wave(root, 4, crit, said);
            for _ in 0..3 {
                crate::shared::spec_state::seed_verdict(root, "x", 4, "rejected", crit);
            }
            let before = seed(root, "decision", json!({"text": "Antes da rodada.", "keys": ["retomada"],
                "why": "o usuário pediu", "origin": said}));
            let after = seed_decisions_after_round(root, said, 1);
            let before = read_log(root).codes()[&before].clone();

            let (started, precompact) = hook_contexts(root, lang);

            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            let replan = translate("conversation_size.replan", lang).replace("{wave}", "2");
            for (moment, context) in [("session start", &started), ("precompact", &precompact)] {
                assert_eq!(slot_value(context, lang, "{delivered}"), "2", "{lang:?} {moment}: how many waves were delivered");
                assert_eq!(slot_value(context, lang, "{running}"), running, "{lang:?} {moment}: the wave in flight");
                assert_eq!(
                    slot_value(context, lang, "{returned}"),
                    format!("{replan}, 5"),
                    "{lang:?} {moment}: the return that swaps a decision asks for the click, the other does not"
                );
                assert_eq!(slot_value(context, lang, "{stuck}"), "4", "{lang:?} {moment}: the wave at the fix limit");
                assert_eq!(slot_value(context, lang, "{recorded}"), after[0], "{lang:?} {moment}: after the round");
                assert!(!context.contains(&before), "{lang:?} {moment}: the decision before the round: {context}");
                assert!(context.contains("mustard-rt run round --spec x"), "{lang:?} {moment}: {context}");
            }
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            assert!(block.len() <= MAX_BYTES, "{lang:?}: {} bytes", block.len());
            for paste in ["cole", "colar", "paste"] {
                assert!(!precompact.contains(paste), "{lang:?}: the notice still asks to paste: {precompact}");
            }
        }
    }

    /// Numa obra com muitas ondas entregues, o bloco de retomada diz só
    /// quantas foram, e não o número de cada uma: o passo seguinte vem das que
    /// faltam e do próximo comando, e a lista das entregues crescia com a obra
    /// e ocupava mais da metade do bloco. O início da sessão depois da
    /// compactação e o aviso de compactar trazem o mesmo bloco curto. Em
    /// português; o inglês do mesmo bloco já é provado em
    /// `after_compaction_the_session_start_brings_the_work_block`.
    #[test]
    fn work_block_counts_the_delivered_waves_instead_of_listing_each_one() {
        use serde_json::json;

        let lang = Locale::PtBr;
        let dir = open_project_in("x", lang);
        let root = dir.path();
        let crit = seed_running_wave(root, "x");
        let said = seed(root, "message", json!({"author": "user", "text": "obra longa"}));
        let mut one_delivered = 0;
        for n in 2..=12 {
            plan_wave(root, n, crit, said);
            seed(root, "delivered", json!({"wave": n, "text": "Pronta.", "files": ["src/a.rs"], "author": "wave"}));
            if n == 2 {
                one_delivered = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block").len();
            }
        }

        let (started, precompact) = hook_contexts(root, lang);
        for (moment, context) in [("session start", &started), ("precompact", &precompact)] {
            assert_eq!(slot_value(context, lang, "{delivered}"), "11", "{moment}: the count: {context}");
            assert!(!context.contains("2, 3, 4"), "{moment}: the delivered waves are listed: {context}");
        }
        let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
        assert!(block.len() <= one_delivered + 1, "the block grew with the delivered waves: {one_delivered} -> {} bytes: {block}", block.len());
    }

    /// Com mais códigos gravados depois da última rodada do que cabem no teto
    /// do início da sessão, o bloco que o gancho injeta mostra os primeiros
    /// que cabem — um a mais já não caberia — e quantos ficaram de fora; o
    /// resto do bloco fica inteiro. Nos dois idiomas.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block_with_the_codes_cut_at_the_cap() {
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "muitas decisões"}));
            let codes = seed_decisions_after_round(root, said, 250);

            let (started, precompact) = hook_contexts(root, lang);
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            let kept = assert_cut_at_the_cap(&block, lang, "{recorded}", &codes);
            assert!(kept > 0, "{lang:?}: the codes that fit are shown: {block}");
            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            assert_eq!(slot_value(&block, lang, "{running}"), running, "{lang:?}: the wave in flight stays whole");
        }
    }

    /// Quando nem a lista de códigos vazia faz o bloco caber, as outras
    /// partes também encolhem até ele caber no teto: os códigos saem todos
    /// primeiro, e das voltas à espera da rodada ficam as primeiras que cabem —
    /// uma a mais já não caberia — e quantas ficaram de fora. A onda em
    /// andamento, que cede por último, fica inteira. Nos dois idiomas.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block_cutting_the_other_parts_until_it_fits() {
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let crit = seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "muitas voltas"}));
            let waves: Vec<u64> = (2..62).collect();
            for n in &waves {
                plan_wave(root, *n, crit, said);
                seed_replan_return(root, *n);
            }
            let codes = seed_decisions_after_round(root, said, 3);

            let (started, precompact) = hook_contexts(root, lang);
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            assert_eq!(slot_value(&block, lang, "{recorded}"), cut_list(&codes, 0, lang), "{lang:?}: {block}");
            let returns: Vec<String> = waves
                .iter()
                .map(|n| translate("conversation_size.replan", lang).replace("{wave}", &n.to_string()))
                .collect();
            let kept = assert_cut_at_the_cap(&block, lang, "{returned}", &returns);
            assert!(kept > 0, "{lang:?}: the returns that fit are shown: {block}");
            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            assert_eq!(slot_value(&block, lang, "{running}"), running, "{lang:?}: the wave in flight stays whole");
            assert!(block.contains("mustard-rt run round --spec x"), "{lang:?}: the next command: {block}");
        }
    }
}
