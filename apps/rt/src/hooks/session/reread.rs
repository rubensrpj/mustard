//! `reread` — o aviso ao agente de onda que vai ler de novo o que já leu.
//!
//! O Claude Code nunca tira da conversa o que o agente já leu: a segunda
//! leitura das mesmas linhas é uma cópia do que ele já tem, e é relida em todo
//! passo seguinte. Este gancho anota cada leitura do agente de onda e, quando
//! ele pede de novo linhas que leu há pouco e que não mudaram, responde no
//! lugar da leitura: diz que ele já leu essas linhas e que elas estão acima,
//! na conversa.
//!
//! - **Quem.** Só o subagente cujo primeiro texto é o título de um pedido de
//!   onda ([`is_wave_title`]). Quem conduz e o subagente de outro tipo leem
//!   sem aviso e sem registro.
//! - **O que se anota.** Depois de uma leitura que rodou — a ferramenta `Read`
//!   ou o terminal ([`terminal_read`]) —, o registro do agente
//!   (`reads-agent-<id>`, em `.claude/.session/<sessão>/`) ganha o arquivo, a
//!   faixa de linhas, a marca de cada linha lida e o tamanho da conversa do
//!   agente naquela hora. A leitura recusada ou que falhou não roda, e por
//!   isso não é anotada. Cada agente tem o registro dele; agente novo começa
//!   vazio.
//! - **Quando recusa.** Antes de uma leitura, só se as três condições valem:
//!   toda a faixa pedida já foi lida; cada linha tem a mesma marca de agora; a
//!   leitura anterior está nos últimos [`REREAD_WINDOW`] tokens da conversa.
//!   Senão a leitura passa, sem aviso. A faixa só em parte lida passa, e a
//!   conversa que encolheu (compactada) também: o que ela leu saiu dali.
//! - **A saída.** O mesmo pedido repetido logo depois da recusa passa. Qualquer
//!   outra leitura que venha no lugar dele apaga a recusa.
//!
//! É um [`Check`] antes da ferramenta (a recusa) e um [`Observer`] depois dela
//! (a anotação). O aviso sai de `conversation_size.reread`, nos dois idiomas.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::wave_prompt::is_wave_title;
use mustard_core::io::fs;
use mustard_core::io::transcript::heading_of;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::translate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::conversation_size::{agent_transcript, last_context, mark_path, user_text};
use crate::hooks::bash::terminal_read::{terminal_read, Span};

/// Até quantos tokens atrás a leitura anterior ainda está "acima" na
/// conversa: mais longe que isto, a leitura passa.
pub(crate) const REREAD_WINDOW: u64 = 50_000;

/// As linhas que a ferramenta `Read` traz quando o pedido não diz quantas.
const READ_DEFAULT_LINES: u64 = 2000;

/// O que o terminal mostra de uma saída antes de cortá-la, em bytes: o que
/// passa disto não chega à conversa, e não conta como lido.
const TERMINAL_OUTPUT_BYTES: usize = 30_000;

/// O gancho de releitura.
pub struct RereadGuard;

impl Check for RereadGuard {
    /// Recusa, antes da ferramenta, a leitura de linhas que o agente de onda
    /// já tem acima na conversa; qualquer outro evento passa.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        Ok(refusal(input, ctx).map_or(Verdict::Allow, |reason| Verdict::Deny { reason }))
    }
}

impl Observer for RereadGuard {
    /// Anota, depois da ferramenta, a leitura que rodou.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger == Some(Trigger::PostToolUse) {
            record(input, ctx);
        }
    }
}

/// O que a chamada lê, antes de olhar o arquivo.
struct Request {
    /// O arquivo como o agente o escreveu.
    typed: String,
    /// O caminho do arquivo, partindo da pasta da chamada.
    path: PathBuf,
    /// As linhas pedidas.
    span: Span,
    /// A leitura é pelo terminal, que corta a saída longa.
    terminal: bool,
}

/// As linhas de um arquivo que a leitura traz, com a marca de cada uma.
struct Seen {
    /// O caminho do arquivo, sem atalho, que é a chave do registro.
    key: String,
    /// A primeira linha, contada de 1.
    from: u64,
    /// A marca de cada linha, a partir de `from`.
    marks: Vec<u32>,
}

impl Seen {
    /// A última linha.
    fn to(&self) -> u64 {
        self.from + self.marks.len() as u64 - 1
    }
}

/// A chamada de um agente de onda que lê um arquivo, com o que se precisa
/// para julgá-la ou anotá-la.
struct Call {
    request: Request,
    seen: Seen,
    /// O tamanho da conversa do agente nesta chamada.
    now: u64,
    /// O arquivo do registro do agente.
    log: PathBuf,
    lang: Locale,
}

/// A recusa que o aviso responde: o arquivo e a faixa pedidos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Refusal {
    key: String,
    from: u64,
    to: u64,
}

/// Uma leitura anotada: onde começa, quanta conversa havia e a marca de cada
/// linha.
#[derive(Debug, Serialize, Deserialize)]
struct LineRead {
    from: u64,
    context: u64,
    marks: Vec<u32>,
}

impl LineRead {
    /// Se a leitura trouxe a linha `line`.
    fn covers(&self, line: u64) -> bool {
        line >= self.from && line - self.from < self.marks.len() as u64
    }
}

/// O registro de um agente: a última recusa e as leituras ainda dentro da
/// janela, por arquivo, da mais antiga à mais nova.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Log {
    #[serde(default)]
    refused: Option<Refusal>,
    #[serde(default)]
    reads: BTreeMap<String, Vec<LineRead>>,
}

impl Log {
    /// O registro em `path`; vazio sem arquivo ou com texto que não se lê.
    fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Guarda o registro. Falhar não derruba o gancho: sem registro, a leitura
    /// passa, o que é melhor que recusar sem saber.
    fn save(&self, path: &Path) {
        let Ok(text) = serde_json::to_string(self) else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = fs::write_atomic(path, text.as_bytes());
    }

    /// Há quantos tokens as linhas de `seen` foram lidas, quando todas foram,
    /// com a mesma marca de agora e dentro da janela: o da leitura mais
    /// antiga entre as que as trouxeram. `None` em qualquer outro caso.
    fn reread(&self, seen: &Seen, now: u64) -> Option<u64> {
        let reads = self.reads.get(&seen.key)?;
        let mut oldest = 0;
        for (at, mark) in seen.marks.iter().enumerate() {
            let line = seen.from + at as u64;
            let read = reads.iter().rev().find(|read| read.covers(line))?;
            if read.marks[(line - read.from) as usize] != *mark {
                return None;
            }
            let ago = now.checked_sub(read.context)?;
            if ago > REREAD_WINDOW {
                return None;
            }
            oldest = oldest.max(ago);
        }
        Some(oldest)
    }

    /// Anota a leitura de `seen` com a conversa em `now`, e deixa cair as que
    /// já saíram da janela ou que a conversa encolhida levou embora.
    fn record(&mut self, seen: Seen, now: u64) {
        for reads in self.reads.values_mut() {
            reads.retain(|read| now.checked_sub(read.context).is_some_and(|ago| ago <= REREAD_WINDOW));
        }
        self.reads.retain(|_, reads| !reads.is_empty());
        self.reads.entry(seen.key).or_default().push(LineRead { from: seen.from, context: now, marks: seen.marks });
    }
}

/// A marca de uma linha: o FNV-1a de 32 bits dos bytes dela.
fn mark(line: &str) -> u32 {
    line.bytes().fold(0x811c_9dc5_u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}

/// Se a conversa em `path` abre com o título de um pedido de onda no idioma
/// `lang`: a primeira mensagem do usuário.
fn is_wave_agent(path: &Path, lang: Locale) -> bool {
    let Ok(file) = std::fs::File::open(path) else { return false };
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { return false };
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(first) = user_text(&value) {
            return is_wave_title(heading_of(&first), lang);
        }
    }
    false
}

/// O que a chamada de `input` lê: a ferramenta `Read` ou um comando do
/// terminal que só mostra linhas de um arquivo. `None` em qualquer outra.
fn request_of(input: &HookInput) -> Option<Request> {
    let cwd = Path::new(input.cwd.as_deref().filter(|cwd| !cwd.is_empty())?);
    let tool = &input.tool_input;
    match input.tool_name.as_deref()? {
        "Read" => {
            if tool.get("pages").is_some() {
                return None;
            }
            let typed = input.file_path()?;
            let from = tool.get("offset").and_then(Value::as_u64).unwrap_or(1).max(1);
            let count = tool.get("limit").and_then(Value::as_u64).filter(|count| *count > 0).unwrap_or(READ_DEFAULT_LINES);
            Some(Request { path: cwd.join(&typed), typed, span: Span::Lines { from, to: Some(from + count - 1) }, terminal: false })
        }
        "Bash" => {
            let read = terminal_read(tool.get("command")?.as_str()?, cwd)?;
            Some(Request { typed: read.typed, path: read.file, span: read.span, terminal: true })
        }
        _ => None,
    }
}

/// As linhas que a ferramenta `Read` diz ter trazido (`startLine` e
/// `numLines` do resultado), que valem mais que o pedido: o portão de escrita
/// corta a leitura inteira antes dos testes. `None` sem esse dado.
fn shown_lines(input: &HookInput) -> Option<(u64, u64)> {
    let file = input.raw.get("tool_response")?.get("file")?;
    let start = file.get("startLine")?.as_u64()?;
    let count = file.get("numLines")?.as_u64()?;
    (start > 0 && count > 0).then_some((start, count))
}

/// O trecho de `lines` que cabe na saída do terminal, ao menos a primeira.
fn within_output_cap<'a, 'b>(lines: &'b [&'a str]) -> &'b [&'a str] {
    let mut bytes = 0;
    let end = lines
        .iter()
        .position(|line| {
            bytes += line.len() + 1;
            bytes > TERMINAL_OUTPUT_BYTES
        })
        .unwrap_or(lines.len());
    &lines[..end.max(1)]
}

/// As linhas do arquivo que a leitura traz agora, com a marca de cada uma.
/// `shown` troca a faixa pedida pela que a ferramenta disse ter trazido.
/// `None` com arquivo que não se lê como texto e com faixa fora dele.
fn seen_by(request: &Request, shown: Option<(u64, u64)>) -> Option<Seen> {
    let content = std::fs::read_to_string(&request.path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len() as u64;
    let span = shown.map_or(request.span, |(start, count)| Span::Lines { from: start, to: Some(start + count - 1) });
    let (from, to) = match span {
        Span::Lines { from, to } => (from, to.map_or(total, |to| to.min(total))),
        Span::Last(count) => (total.saturating_sub(count) + 1, total),
    };
    if from == 0 || from > to {
        return None;
    }
    let mut taken = &lines[(from - 1) as usize..to as usize];
    if request.terminal {
        taken = within_output_cap(taken);
    }
    let key = std::fs::canonicalize(&request.path).unwrap_or_else(|_| request.path.clone());
    Some(Seen { key: key.to_string_lossy().into_owned(), from, marks: taken.iter().map(|line| mark(line)).collect() })
}

/// A chamada de `input` quando ela é de um agente de onda e lê um arquivo de
/// texto; `after` diz que a ferramenta já rodou. `None` em todo o resto.
fn call_of(input: &HookInput, ctx: &Ctx, after: bool) -> Option<Call> {
    if !input.is_subagent() {
        return None;
    }
    let request = request_of(input)?;
    let transcript = agent_transcript(input)?;
    let lang = ctx.config.language().text_or_default();
    if !is_wave_agent(&transcript, lang) {
        return None;
    }
    let seen = seen_by(&request, after.then(|| shown_lines(input)).flatten())?;
    let now = last_context(&transcript)?;
    let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
    let name = input.subagent_transcript_name()?;
    let log = mark_path(&root, input.session_id.as_deref(), &format!("reads-{}", name.trim_end_matches(".jsonl")))?;
    Some(Call { request, seen, now, log, lang })
}

/// O aviso de que a leitura repete o que o agente já tem acima, ou `None`
/// quando ela passa. Guarda a recusa para a saída: o mesmo pedido repetido em
/// seguida passa.
fn refusal(input: &HookInput, ctx: &Ctx) -> Option<String> {
    let call = call_of(input, ctx, false)?;
    let mut log = Log::load(&call.log);
    let asked = Refusal { key: call.seen.key.clone(), from: call.seen.from, to: call.seen.to() };
    let before = log.refused.take();
    let repeated = before.as_ref() == Some(&asked);
    let ago = if repeated { None } else { log.reread(&call.seen, call.now) };
    let refused = ago.map(|_| asked);
    if before.is_some() || refused.is_some() {
        log.refused = refused;
        log.save(&call.log);
    }
    let thousands = ago?.div_ceil(1_000).max(1);
    Some(
        translate("conversation_size.reread", call.lang)
            .replace("{file}", &call.request.typed)
            .replace("{from}", &call.seen.from.to_string())
            .replace("{to}", &call.seen.to().to_string())
            .replace("{ago}", &thousands.to_string()),
    )
}

/// Anota a leitura que o agente de onda acabou de fazer.
fn record(input: &HookInput, ctx: &Ctx) {
    let Some(call) = call_of(input, ctx, true) else { return };
    let mut log = Log::load(&call.log);
    log.record(call.seen, call.now);
    log.save(&call.log);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::run_event;
    use serde_json::json;

    /// Um projeto com o arquivo `notes.txt` de cem linhas (`line 1` a
    /// `line 100`) e agentes de onda que leem dele.
    struct Scene {
        dir: tempfile::TempDir,
        lang: Locale,
    }

    impl Scene {
        fn new(lang: Locale) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let text = if lang == Locale::EnUs { "en-US" } else { "pt-BR" };
            std::fs::write(dir.path().join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
            let scene = Self { dir, lang };
            scene.write_notes(|line| format!("line {line}"));
            scene
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        /// Reescreve `notes.txt` com o texto que `text` dá a cada linha.
        fn write_notes(&self, text: impl Fn(u32) -> String) {
            let body: String = (1..=100).map(|line| format!("{}\n", text(line))).collect();
            std::fs::write(self.root().join("notes.txt"), body).unwrap();
        }

        /// A conversa do agente `id`, aberta com o pedido `opening`, no
        /// tamanho `tokens`.
        fn agent_says(&self, id: &str, opening: &str, tokens: u64) {
            let dir = self.root().join("t").join("subagents");
            std::fs::create_dir_all(&dir).unwrap();
            let first = json!({"type": "user", "message": {"role": "user", "content": opening}});
            let reply = json!({"type": "assistant", "message": {"role": "assistant", "content": [], "usage": {
                "input_tokens": 10, "cache_read_input_tokens": tokens - 10, "cache_creation_input_tokens": 0}}});
            std::fs::write(dir.join(format!("agent-{id}.jsonl")), format!("{first}\n{reply}\n")).unwrap();
            std::fs::write(self.root().join("t.jsonl"), "").unwrap();
        }

        /// O agente de onda `a1` com a conversa em `tokens`.
        fn wave_at(&self, tokens: u64) {
            self.wave_agent_at("a1", tokens);
        }

        fn wave_agent_at(&self, id: &str, tokens: u64) {
            let title = mustard_core::domain::wave_prompt::wave_title("x", 1, self.lang);
            self.agent_says(id, &format!("{title}\n\nO pedido."), tokens);
        }

        /// A chamada de `tool` do agente `agent` (`None`: quem conduz), no
        /// evento `event`.
        fn call(&self, event: &str, agent: Option<&str>, tool: &str, input: Value) -> HookInput {
            HookInput {
                hook_event_name: Some(event.to_string()),
                tool_name: Some(tool.to_string()),
                session_id: Some("s1".to_string()),
                cwd: Some(self.root().to_string_lossy().into_owned()),
                agent_id: agent.map(str::to_string),
                tool_input: input,
                raw: json!({ "transcript_path": self.root().join("t.jsonl").to_string_lossy() }),
                ..HookInput::default()
            }
        }

        fn read_input(&self, offset: u64, limit: u64) -> Value {
            json!({ "file_path": self.root().join("notes.txt").to_string_lossy(), "offset": offset, "limit": limit })
        }

        /// A leitura das linhas `from` a `to` pelo agente `a1`, antes da ferramenta.
        fn read(&self, from: u64, to: u64) -> HookInput {
            self.call("PreToolUse", Some("a1"), "Read", self.read_input(from, to - from + 1))
        }

        /// O comando do terminal do agente `a1`, antes da ferramenta.
        fn bash(&self, command: &str) -> HookInput {
            self.call("PreToolUse", Some("a1"), "Bash", json!({ "command": command }))
        }

        /// A mesma chamada, depois que a ferramenta rodou.
        fn after(&self, call: &HookInput) -> HookInput {
            HookInput { hook_event_name: Some("PostToolUse".to_string()), ..call.clone() }
        }
    }

    /// A recusa que o gancho dá à chamada `call`, pelo despachante e pelo
    /// registro; `None` quando a leitura passa.
    fn refused(call: &HookInput) -> Option<String> {
        match run_event(Some(Trigger::PreToolUse), call).verdict {
            Verdict::Deny { reason } => Some(reason),
            Verdict::Allow => None,
            other => panic!("the guard only refuses or lets pass: {other:?}"),
        }
    }

    /// A ferramenta rodou: o despachante do depois da ferramenta anota.
    fn ran(call: &HookInput) {
        let _ = run_event(Some(Trigger::PostToolUse), &call.clone());
    }

    /// O agente lê as linhas `from` a `to` com a conversa em `tokens`: a
    /// leitura passa e é anotada, como o Claude Code a roda.
    fn read_once(scene: &Scene, from: u64, to: u64, tokens: u64) {
        scene.wave_at(tokens);
        let call = scene.read(from, to);
        assert_eq!(refused(&call), None, "the first read of {from}-{to} passes");
        ran(&scene.after(&call));
    }

    /// Reler as mesmas linhas, sem mudança e dentro de 50 mil tokens, é
    /// recusado com o aviso: o arquivo, a faixa, há quantos tokens a leitura
    /// foi feita e o que fazer. Nos dois idiomas.
    #[test]
    fn rereading_unchanged_lines_within_the_window_is_refused_with_the_notice() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let scene = Scene::new(lang);
            read_once(&scene, 1, 50, 10_000);
            scene.wave_at(30_000);
            let notice = refused(&scene.read(1, 50)).unwrap_or_else(|| panic!("{lang:?}: the reread is refused"));
            let (ago, again) = if lang == Locale::EnUs {
                ("20 thousand tokens ago", "repeat the same request")
            } else {
                ("20 mil tokens", "repita o mesmo pedido")
            };
            for part in ["[Mustard]", "notes.txt", "1", "50", ago, again] {
                assert!(notice.contains(part), "{lang:?}: `{part}` is missing: {notice}");
            }
            assert!(refused(&scene.read(10, 20)).is_some(), "{lang:?}: lines inside the range read are refused too");
        }
    }

    /// A leitura de 50.000 tokens atrás ainda é acima na conversa e é
    /// recusada; com 50.001 passa. Sem o 50.001 passar, o limite não existe.
    #[test]
    fn a_read_more_than_fifty_thousand_tokens_back_passes() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(60_001);
        assert_eq!(refused(&scene.read(1, 50)), None, "50,001 tokens back passes");
        scene.wave_at(60_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "exactly 50,000 tokens back is still above");
    }

    /// O arquivo mudado passa: a linha mudada passa, as que ficaram iguais
    /// continuam recusadas, linha por linha.
    #[test]
    fn a_changed_file_passes_and_the_unchanged_lines_are_still_refused() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(20_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "control: unchanged lines are refused");
        scene.write_notes(|line| if line == 20 { "changed".to_string() } else { format!("line {line}") });
        scene.wave_at(21_000);
        assert_eq!(refused(&scene.read(1, 50)), None, "a range with a changed line passes");
        assert_eq!(refused(&scene.read(10, 30)), None, "the changed line is inside this one too");
        assert!(refused(&scene.read(30, 50)).is_some(), "lines that did not change are still refused");
    }

    /// A faixa só em parte já lida passa; toda lida, recusa.
    #[test]
    fn a_range_only_partly_read_passes() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(20_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "control: the whole range was read");
        assert_eq!(refused(&scene.read(40, 60)), None, "ten of the lines were never read");
        assert_eq!(refused(&scene.read(51, 60)), None, "a range after the one read");
    }

    /// O mesmo pedido logo depois da recusa passa; depois de uma leitura
    /// diferente no meio, a saída acabou e o pedido volta a ser recusado.
    #[test]
    fn the_same_request_right_after_the_refusal_passes() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(20_000);
        let again = scene.read(1, 50);
        assert!(refused(&again).is_some(), "the first reread is refused");
        assert_eq!(refused(&again), None, "the same request right after passes");
        ran(&scene.after(&again));
        assert!(refused(&again).is_some(), "once the read ran it is above again, and refused");

        assert_eq!(refused(&scene.read(51, 60)), None, "a different read passes");
        assert!(refused(&again).is_some(), "a different read in between ends the way out");
    }

    /// Quem conduz e o subagente que não é de onda leem sem aviso, mesmo
    /// com o agente de onda recusado no mesmo arquivo.
    #[test]
    fn the_main_session_and_an_agent_outside_a_wave_pass() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(20_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "control: the wave agent is refused");

        let input = scene.read_input(1, 50);
        let conductor = scene.call("PreToolUse", None, "Read", input.clone());
        ran(&scene.after(&conductor));
        assert_eq!(refused(&conductor), None, "the conductor reads again with no notice");

        scene.agent_says("a1", "Explique o arquivo.", 20_000);
        let other = scene.call("PreToolUse", Some("a1"), "Read", input);
        ran(&scene.after(&other));
        assert_eq!(refused(&other), None, "an agent that is not a wave agent reads with no notice");
    }

    /// Nada do que quem conduz ou o subagente de fora de onda leu entra no
    /// registro: o agente de onda que lê depois o mesmo trecho não é recusado.
    #[test]
    fn what_the_conductor_and_other_agents_read_is_not_recorded() {
        let scene = Scene::new(Locale::PtBr);
        scene.agent_says("a1", "Explique o arquivo.", 10_000);
        let other = scene.call("PreToolUse", Some("a1"), "Read", scene.read_input(1, 50));
        ran(&scene.after(&other));
        let conductor = scene.call("PreToolUse", None, "Read", scene.read_input(1, 50));
        ran(&scene.after(&conductor));

        scene.wave_at(20_000);
        assert_eq!(refused(&scene.read(1, 50)), None, "the wave agent has read nothing yet");
        ran(&scene.after(&scene.read(1, 50)));
        assert!(refused(&scene.read(1, 50)).is_some(), "control: its own read is recorded");
    }

    /// Cada agente de onda tem o registro dele: um agente novo começa vazio,
    /// e o antigo segue com o que leu.
    #[test]
    fn a_new_agent_starts_with_an_empty_log() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 10_000);
        scene.wave_at(20_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "control: the first agent is refused");

        scene.wave_agent_at("a2", 20_000);
        scene.wave_at(20_000);
        let second = scene.call("PreToolUse", Some("a2"), "Read", scene.read_input(1, 50));
        assert_eq!(refused(&second), None, "the second agent has read nothing yet");
        ran(&scene.after(&second));
        assert!(refused(&second).is_some(), "and now it has");
    }

    /// A leitura que passou antes da ferramenta mas não rodou — recusada por
    /// outro gancho — não é anotada: a mesma leitura depois não é recusada.
    #[test]
    fn a_read_that_never_ran_is_not_recorded() {
        let scene = Scene::new(Locale::PtBr);
        scene.wave_at(10_000);
        let call = scene.read(1, 50);
        assert_eq!(refused(&call), None, "the first ask passes");
        scene.wave_at(20_000);
        assert_eq!(refused(&call), None, "no after-tool event came, so nothing was read");
        ran(&scene.after(&call));
        assert!(refused(&call).is_some(), "control: once it ran, the reread is refused");
    }

    /// O terminal vale como a ferramenta de leitura, nos dois sentidos: o
    /// que o `cat` mostrou barra a leitura que o `sed`, o `head`, o `tail` e a
    /// ferramenta repetem; e o `sed` lido barra o `cat` das mesmas linhas.
    #[test]
    fn terminal_reads_and_the_read_tool_share_the_log() {
        let scene = Scene::new(Locale::PtBr);
        scene.wave_at(10_000);
        let cat = scene.bash("cat notes.txt");
        assert_eq!(refused(&cat), None, "the first cat passes");
        ran(&scene.after(&cat));
        scene.wave_at(25_000);
        for command in [
            "cat notes.txt",
            "sed -n '10,20p' notes.txt",
            "nl -ba notes.txt",
            "head -n 5 notes.txt",
            "tail -n 5 notes.txt",
            "cd . && cat notes.txt",
        ] {
            assert!(refused(&scene.bash(command)).is_some(), "{command}");
        }
        assert!(refused(&scene.read(60, 100)).is_some(), "the Read tool is refused after a cat");
        assert_eq!(refused(&scene.bash("cat notes.txt | head -3")), None, "a pipe is not read");
        assert_eq!(refused(&scene.bash("grep line notes.txt")), None, "a search is not a read");

        let scene = Scene::new(Locale::PtBr);
        scene.wave_at(10_000);
        let sed = scene.bash("sed -n '1,30p' notes.txt");
        ran(&scene.after(&sed));
        scene.wave_at(15_000);
        assert!(refused(&scene.read(5, 25)).is_some(), "the Read tool is refused after a sed");
        assert_eq!(refused(&scene.bash("cat notes.txt")), None, "a cat of lines the sed did not show passes");
    }

    /// O resultado da ferramenta manda na faixa lida: a leitura inteira que o
    /// portão cortou na linha 30 só anota 1 a 30.
    #[test]
    fn the_range_the_tool_reports_is_the_range_recorded() {
        let scene = Scene::new(Locale::PtBr);
        scene.wave_at(10_000);
        let whole = scene.call("PreToolUse", Some("a1"), "Read", json!({ "file_path": scene.root().join("notes.txt").to_string_lossy() }));
        let mut done = scene.after(&whole);
        done.raw["tool_response"] = json!({"type": "text", "file": {"startLine": 1, "numLines": 30, "totalLines": 100}});
        ran(&done);
        scene.wave_at(20_000);
        assert!(refused(&scene.read(1, 30)).is_some(), "the lines it showed are refused");
        assert_eq!(refused(&scene.read(1, 50)), None, "the lines it cut away were never read");
    }

    /// A conversa que encolheu (compactada) levou a leitura embora: ela passa.
    #[test]
    fn a_compacted_conversation_forgets_what_it_read() {
        let scene = Scene::new(Locale::PtBr);
        read_once(&scene, 1, 50, 80_000);
        scene.wave_at(90_000);
        assert!(refused(&scene.read(1, 50)).is_some(), "control: still above before the compaction");
        scene.wave_at(20_000);
        assert_eq!(refused(&scene.read(1, 50)), None, "after the compaction the read passes");
    }

    /// A saída que o terminal corta não chega à conversa: o `cat` de um
    /// arquivo grande só anota o que cabe nos 30.000 bytes.
    #[test]
    fn a_terminal_read_only_records_what_the_output_keeps() {
        let scene = Scene::new(Locale::PtBr);
        scene.write_notes(|line| format!("{line} {}", "x".repeat(1_000)));
        scene.wave_at(10_000);
        let cat = scene.bash("cat notes.txt");
        ran(&scene.after(&cat));
        scene.wave_at(20_000);
        assert!(refused(&scene.bash("sed -n '1,20p' notes.txt")).is_some(), "the first lines were shown");
        assert!(refused(&scene.bash("sed -n '1,60p' notes.txt")).is_some(), "the cut output shows nothing new");
        assert_eq!(refused(&scene.bash("sed -n '30,40p' notes.txt")), None, "the lines past the cut never were shown");
    }
}
