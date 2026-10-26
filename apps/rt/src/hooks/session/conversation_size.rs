//! `conversation_size` — o tamanho da conversa e o que fazer quando ela cresce.
//!
//! Dois ganchos:
//!
//! - [`PrecompactNotice`], no evento `PreCompact`: antes de toda compactação
//!   — manual (`/compact`) ou, se a máquina a deixou ligada, a automática —, a
//!   conversa recebe o bloco de retomada
//!   ([`resume_block`](crate::commands::flow::resume::resume_block)), o mesmo
//!   que o início da sessão coloca sozinho depois do resumo. Sem controle de
//!   "já avisado": o próprio `PreCompact` é o degrau, então cada compactação
//!   merece o bloco de novo, e não há como ele ficar velho.
//! - [`SizeNotice`], nos dois lados de cada ferramenta, com a conversa que a
//!   chamada mede. **Em quem conduz** (a sessão principal), depois da
//!   ferramenta, a conversa avisa aos 200 mil e depois a cada 100 mil tokens,
//!   uma vez por faixa, com uma linha de retomada. Só um início confirmado
//!   após `/clear` ou `/compact` reinicia as faixas; uma leitura menor não o
//!   faz. Quem conduz nunca é recusado por tamanho. **No agente de
//!   onda** (o subagente cujo primeiro texto é o título de um pedido de onda),
//!   o sinal chega aos 200 mil tokens de contexto, contando a entrada inicial
//!   e os resumos herdados, com qualquer modelo. A janela do modelo que
//!   responde ao agente só serve de teto, quando 75% dela vem antes: pela
//!   tabela de
//!   [`window_of`](mustard_core::domain::context_window::window_of) e, no
//!   Codex, a que o registro da conversa declara (`model_context_window`) na
//!   última amostra de uso. Com 1 milhão, o sinal segue nos 200 mil; com 200
//!   mil de janela, chega aos 150 mil. O aviso diz a janela e o teto. A medida
//!   do fim de tarefa sai do próprio `run write step`
//!   ([`answer_step`]), pela identidade do agente que o gancho do terminal
//!   exporta ao comando; o gancho só a dá quando o comando chega a ele sem o
//!   prefixo e o relatório ainda não a traz.
//!   No meio da tarefa ele é apenas orientação: concluir a tarefa atual,
//!   testes mínimos e resumo, sem começar outra. Só o término aceito da
//!   tarefa fecha a trava de novas implementações. Estimar uma tarefa maior
//!   não antecipa o sinal. A entrada e o crescimento vêm de amostras do
//!   contexto do próprio agente, nunca do consumo acumulado da assinatura.
//!   A ordem de entregar fecha a trava: dali em diante, o gancho de antes da
//!   ferramenta recusa tudo, menos `run read` e `run write` na spec e o
//!   comandos declarados de compilar, testar e verificar estilo, sem outro comando na mesma
//!   linha. Enquanto a rodada espera o conserto da volta que recusou — o
//!   trecho de conserto dela está em disco —, a trava fica aberta, e a
//!   entrega nova a fecha de novo. Antes da ordem nada é recusado por
//!   tamanho, nem a conversa acima do limite no meio de uma tarefa. O agente
//!   que a reprovação de quem conduz tirou da onda — a conversa dele começou
//!   antes da volta reprovada — tem toda chamada recusada, com ou sem a
//!   ordem, ler e gravar na spec inclusive: uma volta nova dele desfaria a
//!   reprovação, e o conserto é do agente novo. Nas duas travas, a
//!   devolução do relatório a quem conduz passa: ela não grava na spec nem
//!   na cópia, e é o fim do agente. O resumo
//!   é o salto do tamanho entre a resposta que chama `run read delivered-<n>` ou `run read item-<código da entrega>` —
//!   o comando que o pedido da onda manda — e a resposta seguinte, somado
//!   quando o agente lê mais de um; sem resumo lido, conta a conversa inteira.
//!   O agente de onda nunca recebe o aviso de quem conduz, e quem conduz nunca
//!   recebe o da onda.
//!
//! No Claude, o tamanho soma `input_tokens`, `cache_read_input_tokens` e
//! `cache_creation_input_tokens` do último uso. No Codex, usa somente a
//! entrada de `last_token_usage`: cache já está incluído, e uso acumulado
//! não mede contexto. Formato desconhecido permanece sem medida. A de quem
//! conduz é a de `transcript_path`; a do subagente fica em
//! `<transcript_path sem .jsonl>/subagents/agent-<id>.jsonl` e, quando quem
//! conduz limpou a conversa no meio da onda, em pedaços de mesmo nome nas
//! pastas das sessões anteriores: o agente de onda é reconhecido pelo começo
//! do pedaço mais antigo, e os tamanhos dos passos de término e a ordem de
//! entregar ficam guardados pelo agente, não pela sessão. Sem arquivo
//! legível, sem uso gravado ou sem spec para retomar, nada acontece:
//! [`Verdict::Allow`], porque sem tamanho conhecido não há como decidir.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use mustard_core::domain::context_window::window_of;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::mustard_id;
use mustard_core::domain::spec_events::type_spec;
use mustard_core::io::spec_events as store;
use mustard_core::io::transcript::{agent_pieces, first_stamp};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::{ClaudePaths, translate};
use serde_json::Value;

use crate::hooks::bash::lex::{Segment, segments};

/// O degrau de tamanho da conversa de quem conduz, em tokens: a cada degrau
/// novo, um aviso.
pub(crate) const CONDUCTOR_FIRST_NOTICE: u64 = 200_000;
pub(crate) const CONDUCTOR_STEP: u64 = 100_000;

/// O tamanho de contexto do agente de onda em que o sinal chega, com
/// qualquer modelo: fixo, contando a entrada inicial e os resumos herdados; a
/// janela só o encurta, como teto ([`WINDOW_SHARE`]). A montagem das ondas
/// não usa este número: a onda leva todo o agrupamento, e é este sinal que
/// segura o tamanho. O sinal pede conclusão da tarefa atual; não interrompe
/// sua implementação.
pub(crate) const WAVE_LIMIT: u64 = 200_000;

/// A parte da janela do modelo da onda, em porcento, que é o teto do sinal
/// quando a janela é conhecida.
const WINDOW_SHARE: u64 = 75;

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

/// O tamanho da conversa nos dois lados de cada ferramenta: depois dela, a
/// quem conduz, o aviso de limpar ou compactar, e ao agente de onda que
/// terminou uma tarefa, a ordem de seguir ou de entregar; antes dela, só o
/// agente de onda que recebeu a ordem de entregar e o que a reprovação tirou
/// da onda são recusados, fora a devolução do relatório a quem conduz.
pub struct SizeNotice;

impl Check for SizeNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        match ctx.trigger {
            Some(Trigger::PostToolUse) => {
                let context = if input.is_subagent() {
                    wave_notice_text(input, &root, ctx.config.language().text_or_default())
                } else {
                    conductor_text(input, &root)
                };
                Ok(context.map_or(Verdict::Allow, |context| Verdict::Inject { context }))
            }
            Some(Trigger::PreToolUse) => Ok(wave_lock_reason(input, &root, ctx).map_or(Verdict::Allow, |reason| Verdict::Deny { reason })),
            _ => Ok(Verdict::Allow),
        }
    }
}

/// O tamanho de uma conversa: a soma de `input_tokens`,
/// `cache_read_input_tokens` e `cache_creation_input_tokens` do uso `usage`.
fn context_of(usage: &Value) -> Option<u64> {
    let input=usage.get("input_tokens")?.as_u64()?;
    let field = |name: &str| usage.get(name).map_or(Some(0),Value::as_u64);
    input.checked_add(field("cache_read_input_tokens")?)?.checked_add(field("cache_creation_input_tokens")?)
}

/// O uso gravado numa linha da conversa — em `message.usage`, ou na raiz da
/// linha quando ela já é o uso —, lida só quando a linha o cita.
fn usage_of_line(line: &str) -> Option<(Value, Value)> {
    if !line.contains("\"usage\"") && !line.contains("\"token_count\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    if value["type"]=="event_msg" && value["payload"]["type"]=="token_count" {
        let last=&value["payload"]["info"]["last_token_usage"];
        let valid=(|| {
            let input=last["input_tokens"].as_u64()?;
            let cached=last["cached_input_tokens"].as_u64()?;
            let output=last["output_tokens"].as_u64()?;
            (cached<=input && input.checked_add(output)?==last["total_tokens"].as_u64()?)
                .then(||serde_json::json!({"input_tokens":input}))
        })();
        // Keep an unknown latest sample as unknown; never fall back to an
        // older, larger context or the cumulative billing counter.
        return Some((value,valid.unwrap_or(Value::Null)));
    }
    let usage = value.get("message").and_then(|m| m.get("usage")).or_else(|| value.get("usage"))?.clone();
    Some((value, usage))
}

/// A janela do modelo que a linha de uso `row` do Codex declara, em
/// `payload.info.model_context_window`. `None` numa linha do Claude, na
/// amostra sem o campo e com o valor zero, que não é janela: tomado como
/// janela, ele zeraria o teto e mandaria o sinal na primeira amostra.
fn declared_window(row: &Value) -> Option<u64> {
    row["payload"]["info"]["model_context_window"].as_u64().filter(|window| *window > 0)
}

/// O teto do sinal dentro da janela `window`: [`WINDOW_SHARE`] dela.
fn share_of(window: u64) -> u64 {
    window / 100 * WINDOW_SHARE
}

/// O tamanho da conversa em `path`: o último uso gravado nela.
/// `None` sem arquivo legível ou sem nenhum uso gravado.
fn last_context(path: &Path) -> Option<u64> {
    context_of(&last_usage(path)?.1)
}

/// A última linha de uso da conversa em `path` e o uso dela
/// ([`usage_of_line`]). Lê só o fim do arquivo e, se ali não houver uso, a
/// janela cresce até o arquivo inteiro. `None` sem arquivo legível ou sem
/// nenhum uso gravado.
fn last_usage(path: &Path) -> Option<(Value, Value)> {
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
            return Some(found);
        }
        if start == 0 {
            return None;
        }
        window = window.saturating_mul(4);
    }
}

/// Se `text` serve de nome na pasta de estado: não vazio, só letras e
/// números ASCII, `-` e `_`.
fn is_plain(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A pasta de estado `.claude/.session/` do projeto `root`.
fn state_dir(root: &Path) -> Option<PathBuf> {
    Some(ClaudePaths::for_project(root).ok()?.claude_dir().join(".session"))
}

/// O arquivo de estado `name` da sessão `session`, em `.claude/.session/` do
/// projeto `root`. `None` sem sessão de verdade: sem onde guardar o que já
/// foi avisado.
fn mark_path(root: &Path, session: Option<&str>, name: &str) -> Option<PathBuf> {
    let session = session.map(str::trim).filter(|session| is_plain(session) && *session != "unknown")?;
    is_plain(name).then_some(())?;
    Some(state_dir(root)?.join(session).join(name))
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
/// `input`, sob `root`, passou de uma faixa nova de [`CONDUCTOR_STEP`]. A
/// faixa guardada só reinicia quando o host confirma limpeza/compactação;
/// uma leitura menor sozinha não apaga o aviso. `None` sem faixa nova ou spec.
fn conductor_mark(input: &HookInput, root: &Path) -> Option<PathBuf> {
    mark_path(root, input.session_id.as_deref(), "size-step").or_else(|| {
        let transcript = input.transcript_path()?;
        let key = format!("transcript-{}", blake3::hash(transcript.as_bytes()).to_hex());
        mark_path(root, Some(&key), "size-step")
    })
}

/// Reset only after the host confirms a clear or completed compaction.
pub(crate) fn reset_conductor_notice(input: &HookInput, root: &Path) {
    if let Some(path) = conductor_mark(input, root)
        && let Ok(mut mark) = mustard_core::io::fs::lock::LockedFile::exclusive(&path)
    {
        let _ = mark.replace(b"0");
    }
}

fn conductor_text(input: &HookInput, root: &Path) -> Option<String> {
    conductor_text_with(input, root, || crate::commands::flow::resume::current_line_and_live(root, input.session_id.as_deref()))
}

/// O aviso a quem conduz, com a linha de retomada e as ondas com agente vivo
/// tiradas de `line`. A marca da faixa é lida antes: a faixa já avisada não
/// lê a spec, e só a faixa nova a lê. Com onda de agente vivo, o aviso manda
/// esperar a volta delas antes de limpar a conversa, que para esses agentes.
fn conductor_text_with(input: &HookInput, root: &Path, line: impl FnOnce() -> Option<(String, Vec<u64>)>) -> Option<String> {
    let tokens = last_context(Path::new(input.transcript_path()?))?;
    if tokens < CONDUCTOR_FIRST_NOTICE { return None; }
    let threshold = tokens / CONDUCTOR_STEP * CONDUCTOR_STEP;
    let path = conductor_mark(input, root)?;
    let mut mark = mustard_core::io::fs::lock::LockedFile::exclusive(&path).ok()?;
    let warned = mark.read_to_string().ok()?.trim().parse::<u64>().unwrap_or(0);
    // Older versions stored the number of 200k bands instead of tokens.
    let warned = if warned > 0 && warned < CONDUCTOR_FIRST_NOTICE { warned.saturating_mul(200_000) } else { warned };
    if threshold <= warned { return None; }
    let (line, live) = line()?;
    mark.replace(threshold.to_string().as_bytes()).ok()?;
    let lang = crate::commands::spec_events::project(root).lang;
    let key = if live.is_empty() { "conversation_size.notice" } else { "conversation_size.notice_live" };
    let waves: Vec<String> = live.iter().map(u64::to_string).collect();
    Some(
        translate(key, lang)
            .replace("{tokens}", &(threshold / 1000).to_string())
            .replace("{count}", &live.len().to_string())
            .replace("{waves}", &waves.join(", "))
            .replace("{line}", &line),
    )
}

/// O que a leitura da conversa de um agente de onda traz.
struct WaveContext {
    /// O tamanho da conversa na primeira resposta: o começo dela.
    first: u64,
    /// O tamanho da conversa na última resposta.
    now: u64,
    /// Quanto do tamanho é o resumo da onda anterior que o agente leu.
    summary: u64,
    /// O modelo da última resposta que traz `message.model`, como a conversa
    /// o grava. Diferente do `model_used` que a volta da onda grava no envio,
    /// o gancho não pula o `<synthetic>` das respostas que a própria
    /// plataforma escreve: a tabela não conhece esse nome, e a janela fica
    /// desconhecida até a resposta seguinte de um modelo.
    model: Option<String>,
    /// A janela que a última amostra de uso do Codex declara
    /// ([`declared_window`]).
    declared: Option<u64>,
}

impl WaveContext {
    fn baseline(&self) -> u64 { self.first.saturating_add(self.summary) }
    fn growth(&self) -> u64 { self.now.saturating_sub(self.baseline()) }
    /// A janela do modelo da onda: a que a conversa declara ou, sem ela, a da
    /// tabela da API, quando a tabela conhece o modelo.
    fn window(&self) -> Option<u64> { self.declared.or_else(|| self.model.as_deref().and_then(window_of)) }
    /// O tamanho da conversa em que o sinal chega: [`WAVE_LIMIT`], ou menos
    /// quando [`WINDOW_SHARE`] da janela do modelo vem antes. Sem janela
    /// conhecida, não há teto.
    fn signal_at(&self) -> u64 {
        WAVE_LIMIT.min(self.window().map_or(u64::MAX, share_of))
    }
    /// Se a conversa chegou ao sinal.
    fn reached(&self) -> bool { self.now >= self.signal_at() }
}

/// Se o bloco de conteúdo é uma chamada do terminal que lê o resumo de uma
/// onda entregue: `run read delivered-<n>`, ou `run read item-<código>` com o
/// código de uma entrega — a leitura de um item de outro tipo (tarefa, regra,
/// decisão) não é resumo.
fn reads_summary(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("tool_use")
        && block.get("name").and_then(Value::as_str) == Some("Bash")
        && block
            .get("input")
            .and_then(|input| input.get("command"))
            .and_then(Value::as_str)
            .is_some_and(reads_delivery)
}

/// Se o comando `command` lê uma entrega: `run read delivered-<n>`, ou
/// `run read item-<código>` em que o código é de uma entrega.
fn reads_delivery(command: &str) -> bool {
    let delivery = type_spec("delivered").map(|spec| spec.code);
    command.contains("run read delivered-")
        || command.split("run read item-").skip(1).any(|rest| {
            let code = rest.split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | ';' | '&' | '|')).next();
            code.and_then(mustard_id::parse).is_some_and(|(kind, _)| Some(kind) == delivery)
        })
}

/// O texto da primeira mensagem do usuário numa linha da conversa: corrido,
/// ou o dos blocos de texto juntos. `None` quando a linha não é uma mensagem
/// do usuário.
fn user_text(line: &Value) -> Option<String> {
    let message = if line["type"]=="response_item" {line.get("payload")?} else {line.get("message")?};
    let role = message.get("role").and_then(Value::as_str).or_else(|| line.get("type").and_then(Value::as_str));
    if role != Some("user") {
        return None;
    }
    match message.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => Some(blocks.iter().filter_map(|block| block.get("text")).filter_map(Value::as_str).collect()),
        _ => None,
    }
}

/// A spec e a onda do pedido com que a conversa em `path` abre, pela régua
/// única do despacho ([`wave_named`](crate::shared::host_observation::wave_named)),
/// com a raiz das specs de `root`: a primeira mensagem do usuário nela é a
/// que vale. `None` quando ela não abre com um pedido de onda deste projeto.
fn wave_opened(path: &Path, root: &Path) -> Option<(String, u64)> {
    use std::io::BufRead;

    let file = std::fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file.take(262_144)).lines().map_while(Result::ok) {
        let Ok(row) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(text) = user_text(&row) else { continue };
        let wave = crate::shared::host_observation::wave_named(root, &text).map(|read| (read.spec, read.wave));
        // Codex can precede the delegated user task with environment/rule
        // messages. Its file identity is checked separately, not by title.
        if wave.is_some() || row["type"]!="response_item" {return wave;}
    }
    None
}

/// A leitura da conversa do agente, nos pedaços `pieces`, do mais antigo ao
/// mais novo ([`agent_pieces`]): `None` quando o mais antigo não abre com o
/// título de um pedido de onda no idioma `lang` (um agente qualquer), ou
/// quando ainda não há nenhum uso gravado. O começo é o primeiro uso, e o
/// tamanho de agora, o último, o do pedaço da sessão atual, que é o mais
/// novo. O resumo pesa o salto do
/// tamanho entre a resposta que o lê (veja [`reads_summary`]) e a resposta
/// seguinte — a linha seguinte com o mesmo `message.id` é da mesma resposta
/// —, também quando o resumo foi lido num pedaço anterior. O modelo é o da
/// última resposta que traz `message.model`, e a janela declarada, a da
/// última amostra de uso.
fn wave_context(pieces: &[PathBuf], root: &Path) -> Option<WaveContext> {
    use std::io::BufRead;

    wave_opened(pieces.first()?, root)?;
    let (mut first, mut now, mut summary, mut before) = (None, None, 0_u64, None::<(Option<String>, u64)>);
    let (mut model, mut declared) = (None::<String>, None);
    for piece in pieces {
        let Ok(file) = std::fs::File::open(piece) else { continue };
        for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
            if line.contains("function_call") && let Ok(row)=serde_json::from_str::<Value>(&line)
                && row["type"]=="response_item" && row["payload"]["type"]=="function_call"
                && row["payload"]["name"]=="exec_command"
                && let Some(args)=row["payload"]["arguments"].as_str().and_then(|args|serde_json::from_str::<Value>(args).ok())
                && args["cmd"].as_str().is_some_and(reads_delivery) && let Some(context)=now {
                before=Some((None,context));
            }
            let Some((value, usage)) = usage_of_line(&line) else { continue };
            let Some(context) = context_of(&usage) else {
                now = None;
                continue;
            };
            first.get_or_insert(context);
            now = Some(context);
            declared = declared_window(&value);
            let message = value.get("message");
            if let Some(name) = message.and_then(|message| message.get("model")).and_then(Value::as_str) {
                model = Some(name.to_owned());
            }
            let id = message.and_then(|message| message.get("id")).and_then(Value::as_str).map(str::to_owned);
            if let Some((_, read_at)) = before.take_if(|(read_id, _)| id.is_none() || *read_id != id) {
                summary += context.saturating_sub(read_at);
            }
            let reads = message
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
                .is_some_and(|blocks| blocks.iter().any(reads_summary));
            if reads {
                before = Some((id, context));
            }
        }
    }
    Some(WaveContext { first: first?, now: now?, summary, model, declared })
}

/// Prefere o caminho explícito do host. O Codex só aceita o caminho comum
/// quando a metadata identifica o filho; nunca adivinha pastas do Claude.
/// Sem caminho nativo, Claude conserva sua convenção `subagents/`.
fn agent_transcript(input: &HookInput) -> Option<PathBuf> {
    let agent=input.subagent_id()?;
    if let Some(path)=input.raw.get("agent_transcript_path").and_then(Value::as_str).map(str::trim).filter(|p|!p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let transcript = input.transcript_path()?;
    if input.raw["mustard_host"]=="codex" {
        // The common path can be the parent's. Accept a native rollout only
        // when its metadata identifies this exact child; never scan guesses.
        let mut bytes=Vec::new();
        std::fs::File::open(transcript).ok()?.take(65_536).read_to_end(&mut bytes).ok()?;
        let meta=String::from_utf8_lossy(&bytes).lines().find_map(|line| {
            let row:Value=serde_json::from_str(line).ok()?;
            (row["type"]=="session_meta").then_some(row)
        })?;
        return (meta["payload"]["id"].as_str()==Some(agent) && meta["payload"]["source"]["subagent"].is_object())
            .then(||PathBuf::from(transcript));
    }
    let name = input.subagent_transcript_name()?;
    let session_dir = transcript.strip_suffix(".jsonl").unwrap_or(transcript);
    Some(Path::new(session_dir).join("subagents").join(name))
}

fn agent_parts(input:&HookInput)->Vec<PathBuf> {
    let Some(path)=agent_transcript(input) else {return Vec::new();};
    if input.raw["mustard_host"]=="codex" {return vec![path];}
    let pieces=agent_pieces(&path);
    if pieces.is_empty() && input.raw.get("agent_transcript_path").is_some() {vec![path]} else {pieces}
}

/// Use this exact child's latest native context, never the parent's total.
pub(crate) fn analysis_context(input:&HookInput)->Option<u64> {
    last_context(agent_parts(input).last()?)
}

/// Os dois arquivos de estado do agente de onda de `input`: o tamanho da
/// conversa, sem o resumo lido, em cada passo de término, um por linha, e a
/// ordem de entregar, que só existe depois dela. Ficam pelo nome do agente,
/// direto em `.claude/.session/`, fora da pasta de qualquer sessão: o
/// `/clear` de quem conduz troca a sessão no meio da onda, e nem as tarefas
/// já medidas nem a ordem dada antes dele se perdem. `None` fora de um
/// subagente.
fn wave_marks(root: &Path, input: &HookInput) -> Option<(PathBuf, PathBuf)> {
    Some((wave_mark(root, input, "steps")?, wave_mark(root, input, "deliver")?))
}

fn wave_mark(root: &Path, input: &HookInput, kind: &str) -> Option<PathBuf> {
    let name = input.subagent_transcript_name()?;
    let agent = name.trim_end_matches(".jsonl");
    let name = format!("size-{kind}-{agent}");
    Some(state_dir(root)?.join(is_plain(&name).then_some(name)?))
}

/// O que o agente de onda de `input` recebe depois da ferramenta: a medida
/// do fim de tarefa quando a chamada grava o passo de término
/// ([`task_end_text`]) e, nas outras chamadas, o sinal de conclusão. O sinal
/// deixa a tarefa atual seguir; só o passo de término aceito fecha a trava.
/// Para não reler a conversa inteira a cada chamada, o gancho cala depois do
/// sinal ou da ordem de entregar e, olhando só o último uso, abaixo do ponto
/// do sinal medido da última vez — ou, antes de qualquer medida, abaixo do
/// horizonte fixo ([`WAVE_LIMIT`]). Quando a última amostra declara a janela
/// do modelo ([`declared_window`]), o teto dela ([`WINDOW_SHARE`]) baixa esse
/// ponto se vier antes dele.
fn wave_notice_text(input: &HookInput, root: &Path, lang: Locale) -> Option<String> {
    if answered_step(input) { return None; }
    if finishes_a_task(input) { return task_end_text(input, root, lang); }
    let signal = wave_mark(root, input, "signal")?;
    let (_, order) = wave_marks(root, input)?;
    if signal.exists() || order.exists() { return None; }
    let parts = agent_parts(input);
    let (row, usage) = last_usage(parts.last()?)?;
    let now = context_of(&usage)?;
    let threshold_path = wave_mark(root, input, "threshold")?;
    let threshold = std::fs::read_to_string(&threshold_path).ok().and_then(|value| value.trim().parse::<u64>().ok());
    // A janela da última resposta — a declarada, ou a do modelo dela — cujo
    // teto vem antes do limite fixo, ou do ponto guardado, adianta o sinal.
    let window = declared_window(&row).or_else(|| row["message"]["model"].as_str().and_then(window_of));
    let skip_below = threshold.unwrap_or(WAVE_LIMIT).min(window.map_or(u64::MAX, share_of));
    if now < skip_below { return None; }
    let context = wave_context(&parts, root)?;
    write_mark(&threshold_path, context.signal_at());
    if !context.reached() { return None; }
    write_mark(&signal, context.now);
    Some(wave_measurement("conversation_size.wave_signal", &context, 0, lang))
}

fn wave_measurement(key: &str, context: &WaveContext, largest: u64, lang: Locale) -> String {
    let thousands = |tokens: u64| (tokens / 1000).to_string();
    translate(key, lang)
        .replace("{now}", &thousands(context.now))
        .replace("{baseline}", &thousands(context.baseline()))
        .replace("{counted}", &thousands(context.growth()))
        .replace("{total_limit}", &thousands(context.signal_at()))
        .replace("{largest}", &thousands(largest))
        .replace("{basis}", &window_text(context, lang))
}

/// O que o aviso diz da janela que usou: a que a conversa declara, ou a do
/// modelo da onda, com o nome dele, e a parte dela que é o teto do sinal; ou
/// que ela é desconhecida, e o sinal fica sem teto.
fn window_text(context: &WaveContext, lang: Locale) -> String {
    if let Some(window) = context.declared {
        return translate("conversation_size.window_declared", lang)
            .replace("{window}", &window_size(window, lang))
            .replace("{share}", &WINDOW_SHARE.to_string());
    }
    match (context.model.as_deref(), context.window()) {
        (Some(model), Some(window)) => translate("conversation_size.window_known", lang)
            .replace("{model}", model)
            .replace("{window}", &window_size(window, lang))
            .replace("{share}", &WINDOW_SHARE.to_string()),
        _ => translate("conversation_size.window_unknown", lang).to_owned(),
    }
}

/// O tamanho da janela `window` como se fala: 1 milhão de tokens, ou o
/// número de mil das janelas menores da tabela.
fn window_size(window: u64, lang: Locale) -> String {
    if window == 1_000_000 {
        return translate("conversation_size.window_million", lang).to_owned();
    }
    translate("conversation_size.window_thousands", lang).replace("{n}", &(window / 1000).to_string())
}

/// Se a chamada `input` gravou o fim de uma tarefa: o terminal, com um
/// comando só além dos `cd`, `mustard-rt run write step`, cujo `--json` traz
/// no `item` o código de uma tarefa, e cuja saída não é a recusa da gravação.
/// O `--json @arquivo` é relido do arquivo, partindo da pasta da chamada
/// depois dos `cd` que vêm antes do comando.
fn finishes_a_task(input: &HookInput) -> bool {
    if input.tool_name.as_deref() != Some("Bash") {
        return false;
    }
    let Some(command) = input.tool_input.get("command").and_then(Value::as_str) else {
        return false;
    };
    let mut folder = PathBuf::from(input.cwd.as_deref().unwrap_or_default());
    let mut main = None;
    for segment in segments(command) {
        if program_name(&segment) != "cd" {
            if main.replace(segment).is_some() {
                return false;
            }
        } else if let (None, Some(to)) = (&main, segment.args.first()) {
            folder = folder.join(&to.text);
        }
    }
    let Some(main) = main else { return false };
    let args: Vec<&str> = main.args.iter().map(|word| word.text.as_str()).collect();
    if program_name(&main) != "mustard-rt" || !args.starts_with(&["run", "write", "step"]) {
        return false;
    }
    let json = args
        .windows(2)
        .find_map(|pair| (pair[0] == "--json").then_some(pair[1]))
        .or_else(|| args.iter().find_map(|arg| arg.strip_prefix("--json=")))
        .and_then(|json| crate::shared::json_arg::resolve_in(json.to_string(), &folder).ok());
    // O relatório que já traz a medida veio do próprio passo
    // ([`answer_step`]): medir de novo contaria o passo duas vezes.
    let accepted = tool_report(input).is_some_and(|report|report["ok"]==true && report.get(STEP_SIZE_FIELD).is_none());
    json.is_some_and(|json| names_a_task(&json)) && accepted
}

/// O relatório JSON que a chamada de terminal `input` imprimiu; `None` na
/// saída com erro ou que não é JSON.
fn tool_report(input: &HookInput) -> Option<Value> {
    let response=&input.raw["tool_response"];
    if response.get("exit_code").is_some_and(|code|code.as_i64()!=Some(0)) {return None;}
    let stdout = response.get("stdout").or_else(||response.get("output")).and_then(Value::as_str).or_else(||response.as_str())?;
    serde_json::from_str::<Value>(stdout).ok()
}

/// Se a chamada `input` gravou um passo cuja resposta já trouxe a medida
/// ([`answer_step`]), liberando a próxima tarefa ou mandando entregar: o
/// gancho não mede de novo nem dá outra ordem para esse mesmo passo, mesmo
/// com a conversa crescida depois da resposta. Quem decide é a medida do fim
/// da tarefa seguinte. Vale com qualquer prefixo no comando: o que conta é o
/// relatório do passo.
fn answered_step(input: &HookInput) -> bool {
    input.tool_name.as_deref() == Some("Bash")
        && tool_report(input).is_some_and(|report| report["ok"] == true && report["type"] == "step" && report.get(STEP_SIZE_FIELD).is_some())
}

/// Se o `--json` de um passo aponta no `item` o código de uma tarefa.
fn names_a_task(json: &str) -> bool {
    let task = type_spec("task").map(|spec| spec.code);
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|step| step.get("item")?.as_str().and_then(mustard_id::parse).map(|(kind, _)| Some(kind) == task))
        .unwrap_or(false)
}

/// O campo do relatório de `run write step` com a medida do fim de tarefa.
pub(crate) const STEP_SIZE_FIELD: &str = "size";

/// A medida do fim de tarefa respondida pelo próprio `run write step`: o
/// passo `event_type` aceito (`report` com `ok`), cujo `--json` aponta uma
/// tarefa, gravado pelo agente `agent` da sessão `session`, cuja conversa
/// principal é `transcript`, ganha no relatório a medida e a ordem de seguir
/// ou de entregar ([`task_end_measure`]). A ordem de entregar fecha a trava
/// dali em diante. Não depende de o gancho reconhecer o comando, que chega a
/// ele reescrito com o prefixo da identidade. Fora de um agente de onda, sem
/// a conversa ou sem medida, o relatório fica como está. Devolve se a medida
/// mandou entregar; `None` sem medida.
#[allow(clippy::too_many_arguments)]
pub(crate) fn answer_step(
    report: &mut Value,
    root: &Path,
    event_type: &str,
    json: &str,
    agent: Option<&str>,
    session: Option<&str>,
    transcript: Option<&Path>,
    lang: Locale,
) -> Option<bool> {
    if report["ok"] != true || event_type.trim() != "step" || !names_a_task(json) {
        return None;
    }
    let (Some(agent), Some(transcript)) = (agent, transcript) else { return None };
    let input = HookInput {
        session_id: session.map(str::to_string),
        agent_id: Some(agent.to_string()),
        raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
        ..HookInput::default()
    };
    let (text, deliver) = task_end_measure(&input, root, lang)?;
    report[STEP_SIZE_FIELD] = Value::String(text);
    Some(deliver)
}

/// A medida do fim de tarefa ao agente de onda: no resultado do passo de
/// término ([`finishes_a_task`]) do subagente de `input`, o tamanho da
/// conversa inteira, o limite, o crescimento da maior tarefa já
/// terminada e a ordem. Manda entregar quando o tamanho passou do ponto do
/// sinal ([`WaveContext::signal_at`]), ou
/// quando a ordem já saiu; senão, manda seguir. Guarda o tamanho do passo e,
/// com a ordem de entregar, fecha a trava ([`wave_lock_reason`]). `None` fora
/// de uma onda e em toda chamada que não termina uma tarefa: no meio dela,
/// nada a manda parar.
fn task_end_text(input: &HookInput, root: &Path, lang: Locale) -> Option<String> {
    if !finishes_a_task(input) {
        return None;
    }
    task_end_measure(input, root, lang).map(|(text, _)| text)
}

/// A medida de [`task_end_text`], já sabido que a chamada terminou uma
/// tarefa: pelo gancho ou pelo próprio passo ([`answer_step`]), com a ordem
/// de entregar (`true`) ou de seguir.
fn task_end_measure(input: &HookInput, root: &Path, lang: Locale) -> Option<(String, bool)> {
    let context = wave_context(&agent_parts(input), root)?;
    let counted = context.now.saturating_sub(context.summary);
    let (steps, order) = wave_marks(root, input)?;
    let mut sizes: Vec<u64> = std::fs::read_to_string(&steps)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    sizes.push(counted);
    if let Some(parent) = steps.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let lines = sizes.iter().fold(String::new(), |mut lines, size| {
        let _ = writeln!(lines, "{size}");
        lines
    });
    let _ = std::fs::write(&steps, lines);
    let largest = std::iter::once(context.first)
        .chain(sizes.iter().copied())
        .collect::<Vec<_>>()
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .max()
        .unwrap_or(0);
    write_mark(&wave_mark(root, input, "threshold")?, context.signal_at());
    // A entrada inicial e os resumos herdados vêm antes do horizonte de
    // trabalho. A previsão do tamanho da tarefa só informa; nunca adianta o
    // corte.
    let deliver = order.exists() || wave_mark(root,input,"signal")?.exists() || context.reached();
    if deliver {
        write_mark(&order, context.now);
    }
    let key = if deliver { "conversation_size.wave_deliver" } else { "conversation_size.wave_continue" };
    Some((wave_measurement(key, &context, largest, lang), deliver))
}

/// O nome do programa de `segment`, como foi escrito, sem a pasta e sem o
/// `.exe`: `$HOME/.cargo/bin/cargo` é `cargo`.
fn program_name(segment: &Segment) -> &str {
    let name = segment.program.raw_unquoted().rsplit(['/', '\\']).next().unwrap_or_default();
    name.strip_suffix(".exe").unwrap_or(name)
}

/// Se `segment` lê ou grava na spec: o `mustard-rt` seguido de `run read` ou
/// `run write`.
fn uses_the_spec(segment: &Segment) -> bool {
    let args: Vec<&str> = segment.args.iter().take(2).map(|word| word.text.as_str()).collect();
    program_name(segment) == "mustard-rt" && matches!(args[..], ["run", "read" | "write"])
}

/// Se `segment` é o comando de compilar do projeto, `build`: o mesmo
/// programa, pelo nome ou pelo caminho, com os argumentos dele no começo. As
/// variáveis e o `rtk` da frente não contam.
fn runs_build(segment: &Segment, build: &Segment) -> bool {
    !program_name(build).is_empty()
        && program_name(segment) == program_name(build)
        && segment.args.len() >= build.args.len()
        && segment.args.iter().zip(&build.args).all(|(asked, wanted)| asked.text == wanted.text)
}

/// Se `segment` só corta a saída que recebe: `tail` ou `head` sem arquivo,
/// só com opções e números.
fn trims_the_output(segment: &Segment) -> bool {
    matches!(program_name(segment), "tail" | "head")
        && segment.args.iter().all(|arg| arg.text.starts_with('-') || arg.text.chars().all(|c| c.is_ascii_digit()))
}

use crate::shared::handback::hands_back_the_report;
#[cfg(test)]
use crate::shared::handback::REPORT_BACK_TOOL;

/// Finish-only commands may be composed; every executable is checked.
/// Nested substitutions, unrelated commands and file redirections stay denied.
fn passes_after_the_order(input: &HookInput, commands: &[Option<&str>]) -> bool {
    if input.tool_name.as_deref() != Some("Bash") { return false; }
    let Some(command) = input.tool_input.get("command").and_then(Value::as_str) else { return false; };
    let parts = segments(command);
    if parts.iter().all(|part| program_name(part) == "cd") || parts.len() > crate::hooks::bash::lex::command_ranges(command).len() { return false; }
    let checks: Vec<_> = commands.iter().flatten().flat_map(|command| segments(command)).collect();
    parts.iter().all(|part| {
        if part.redirects.iter().any(|redirect| redirect.op.trim_start_matches(|ch: char| ch.is_ascii_digit()) != ">&" || !["1","2"].contains(&redirect.target.text.as_str())) { return false; }
        program_name(part) == "cd" || uses_the_spec(part)
            || checks.iter().any(|check| runs_build(part, check))
            || trims_the_output(part) && parts.iter().any(|other| other.piped)
            || removes_dependency_link(input, part)
    })
}

/// Legacy links can be unlinked after finishing; never remove a directory or
/// the main checkout's dependencies. New copies use local installations.
fn removes_dependency_link(input: &HookInput, part: &Segment) -> bool {
    if program_name(part) != "unlink" { return false; }
    let paths: Vec<_> = part.args.iter().filter(|arg| arg.text != "--").collect();
    let [path] = paths.as_slice() else { return false; };
    let Some(cwd) = input.cwd.as_deref().map(Path::new) else { return false; };
    if mustard_core::io::workspace::linked_worktree_main(cwd).is_none() { return false; }
    let file = Path::new(&path.text);
    let file = if file.is_absolute() { file.to_path_buf() } else { cwd.join(file) };
    file.file_name().is_some_and(|name| name == "node_modules")
        && file.parent().and_then(|parent| parent.canonicalize().ok()) == cwd.canonicalize().ok()
        && file.symlink_metadata().is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// Se a rodada recusou a volta da onda do agente de `input` e espera o
/// conserto dele: o pedaço mais antigo da conversa abre com o título do
/// pedido da onda no idioma `lang`, e o trecho de conserto da volta pendente
/// dela ([`fix_file`](crate::commands::flow::round::fix_file)) está em disco,
/// na spec de `root`. A entrega nova muda o nome do trecho, e a resposta
/// volta a ser `false` sozinha, sem marca a apagar.
fn awaits_its_fix(input: &HookInput, root: &Path) -> bool {
    let pieces = agent_parts(input);
    let Some(first) = pieces.first() else { return false };
    let Some((spec, wave)) = wave_opened(first, root) else { return false };
    let root = store::spec_root(root);
    let Some(log) = store::spec_file(&root, &spec).ok().and_then(|path| store::read(&path).ok().flatten()) else {
        return false;
    };
    crate::commands::flow::round::fix_file(&root, &spec, &log, wave).is_some_and(|file| file.is_file())
}

/// A onda de que a reprovação de quem conduz tirou o agente de `input`: o
/// pedaço mais antigo da conversa dele abre com o pedido da onda
/// ([`wave_opened`]), e o primeiro carimbo dele vem antes da volta reprovada,
/// gravada na spec de `root`
/// ([`replaced_in_file`](crate::commands::flow::round::replaced_in_file)).
/// `None` fora de um agente de onda, sem carimbo na conversa e sem
/// reprovação gravada para a onda.
fn left_by_rejection(input: &HookInput, root: &Path) -> Option<u64> {
    let pieces = agent_parts(input);
    let first = pieces.first()?;
    let (spec, wave) = wave_opened(first, root)?;
    let started = first_stamp(first)?;
    let path = store::spec_file(&store::spec_root(root), &spec).ok()?;
    crate::commands::flow::round::replaced_in_file(&path, wave, started).then_some(wave)
}

/// A recusa ao agente de onda de `input`. O agente que a reprovação de quem
/// conduz tirou da onda ([`left_by_rejection`]) tem toda chamada recusada,
/// com ou sem a ordem de entregar, ler e gravar na spec inclusive: uma volta
/// nova dele desfaria a reprovação, e o conserto é do agente novo. Os outros
/// só depois da ordem de entregar: a trava fechada por [`task_end_text`], e a
/// chamada não é ler ou gravar na spec nem o comando de compilar do projeto
/// ([`passes_after_the_order`]). `None` fora de um agente de onda, antes da
/// ordem — por maior que a conversa esteja —, para o que passa e enquanto a
/// rodada espera o conserto da volta que recusou ([`awaits_its_fix`]): quem
/// conserta é o agente que fez a onda, mesmo acima do limite, e a entrega
/// nova fecha a trava de novo. A devolução do relatório a quem conduz
/// ([`hands_back_the_report`]) passa pelas duas travas.
fn wave_lock_reason(input: &HookInput, root: &Path, ctx: &Ctx) -> Option<String> {
    if !input.is_subagent() || hands_back_the_report(input) {
        return None;
    }
    let lang = ctx.config.language().text_or_default();
    if let Some(wave) = left_by_rejection(input, root) {
        return Some(translate("subagent.rejected_agent_locked", lang).replace("{wave}", &wave.to_string()));
    }
    let (_, order) = wave_marks(root, input)?;
    if !order.exists() {
        return None;
    }
    let commands = ctx.config.commands();
    let mut finish_checks: Vec<String> = [commands.build.clone(), commands.test.clone(), commands.lint.clone()].into_iter().flatten().collect();
    if let Some((spec, wave)) = agent_parts(input).first().and_then(|path| wave_opened(path, root))
        && let Some(log) = store::spec_file(&store::spec_root(root), &spec).ok().and_then(|path| store::read(&path).ok().flatten()) {
        finish_checks.extend(log.criteria_for_waves(&[wave]).into_iter().filter_map(|criterion| criterion.str_field("proof").map(str::to_string)));
    }
    let allowed: Vec<_> = finish_checks.iter().map(|command| Some(command.as_str())).collect();
    if passes_after_the_order(input, &allowed) { return None; }
    if awaits_its_fix(input, root) {
        return None;
    }
    let build = [commands.build, commands.test, commands.lint].into_iter().flatten()
        .map(|command| translate("conversation_size.wave_locked_build", lang).replace("{command}", &command)).collect::<String>();
    Some(translate("conversation_size.wave_locked", lang).replace("{build}", &build))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_sample(input:u64,cached:u64)->Value {
        serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{
            "last_token_usage":{"input_tokens":input,"cached_input_tokens":cached,"output_tokens":100,"total_tokens":input+100},
            "total_token_usage":{"input_tokens":9_000_000,"cached_input_tokens":8_000_000,"output_tokens":1_000_000,"total_tokens":10_000_000}
        }}})
    }
    fn codex_event(event:&str,call:&HookInput)->Option<Value> {
        let mut call=call.clone(); call.hook_event_name=Some(event.into());
        crate::host::codex(event,&call).map(|s|serde_json::from_str(&s).unwrap())
    }
    fn native_rollout(path:&Path,agent:&str,rows:&[Value]) {
        let mut all=vec![serde_json::json!({"type":"session_meta","payload":{"id":agent,"source":{"subagent":{"thread_spawn":{"parent_thread_id":"s1"}}}}}),
            serde_json::json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"<environment_context>Setup</environment_context>"}]}}),
            serde_json::json!({"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":wave_request(Locale::PtBr)}]}})];
        all.extend_from_slice(rows);
        let text=all.into_iter().fold(String::new(),|mut text,row| {writeln!(text,"{row}").unwrap(); text});
        std::fs::write(path,text).unwrap();
    }

    #[test]
    fn codex_context_notices_use_last_input_once_and_reset_only_on_confirmed_compaction() {
        let dir=open_project("x"); let root=dir.path(); let path=root.join("rollout.jsonl");
        let call=conductor_after_tool(root,&path);
        let at=|tokens| {
            std::fs::write(&path,codex_sample(tokens,tokens-1).to_string()).unwrap();
            codex_event("PostToolUse",&call)
        };
        assert!(at(199_999).is_none(),"cumulative 10M and cached input must not trigger a notice");
        assert!(at(200_000).unwrap()["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("200"));
        assert!(at(299_999).is_none());
        // The resume line already opens with its own label: the notice does not repeat it.
        let notice=at(300_000).unwrap()["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string();
        assert!(notice.contains("Retomada: spec x") && !notice.contains("Retomada: Retomada:"),"{notice}");
        assert!(at(70_000).is_none()); assert!(at(200_000).is_none(),"a lower reading is not compaction");
        let before=codex_event("PreCompact",&call).unwrap();
        let block=crate::commands::flow::resume::current_block(root,Some("s1")).unwrap();
        assert!(before["systemMessage"].as_str().unwrap().contains(&block));
        assert!(before.get("hookSpecificOutput").is_none(),"compact uses documented common output");
        assert!(at(200_000).is_none(),"pre-compaction is not completed compaction");
        assert!(codex_event("PostCompact",&call).is_none()); assert!(at(200_000).is_some());
        assert!(at(200_000).is_none());
        let mut clear=call.clone(); clear.raw["source"]=serde_json::json!("clear");
        codex_event("SessionStart",&clear); assert!(at(200_000).is_some());
    }

    #[test]
    fn codex_unknown_latest_context_does_not_reuse_an_older_measurement() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("rollout.jsonl");
        let good=codex_sample(250_000,240_000);
        let mut bad=codex_sample(300_000,300_001);
        for invalid in [bad.clone(),{
            bad["payload"]["info"]["last_token_usage"]=Value::Null; bad.clone()
        }] {
            std::fs::write(&path,format!("{good}\n{invalid}\n")).unwrap();
            assert_eq!(last_context(&path),None);
        }
        native_rollout(&path,"a1",&[good,bad]);
        assert!(wave_context(&[path],dir.path()).is_none());
    }

    #[test]
    fn codex_wave_finishes_current_task_then_allows_tests_and_summary_but_no_new_work() {
        let dir=open_project("x"); let root=dir.path();
        std::fs::write(root.join("mustard.json"),r#"{"buildCommand":"cargo build","testCommand":"cargo test --lib"}"#).unwrap();
        let path=root.join("native-child.jsonl"); native_rollout(&path,"a1",&[codex_sample(30_000,20_000),codex_sample(190_000,150_000)]);
        let mut call=agent_after_tool(root); call.raw["agent_transcript_path"]=serde_json::json!(path);
        assert!(codex_event("PostToolUse",&call).is_none(),"no mid-task interruption");
        assert!(codex_event("PreToolUse",&call).as_ref().is_none_or(|v|v["hookSpecificOutput"]["permissionDecision"]!="deny"));
        native_rollout(&path,"a1",&[codex_sample(30_000,20_000),codex_sample(200_000,150_000)]);
        let done=finishing(call.clone());
        let notice=codex_event("PostToolUse",&done).unwrap();
        assert!(notice["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("Grave a entrega"));
        let mut next=call.clone(); next.tool_name=Some("apply_patch".into());
        next.tool_input=serde_json::json!({"command":"*** Begin Patch\n*** Add File: another.rs\n+fn another() {}\n*** End Patch"});
        assert_eq!(codex_event("PreToolUse",&next).unwrap()["hookSpecificOutput"]["permissionDecision"],"deny");
        next.tool_name=Some("exec_command".into());
        for command in ["cargo test --lib","cargo build","mustard-rt run write delivered --spec x --json '{}'","mustard-rt run read item-MSTD-TASK-0001 --spec x"] {
            next.tool_input=serde_json::json!({"cmd":command});
            let result=codex_event("PreToolUse",&next);
            assert!(result.as_ref().is_none_or(|v|v["hookSpecificOutput"]["permissionDecision"]!="deny"),"wrap-up allows {command}: {result:?}");
        }
    }

    #[test]
    fn native_agent_context_requires_its_actual_path_or_exact_child_metadata() {
        let dir=open_project("x"); let root=dir.path(); let path=root.join("parent.jsonl");
        native_rollout(&path,"parent",&[codex_sample(50_000,40_000),codex_sample(200_000,100_000)]);
        let mut call=finishing(agent_after_tool(root)); call.raw["transcript_path"]=serde_json::json!(path);
        assert!(codex_event("PostToolUse",&call).is_none(),"never attribute the parent's context to the child");
        native_rollout(&path,"a1",&[codex_sample(50_000,40_000),codex_sample(200_000,100_000)]);
        assert!(codex_event("PostToolUse",&call).is_some(),"exact child metadata supports its common native path");
        let actual=root.join("claude-child-elsewhere.jsonl");
        std::fs::write(&actual,format!("{}\n{}\n{}\n",serde_json::json!({"type":"user","message":{"role":"user","content":wave_request(Locale::PtBr)}}),agent_reply(50_000,None),agent_reply(200_000,None))).unwrap();
        let mut claude=finishing(agent_after_tool(root)); claude.agent_id=Some("a2".into());
        claude.raw["agent_transcript_path"]=serde_json::json!(actual);
        assert!(injected(&claude).is_some(),"Claude's explicit native path also avoids directory guessing");
    }

    #[test]
    fn task_end_requires_a_successful_native_receipt() {
        let dir=tempfile::tempdir().unwrap(); let mut call=finishing(agent_after_tool(dir.path()));
        assert!(finishes_a_task(&call));
        for response in [Value::Null,serde_json::json!({"output":"{\"ok\":false}","exit_code":0}),
            serde_json::json!({"stdout":"{\"ok\":true}","exit_code":1}),serde_json::json!({"stdout":"still running"})] {
            call.raw["tool_response"]=response; assert!(!finishes_a_task(&call));
        }
        for response in [serde_json::json!({"output":"{\"ok\":true}","exit_code":0}),serde_json::json!("{\"ok\":true}")] {
            call.raw["tool_response"]=response; assert!(finishes_a_task(&call));
        }
    }

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
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}},"buildCommand":"cargo build"}}"#))
            .unwrap();
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
            let line = crate::commands::flow::resume::current_line(root, Some("s1")).expect("the line");
            assert!(notice.contains(&line), "{lang:?}: the notice carries the resume line: {notice}");
            assert!(notice.len()<700,"short notice: {notice}");
            assert_eq!(at(299_999), None, "{lang:?}: the same step does not repeat");
            assert!(at(300_000).is_some(), "{lang:?}: 300k is the next band");
            assert_eq!(at(399_999), None, "{lang:?}: 300k does not repeat");
            let second = at(400_000).unwrap_or_else(|| panic!("{lang:?}: a new step warns again"));
            assert!(second.contains("400"), "{lang:?}: {second}");
            assert_eq!(at(70_000), None, "{lang:?}: a compacted conversation is below the step");
            assert_eq!(at(200_000),None,"a lower reading alone is not a compaction");
            reset_conductor_notice(&call, root);
            assert!(at(200_000).is_some(), "{lang:?}: after confirmed compaction the step warns again");
            assert!(at(500_000).is_some(),"skipped bands emit only the latest notice");
            assert_eq!(at(500_000),None);
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

    /// What the step command prints for the wave agent of `call`, at `root`.
    fn step_command_report(root: &Path, call: &HookInput) -> Value {
        let mut report = serde_json::json!({"ok": true, "type": "step"});
        let json = r#"{"wave":1,"item":"MSTD-TASK-0001","text":"feita"}"#;
        let transcript = call.transcript_path().map(PathBuf::from);
        answer_step(&mut report, root, "step", json, call.subagent_id(), call.session_id.as_deref(), transcript.as_deref(), Locale::PtBr);
        report
    }

    /// The host runs the step with the identity prefix the Bash hook adds,
    /// so the hook never sees a bare `run write step`. The step command
    /// itself answers the measure and closes the lock: the next new edit is
    /// refused.
    #[test]
    fn a_task_end_behind_the_hook_prefix_orders_the_delivery_and_locks() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let replies = vec![agent_reply(30_000, Some("read")), agent_reply(210_000, Some("edit"))];
        write_agent(root, &wave_request(Locale::PtBr), &replies);
        let call = agent_after_tool(root);
        let report = step_command_report(root, &call);
        let step = serde_json::json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "feita"});
        let command = format!("export MUSTARD_SESSION_ID='s1'\nexport MUSTARD_AGENT_ID='a1'\n/x/mustard-rt run write step --spec x --json '{step}'");
        let mut raw = call.raw.clone();
        raw["tool_response"] = serde_json::json!({"stdout": serde_json::to_string_pretty(&report).unwrap(), "stderr": ""});
        let ran = HookInput { tool_name: Some("Bash".into()), tool_input: serde_json::json!({"command": command}), raw, ..call };
        let told = report["size"].as_str().map(str::to_string).or_else(|| injected(&ran)).unwrap_or_default();
        assert!(told.contains("Grave a entrega"), "the step answers the order: {report}");
        let file = root.join("b.rs").to_string_lossy().into_owned();
        let edit = refused(root, "Edit", serde_json::json!({"file_path": file, "old_string": "a", "new_string": "b"}));
        assert!(edit.is_some(), "after the order, a new edit is refused");
    }

    /// A step whose answer already carried the measure gets no second order
    /// from the hook, even when the conversation crosses the limit right
    /// after the answer: the measure at the end of the next task decides.
    #[test]
    fn a_step_answered_below_the_limit_gets_no_other_order_from_the_hook() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let replies = vec![agent_reply(30_000, Some("read")), agent_reply(60_000, Some("edit"))];
        write_agent(root, &wave_request(Locale::PtBr), &replies);
        let call = agent_after_tool(root);
        let report = step_command_report(root, &call);
        assert!(report["size"].as_str().is_some_and(|size| size.contains("Siga")), "the step goes on: {report}");

        let crossed = vec![agent_reply(30_000, Some("read")), agent_reply(60_000, Some("edit")), agent_reply(210_000, None)];
        write_agent(root, &wave_request(Locale::PtBr), &crossed);
        let step = serde_json::json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "feita"});
        let command = format!("export MUSTARD_SESSION_ID='s1'\nexport MUSTARD_AGENT_ID='a1'\n/x/mustard-rt run write step --spec x --json '{step}'");
        let mut raw = call.raw.clone();
        raw["tool_response"] = serde_json::json!({"stdout": serde_json::to_string_pretty(&report).unwrap(), "stderr": ""});
        let ran = HookInput { tool_name: Some("Bash".into()), tool_input: serde_json::json!({"command": command}), raw, ..call.clone() };
        assert_eq!(injected(&ran), None, "the hook gives no other order for the answered step");
        assert!(wave_mark(root, &call, "signal").is_some_and(|signal| !signal.exists()), "no signal was kept");

        let next = step_command_report(root, &call);
        assert!(next["size"].as_str().is_some_and(|size| size.contains("Grave a entrega")), "the next task end decides: {next}");
    }

    /// Seeds wave 1 of `x` running, with its agent launched and alive.
    fn seed_live_wave(root: &Path) {
        seed_running_wave(root, "x");
        let path = store::spec_file(&store::spec_root(root), "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = crate::commands::flow::round::queue::open_sends(&log)[&1];
        assert!(crate::commands::flow::round::queue::record_launch(root, "x", 1, log.dispatch_position(sent)));
    }

    /// Clearing the conversation stops the agents still working: the
    /// conductor's size notice counts the waves with a live agent and says to
    /// wait for them before clearing.
    #[test]
    fn the_conductor_notice_says_to_wait_for_the_waves_with_a_live_agent() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            seed_live_wave(root);
            let transcript = root.join("t.jsonl");
            conductor_transcript_of(&transcript, 200_000);
            let notice = injected(&conductor_after_tool(root, &transcript)).expect("the notice");
            let wait = if lang == Locale::EnUs { "1 wave(s) with a live agent (1)" } else { "1 onda(s) com agente vivo (1)" };
            assert!(notice.contains(wait) && notice.contains("/clear"), "{lang:?}: {notice}");
        }
    }

    /// After a clear, the session start lists the waves whose agent the clear
    /// stopped and how to send them again.
    #[test]
    fn the_resume_after_a_clear_lists_the_waves_whose_agent_stopped() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            seed_live_wave(root);
            let start = |source: &str| {
                let input = HookInput {
                    hook_event_name: Some("SessionStart".into()),
                    session_id: Some("s1".into()),
                    cwd: Some(root.to_string_lossy().into_owned()),
                    raw: serde_json::json!({"source": source}),
                    ..HookInput::default()
                };
                match crate::dispatch::run_event(Some(Trigger::SessionStart), &input).verdict {
                    Verdict::Inject { context } => context,
                    _ => String::new(),
                }
            };
            let cleared = start("clear");
            assert!(cleared.contains("<PAUSED>{\"wave\":1}</PAUSED>"), "{lang:?}: {cleared}");
            assert!(!start("startup").contains("<PAUSED>"), "{lang:?}: only the clear stops the agents");
        }
    }

    /// Above the first step, every tool call reaches the notice; the spec is
    /// read only when the band is new.
    #[test]
    fn a_band_already_warned_does_not_read_the_spec() {
        let dir = open_project("x");
        let root = dir.path();
        let transcript = root.join("t.jsonl");
        conductor_transcript_of(&transcript, 250_000);
        let call = conductor_after_tool(root, &transcript);
        assert!(injected(&call).is_some(), "the first band warns");
        let read = std::cell::Cell::new(false);
        let again = conductor_text_with(&call, root, || {
            read.set(true);
            Some(("line".to_string(), Vec::new()))
        });
        assert_eq!(again, None, "the same band is quiet");
        assert!(!read.get(), "the warned band never reads the spec");
    }

    #[test]
    fn overlapping_notices_without_a_session_id_share_the_transcript_checkpoint() {
        let dir=open_project("x");
        let transcript=dir.path().join("t.jsonl");
        conductor_transcript_of(&transcript,300_000);
        let call=HookInput {session_id:None,..conductor_after_tool(dir.path(),&transcript)};
        let notices=std::thread::scope(|scope| {
            let threads:Vec<_>=(0..8).map(|_|scope.spawn(||injected(&call))).collect();
            threads.into_iter().filter_map(|thread|thread.join().unwrap()).count()
        });
        assert_eq!(notices,1,"same band is claimed once even without a session identity");
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
    /// resumo de uma onda entregue (pelo número da onda ou pelo código da
    /// entrega), a leitura de um item de tarefa, ou uma edição.
    fn agent_reply(tokens: u64, action: Option<&str>) -> String {
        let mut content = vec![serde_json::json!({"type": "text", "text": "certo"})];
        match action {
            Some("read") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read request-1 --root /r --spec x"}})),
            Some("summary") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read delivered-5 --root /r --spec x"}})),
            Some("summary_item") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read item-MSTD-DELIV-0004 --root /r --spec x"}})),
            Some("task_item") => content.push(serde_json::json!({"type": "tool_use", "id": "t", "name": "Bash",
                "input": {"command": "/x/mustard-rt run read item-MSTD-TASK-0001 --root /r --spec x"}})),
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

    /// A chamada `call` trocada por um passo gravado pelo terminal, de dentro
    /// da cópia: `run write step` com `item` no `--json`, e a gravação aceita
    /// ou recusada, como `ok` diz.
    fn step_of(call: HookInput, item: &str, ok: bool) -> HookInput {
        let step = serde_json::json!({"wave": 1, "item": item, "text": "feita"});
        let command = format!("cd /copy && /x/mustard-rt run write step --root /r --spec x --json '{step}'");
        let report = serde_json::to_string_pretty(&serde_json::json!({"ok": ok})).unwrap();
        let mut raw = call.raw.clone();
        raw["tool_response"] = serde_json::json!({"stdout": report, "stderr": ""});
        HookInput { tool_name: Some("Bash".to_string()), tool_input: serde_json::json!({"command": command}), raw, ..call }
    }

    /// A chamada `call` trocada pelo passo de término aceito de uma tarefa.
    fn finishing(call: HookInput) -> HookInput {
        step_of(call, "MSTD-TASK-0001", true)
    }

    /// A conversa do agente de onda `a1`, em `root`, depois de mais uma
    /// resposta com o contexto somando `tokens`, e o que o gancho de depois
    /// da ferramenta injeta quando essa resposta grava o fim de uma tarefa.
    fn agent_finishes(root: &Path, replies: &mut Vec<String>, tokens: u64, lang: Locale) -> Option<String> {
        replies.push(agent_reply(tokens, Some("edit")));
        write_agent(root, &wave_request(lang), replies);
        injected(&finishing(agent_after_tool(root)))
    }

    /// No fim de cada tarefa, o passo de término responde ao agente de onda,
    /// com a marca do Mustard, o tamanho da conversa, o limite e o gasto da
    /// maior tarefa. Começando em 30 mil, a tarefa que termina em 60 mil
    /// gastou 30 mil, e com 90 mil até o limite a ordem é seguir, sem
    /// recusa nenhuma. A que termina em 200 mil passou do limite: a ordem é
    /// gravar a entrega, com as não começadas em `undone`, e escrever nela o
    /// resumo em cinco blocos, nesta ordem — estado, feito, decidido, fatos e
    /// dúvidas —, o novo no lugar do que o agente continuou; a chamada
    /// seguinte que não lê nem grava na spec, a suíte inclusive, é recusada,
    /// com o motivo na língua do projeto e o que ainda passa. Nos dois
    /// idiomas.
    #[test]
    fn a_finished_task_is_told_to_go_on_under_the_limit_and_to_deliver_over_it() {
        use serde_json::json;
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let read = || refused(root, "Read", json!({"file_path": root.join("a.rs").to_string_lossy()}));
            let suite = || refused(root, "Bash", json!({"command": "cargo test --locked -p mustard-rt"}));
            let (go_on, deliver, numbers, only) = if lang == Locale::EnUs {
                (
                    "Go on to the next task.",
                    "Record the delivery, with the tasks not started in `undone`.",
                    "total context, 60 thousand tokens; initial input and inherited summaries, 30 thousand; work, 30 thousand. The limit is 200 thousand context tokens. The largest task added 30 thousand.",
                    "Only reading and writing the spec (`mustard-rt run read` and `run write`) and `cargo build` pass",
                )
            } else {
                (
                    "Siga para a próxima tarefa.",
                    "Grave a entrega, com as tarefas não começadas em `undone`.",
                    "contexto total, 60 mil tokens; entrada e resumos herdados, 30 mil; trabalho, 30 mil. O limite é 200 mil tokens de contexto. A maior tarefa acrescentou 30 mil.",
                    "Só passam ler e gravar na spec (`mustard-rt run read` e `run write`) e `cargo build`",
                )
            };
            let (blocks, replaces) = if lang == Locale::EnUs {
                (["State:", "Done:", "Decided:", "Facts:", "Doubts:"], "If you continued another summary, yours replaces it")
            } else {
                (["Estado:", "Feito:", "Decidido:", "Fatos:", "Dúvidas:"], "Se você continuou outro resumo, o seu o substitui")
            };
            let mut replies = vec![agent_reply(30_000, Some("read"))];

            let first = agent_finishes(root, &mut replies, 60_000, lang).unwrap_or_else(|| panic!("{lang:?}: the task end reads"));
            assert!(first.starts_with("[Mustard]") && first.contains(numbers), "{lang:?}: {first}");
            assert!(first.contains(go_on) && !first.contains(deliver), "{lang:?}: {first}");
            assert_eq!((read(), suite()), (None, None), "{lang:?}: going on, nothing is refused");

            let last = agent_finishes(root, &mut replies, 200_000, lang).unwrap_or_else(|| panic!("{lang:?}: the task end reads"));
            assert!(last.starts_with("[Mustard]") && last.contains(deliver) && !last.contains(go_on), "{lang:?}: {last}");
            let at: Vec<usize> =
                blocks.iter().map(|block| last.find(block).unwrap_or_else(|| panic!("{lang:?}: no `{block}`: {last}"))).collect();
            assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{lang:?}: the five blocks, in order: {last}");
            assert!(last.contains(replaces), "{lang:?}: the new summary replaces the one continued: {last}");
            assert!(blocks.iter().all(|block| !first.contains(block)), "{lang:?}: going on asks for no summary: {first}");
            for reason in [read(), suite()] {
                let reason = reason.unwrap_or_else(|| panic!("{lang:?}: after the order to deliver, the call is refused"));
                assert!(reason.starts_with("[Mustard]") && reason.contains(only), "{lang:?}: {reason}");
                assert!(reason.contains("`undone`"), "{lang:?}: the delivery is told: {reason}");
            }
        }
    }

    /// A ordem de entregar chega também abaixo do limite, quando o que resta
    /// até ele é menos que a maior tarefa já terminada. Começando em 30 mil,
    /// a primeira tarefa termina em 80 mil e gasta 50 mil: restam 70 mil, e a
    /// ordem é seguir. A segunda termina em 100 mil: restam 50 mil, o mesmo
    /// que a maior, e a ordem ainda é seguir. A terceira termina em 101 mil:
    /// restam 49 mil, menos que os 50 mil da maior, e a ordem é entregar, com
    /// a trava fechada.
    #[test]
    fn a_task_forecast_does_not_issue_an_early_cutoff() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let read = || refused(root, "Read", serde_json::json!({"file_path": root.join("a.rs").to_string_lossy()}));
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        for (tokens, order, locked) in [(80_000, "Siga", false), (100_000, "Siga", false), (101_000, "Siga", false), (200_000, "Grave a entrega", true)] {
            let reading = agent_finishes(root, &mut replies, tokens, Locale::PtBr).expect("the task end reads");
            assert!(reading.contains("O limite é 200 mil tokens de contexto") && reading.contains(order), "{tokens}: {reading}");
            assert_eq!(read().is_some(), locked, "{tokens}: the lock");
        }
    }

    /// No meio de uma tarefa nada manda parar, nem com a conversa acima do
    /// limite: o agente que seguiu em 60 mil chega a 158 mil sem terminar a
    /// tarefa em curso, e nenhuma chamada recebe a medida — nem o passo que
    /// prova um critério, nem o passo de término que a gravação recusou — e
    /// nenhuma é recusada, a suíte inclusive. O passo de término aceito,
    /// depois, manda entregar.
    #[test]
    fn a_wave_agent_over_the_limit_in_the_middle_of_a_task_is_neither_told_nor_refused() {
        use serde_json::json;
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        let first = agent_finishes(root, &mut replies, 60_000, Locale::PtBr).expect("the task end reads");
        assert!(first.contains("Siga"), "{first}");
        replies.push(agent_reply(190_000, Some("edit")));
        write_agent(root, &wave_request(Locale::PtBr), &replies);

        for call in [
            agent_after_tool(root),
            step_of(agent_after_tool(root), "MSTD-CRIT-0001", true),
            step_of(agent_after_tool(root), "MSTD-TASK-0001", false),
        ] {
            assert_eq!(injected(&call), None, "mid-task: {}", call.tool_input);
        }
        let file = root.join("a.rs").to_string_lossy().into_owned();
        for (tool, input) in [
            ("Bash", json!({"command": "cargo test --locked -p mustard-core -p mustard-rt -- --test-threads=8"})),
            ("Read", json!({"file_path": file})),
            ("Edit", json!({"file_path": file, "old_string": "a", "new_string": "b"})),
        ] {
            assert_eq!(refused(root, tool, input), None, "mid-task over the limit, {tool} passes");
        }
        let last = agent_finishes(root,&mut replies,205_000,Locale::PtBr).expect("the task end reads");
        assert!(last.contains("205 mil") && last.contains("Grave a entrega"), "{last}");
    }

    /// O resumo anterior sai apenas da estimativa de crescimento do trabalho.
    /// O contexto total permanece inteiro para decidir se cabe outra tarefa.
    /// A medição identifica o salto entre a resposta que lê o resumo e a seguinte,
    /// pelo número da onda (`run read delivered-<n>`) ou pelo código da
    /// entrega (`run read item-<código>`), somado quando o agente lê dois, e
    /// também quando a leitura sai junto de outra ferramenta, na mesma
    /// resposta (duas linhas com o mesmo `message.id`). Com um resumo de 100
    /// mil e contexto de 250 mil, o crescimento reportado é 150 mil; a
    /// capacidade usa os 250 mil. Ler o pedido ou uma tarefa não desconta nada.
    #[test]
    fn summary_read_is_reported_separately_from_total_context() {
        let with_id = |tokens, action, id: &str| {
            let mut reply: Value = serde_json::from_str(&agent_reply(tokens, Some(action))).unwrap();
            reply["message"]["id"] = id.into();
            reply.to_string()
        };
        for (mut replies, now, counted) in [
            (vec![agent_reply(40_000, Some("summary")), agent_reply(140_000, Some("edit"))], 250_000, 110),
            (vec![agent_reply(40_000, Some("summary_item")), agent_reply(140_000, Some("edit"))], 250_000, 110),
            (
                vec![
                    agent_reply(40_000, Some("summary")),
                    agent_reply(100_000, Some("summary_item")),
                    agent_reply(150_000, Some("edit")),
                ],
                260_000,
                110,
            ),
            (vec![with_id(40_000, "summary", "r1"), with_id(40_000, "edit", "r1"), with_id(140_000, "edit", "r2")], 250_000, 110),
            (vec![agent_reply(30_000, Some("read")), agent_reply(230_000, Some("edit"))], 230_000, 200),
            (vec![agent_reply(30_000, Some("task_item")), agent_reply(230_000, Some("edit"))], 230_000, 200),
        ] {
            let dir = open_project_in("x", Locale::PtBr);
            let reading = agent_finishes(dir.path(), &mut replies, now, Locale::PtBr).expect("the task end reads");
            assert!(reading.contains(&format!("contexto total, {} mil tokens",now/1000)) && reading.contains(&format!("trabalho, {counted} mil")), "{reading}");
        }
    }

    #[test]
    fn inherited_input_precedes_the_work_horizon() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            for total in [145_000, 160_000, 260_000] {
                let dir=open_project_in("x",lang);
                let mut replies=vec![agent_reply(10_000,Some("summary")),agent_reply(110_000,Some("edit"))];
                let text=agent_finishes(dir.path(),&mut replies,total,lang).expect("task boundary measured");
                if total<260_000 {
                    assert!(text.contains(if lang==Locale::PtBr {"Siga para a próxima tarefa"}else{"Go on to the next task"}),"{text}");
                } else {
                    assert!(text.contains(if lang==Locale::PtBr {"não comece outra tarefa"}else{"do not start another task"}),"{text}");
                    assert!(text.contains(if lang==Locale::PtBr {"testes mínimos dela"}else{"minimal tests"}),"{text}");
                }
            }
        }
    }

    #[test]
    fn fifty_thousand_initial_tokens_signal_at_two_hundred_without_stopping_current_work() {
        use serde_json::json;
        for lang in [Locale::PtBr,Locale::EnUs] {
            let dir=open_project_in("x",lang); let root=dir.path();
            let mut replies=vec![agent_reply(40_000,Some("summary")),agent_reply(50_000,Some("edit")),agent_reply(199_999,Some("edit"))];
            write_agent(root,&wave_request(lang),&replies);
            assert!(injected(&agent_after_tool(root)).is_none(),"below the 200K context limit");
            replies.push(agent_reply(200_000,Some("edit")));
            write_agent(root,&wave_request(lang),&replies);
            let signal=injected(&agent_after_tool(root)).expect("signal at the 200K context limit");
            assert!(signal.contains(if lang==Locale::PtBr {"entrada e resumos herdados, 50 mil; trabalho, 150 mil"} else {"initial input and inherited summaries, 50 thousand; work, 150 thousand"}),"{signal}");
            assert!(injected(&agent_after_tool(root)).is_none(),"signal is issued once");
            assert!(refused(root,"Edit",json!({"file_path":root.join("a.rs"),"old_string":"a","new_string":"b"})).is_none(),"current task can still be finished");
            let end=agent_finishes(root,&mut replies,215_000,lang).unwrap();
            assert!(end.contains(if lang==Locale::PtBr {"Grave a entrega"}else{"Record the delivery"}));
            assert!(refused(root,"Edit",json!({"file_path":root.join("another-task.rs")})).is_some(),"new work is refused only after task end");
            assert!(refused(root,"Bash",json!({"command":"cargo build"})).is_none());
            assert!(refused(root,REPORT_BACK_TOOL,json!({"summary":"finished"})).is_none());
        }
    }

    /// Com a janela de 1 milhão do modelo que responde ao agente e 50 mil de
    /// entrada, o sinal chega após 150 mil de trabalho, e não aos 75% da
    /// janela: 199.999 não avisa, e as chamadas seguintes só releem a
    /// conversa dos 200 mil em diante; 200 mil avisa, dizendo a janela como
    /// teto. O teto vira o sinal quando 75% da janela vem antes de entrada +
    /// 150 mil: com 650 mil de entrada e 1 milhão, aos 750 mil; com 50 mil e a
    /// janela de 200 mil, aos 150 mil. O modelo que a tabela não conhece fica
    /// em entrada + 150 mil, e o sinal diz que não há teto. Quando a conversa
    /// troca de modelo no meio, vale a janela do que respondeu por último: de
    /// um de 200 mil para um de 1 milhão, com 50 mil de entrada, os 199.999
    /// não avisam e os 200 mil avisam com a janela de 1 milhão; do de 1
    /// milhão para o de 200 mil, os 149.999 não avisam e os 150 mil avisam
    /// com a de 200 mil. Nos dois idiomas.
    #[test]
    fn the_wave_signal_is_a_fixed_context_limit_capped_by_the_model_window() {
        let named = |model: &str, tokens: u64, action: &str| {
            let mut reply: Value = serde_json::from_str(&agent_reply(tokens, Some(action))).unwrap();
            reply["message"]["model"] = model.into();
            reply.to_string()
        };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let pt = lang == Locale::PtBr;
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let threshold = root.join(".claude/.session/size-threshold-agent-a1");
            let mut replies = vec![named("claude-opus-5-5", 50_000, "read"), named("claude-opus-5-5", 199_999, "edit")];
            write_agent(root, &wave_request(lang), &replies);
            assert!(injected(&agent_after_tool(root)).is_none(), "{lang:?}: 199,999 is below the limit");
            assert_eq!(std::fs::read_to_string(&threshold).unwrap_or_default(), "", "{lang:?}: below the fixed limit the conversation is not read");
            replies.push(named("claude-opus-5-5", 200_000, "edit"));
            write_agent(root, &wave_request(lang), &replies);
            let signal = injected(&agent_after_tool(root)).unwrap_or_else(|| panic!("{lang:?}: the 1M window signals at 200 thousand"));
            let said = if pt {
                ["Janela do modelo claude-opus-5-5: 1 milhão de tokens; o teto do sinal é 75% dela.", "O limite é 200 mil tokens de contexto."]
            } else {
                ["Window of model claude-opus-5-5: 1 million tokens; the signal is capped at 75% of it.", "The limit is 200 thousand context tokens."]
            };
            assert!(said.iter().all(|said| signal.contains(said)), "{lang:?}: {signal}");

            let others = [
                ("claude-opus-5-5", 150_000, 200_000, ["1 milhão de tokens; o teto do sinal é 75% dela.", "O limite é 200 mil tokens de contexto."], ["1 million tokens; the signal is capped at 75% of it.", "The limit is 200 thousand context tokens."]),
                ("claude-sonnet-4-5-20250929", 50_000, 150_000, ["Janela do modelo claude-sonnet-4-5-20250929: 200 mil tokens;", "O limite é 150 mil tokens de contexto."], ["Window of model claude-sonnet-4-5-20250929: 200 thousand tokens;", "The limit is 150 thousand context tokens."]),
                ("claude-opus-4-20250514", 50_000, 200_000, ["Janela do modelo desconhecida: o sinal fica sem teto.", "O limite é 200 mil tokens de contexto."], ["Model window unknown: the signal has no cap.", "The limit is 200 thousand context tokens."]),
            ];
            for (model, input, at, said_pt, said_en) in others {
                let dir = open_project_in("x", lang);
                let root = dir.path();
                write_agent(root, &wave_request(lang), &[named(model, input, "read"), named(model, at - 1, "edit")]);
                assert!(injected(&agent_after_tool(root)).is_none(), "{lang:?}: {model} from {input} is quiet just below {at}");
                write_agent(root, &wave_request(lang), &[named(model, input, "read"), named(model, at, "edit")]);
                let signal = injected(&agent_after_tool(root)).unwrap_or_else(|| panic!("{lang:?}: {model} from {input} signals at {at}"));
                let said = if pt { said_pt } else { said_en };
                assert!(said.iter().all(|said| signal.contains(said)), "{lang:?}: {signal}");
            }

            let switches = [
                ("claude-sonnet-4-5-20250929", "claude-opus-5-5", 200_000, ["Janela do modelo claude-opus-5-5: 1 milhão de tokens;", "O limite é 200 mil tokens de contexto."], ["Window of model claude-opus-5-5: 1 million tokens;", "The limit is 200 thousand context tokens."]),
                ("claude-opus-5-5", "claude-sonnet-4-5-20250929", 150_000, ["Janela do modelo claude-sonnet-4-5-20250929: 200 mil tokens;", "O limite é 150 mil tokens de contexto."], ["Window of model claude-sonnet-4-5-20250929: 200 thousand tokens;", "The limit is 150 thousand context tokens."]),
            ];
            for (before, after, at, said_pt, said_en) in switches {
                let dir = open_project_in("x", lang);
                let root = dir.path();
                write_agent(root, &wave_request(lang), &[named(before, 50_000, "read"), named(after, at - 1, "edit")]);
                assert!(injected(&agent_after_tool(root)).is_none(), "{lang:?}: from {before} to {after}, quiet just below {at}");
                write_agent(root, &wave_request(lang), &[named(before, 50_000, "read"), named(after, at, "edit")]);
                let signal = injected(&agent_after_tool(root)).unwrap_or_else(|| panic!("{lang:?}: from {before} to {after}, the last model signals at {at}"));
                let said = if pt { said_pt } else { said_en };
                assert!(said.iter().all(|said| signal.contains(said)), "{lang:?}: {signal}");
            }
        }
    }

    #[test]
    fn codex_signal_uses_last_context_growth_and_not_cumulative_billing() {
        let dir=open_project("x");let root=dir.path();let path=root.join("child.jsonl");
        native_rollout(&path,"a1",&[codex_sample(50_000,40_000),codex_sample(199_999,190_000)]);
        let mut call=agent_after_tool(root);call.raw["agent_transcript_path"]=serde_json::json!(path);
        assert!(codex_event("PostToolUse",&call).is_none());
        native_rollout(&path,"a1",&[codex_sample(50_000,40_000),codex_sample(200_000,190_000)]);
        let signal=codex_event("PostToolUse",&call).unwrap();
        assert!(signal["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("Sinal de conclusão"));
        call.tool_name=Some("apply_patch".into());call.tool_input=serde_json::json!({"command":"*** Begin Patch\n*** Add File: a.rs\n+fn a() {}\n*** End Patch"});
        assert!(codex_event("PreToolUse",&call).as_ref().is_none_or(|reply|reply["hookSpecificOutput"]["permissionDecision"]!="deny"));
    }

    /// A conversa que abre com o comando de ler o pedido de outra raiz não é
    /// de uma onda deste projeto: a régua é a mesma do despacho.
    #[test]
    fn a_read_command_of_another_root_is_not_a_wave_of_this_project() {
        let dir=open_project("x");let root=dir.path();let path=root.join("child.jsonl");
        let save=|text:String| std::fs::write(&path,serde_json::json!({"type":"user","message":{"role":"user","content":text}}).to_string()).unwrap();
        save("mustard-rt run read request-1 --root /another/project --spec x".into());
        assert!(wave_opened(&path,root).is_none());
        save(format!("mustard-rt run read request-1 --root {} --spec x",mustard_core::io::wave_prompt::shown(&store::spec_root(root))));
        assert_eq!(wave_opened(&path,root),Some(("x".to_string(),1)));
    }

    #[test]
    fn codex_summary_read_is_part_of_initial_input() {
        let dir=open_project("x");let root=dir.path();let path=root.join("child.jsonl");
        let read=serde_json::json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command",
            "arguments":serde_json::json!({"cmd":"mustard-rt run read delivered-1 --spec x"}).to_string(),"call_id":"summary"}});
        native_rollout(&path,"a1",&[codex_sample(40_000,30_000),read,codex_sample(50_000,40_000),codex_sample(200_000,190_000)]);
        let measured=wave_context(&[path],root).unwrap();
        assert_eq!(measured.baseline(),50_000);
        assert_eq!(measured.growth(),150_000);
    }

    /// Uma amostra de uso do Codex com a janela `window` declarada pelo
    /// registro da conversa.
    fn declared_sample(input: u64, window: u64) -> Value {
        let mut sample = codex_sample(input, input - 10_000);
        sample["payload"]["info"]["model_context_window"] = window.into();
        sample
    }

    /// O aviso depois da ferramenta ao agente de onda do Codex cuja conversa,
    /// no projeto `root`, traz as amostras `rows`.
    fn codex_wave_notice(root: &Path, rows: &[Value]) -> Option<String> {
        let path = root.join("child.jsonl");
        native_rollout(&path, "a1", rows);
        let mut call = agent_after_tool(root);
        call.raw["agent_transcript_path"] = serde_json::json!(path);
        codex_event("PostToolUse", &call).map(|reply| reply["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_owned())
    }

    /// Com 1 milhão declarado na conversa do Codex e 50 mil de entrada, o
    /// sinal segue em entrada + 150 mil: os 199.999 não avisam, e os 200 mil
    /// avisam dizendo a janela declarada como teto; a mesma conversa sem o
    /// campo avisa nos mesmos 200 mil, com a janela desconhecida.
    #[test]
    fn codex_declared_window_above_the_limit_keeps_the_fixed_limit() {
        let dir = open_project("x");
        let root = dir.path();
        let mut rows = vec![declared_sample(50_000, 1_000_000), declared_sample(199_999, 1_000_000)];
        assert_eq!(codex_wave_notice(root, &rows), None, "199,999 is below the limit");
        rows.push(declared_sample(200_000, 1_000_000));
        let signal = codex_wave_notice(root, &rows).expect("the declared 1M window signals at 200 thousand");
        let said = ["Janela do modelo declarada pela conversa: 1 milhão de tokens; o teto do sinal é 75% dela.", "O limite é 200 mil tokens de contexto."];
        assert!(said.iter().all(|said| signal.contains(said)), "{signal}");

        let dir = open_project("x");
        let signal = codex_wave_notice(dir.path(), &[codex_sample(50_000, 40_000), codex_sample(200_000, 190_000)])
            .expect("without the declared window the signal comes at 200 thousand");
        let said = ["Janela do modelo desconhecida: o sinal fica sem teto.", "O limite é 200 mil tokens de contexto."];
        assert!(said.iter().all(|said| signal.contains(said)), "{signal}");
    }

    /// Uma janela declarada cujo teto vem antes de entrada + 150 mil — 128
    /// mil, teto aos 96 mil, com 30 mil de entrada — avisa no teto, antes dos
    /// 150 mil com que as chamadas sem medida deixam de reler a conversa.
    #[test]
    fn codex_declared_window_below_the_limit_signals_at_its_cap() {
        let dir = open_project("x");
        let root = dir.path();
        let mut rows = vec![declared_sample(30_000, 128_000), declared_sample(95_999, 128_000)];
        assert_eq!(codex_wave_notice(root, &rows), None);
        rows.push(declared_sample(96_000, 128_000));
        let signal = codex_wave_notice(root, &rows).expect("75% of the declared 128 thousand is 96 thousand");
        let said = ["Janela do modelo declarada pela conversa: 128 mil tokens; o teto do sinal é 75% dela.", "O limite é 96 mil tokens de contexto."];
        assert!(said.iter().all(|said| signal.contains(said)), "{signal}");
    }

    /// A janela declarada como zero não é janela: com 50 mil de entrada, a
    /// conversa do Codex que a traz não avisa na primeira amostra nem nos
    /// 199.999, e avisa nos 200 mil como a que não declara nenhuma, com a
    /// janela desconhecida.
    #[test]
    fn codex_zero_declared_window_counts_as_no_window() {
        let dir = open_project("x");
        let root = dir.path();
        let mut rows = vec![declared_sample(50_000, 0)];
        assert_eq!(codex_wave_notice(root, &rows), None, "a zero window does not signal on the first sample");
        rows.push(declared_sample(199_999, 0));
        assert_eq!(codex_wave_notice(root, &rows), None, "a zero window is no cap");
        rows.push(declared_sample(200_000, 0));
        let signal = codex_wave_notice(root, &rows).expect("a zero window leaves the signal at 200 thousand");
        let said = ["Janela do modelo desconhecida: o sinal fica sem teto.", "O limite é 200 mil tokens de contexto."];
        assert!(said.iter().all(|said| signal.contains(said)), "{signal}");
    }

    #[test]
    fn a_signal_survives_a_smaller_context_until_the_current_task_returns() {
        let dir=open_project("x");let root=dir.path();
        let mut replies=vec![agent_reply(50_000,Some("read")),agent_reply(200_000,Some("edit"))];
        write_agent(root,&wave_request(Locale::PtBr),&replies);
        assert!(injected(&agent_after_tool(root)).unwrap().contains("Sinal de conclusão"));
        let end=agent_finishes(root,&mut replies,80_000,Locale::PtBr).unwrap();
        assert!(end.contains("Grave a entrega") && !end.contains("Siga para a próxima tarefa"),"a smaller context cannot revoke the previous signal: {end}");
    }

    /// O pedaço da conversa do agente `a1` na sessão `u`, que um `/clear` de
    /// quem conduz abriu depois da sessão `t` de [`write_agent`]: abre com um
    /// resultado de ferramenta e traz uma resposta por tamanho de `sizes`.
    /// Devolve a chamada de ferramenta do agente nessa sessão, depois dela.
    fn agent_after_clear(root: &Path, sizes: &[u64]) -> HookInput {
        let dir = root.join("u").join("subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let opening = serde_json::json!({"type": "user", "isSidechain": true,
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}]}});
        let mut text = format!("{opening}\n");
        for size in sizes {
            text.push_str(&agent_reply(*size, Some("edit")));
            text.push('\n');
        }
        std::fs::write(dir.join("agent-a1.jsonl"), text).unwrap();
        let transcript = root.join("u.jsonl");
        conductor_transcript_of(&transcript, 50_000);
        HookInput { session_id: Some("s2".to_string()), agent_id: Some("a1".to_string()), ..conductor_after_tool(root, &transcript) }
    }

    /// A conversa do agente de onda atravessou um `/clear` de quem conduz: o
    /// pedaço da sessão antiga abre com o título do pedido e traz o resumo de
    /// 100 mil que o agente leu; o da sessão atual abre com um resultado de
    /// ferramenta. A ordem de entregar dada na sessão antiga, em 260 mil (160
    /// mil sem o resumo), segue trancando a sessão nova, e o fim de tarefa
    /// nela tira da conta o resumo lido no pedaço antigo.
    #[test]
    fn the_order_to_deliver_before_a_clear_keeps_the_lock_after_it() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let mut replies = vec![agent_reply(40_000, Some("summary")), agent_reply(140_000, Some("edit"))];
        let order = agent_finishes(root, &mut replies, 290_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("trabalho, 150 mil") && order.contains("Grave a entrega"), "{order}");

        let read = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({"file_path": root.join("a.rs").to_string_lossy()}),
            ..agent_after_clear(root, &[291_000])
        };
        let verdict = crate::dispatch::run_event(Some(Trigger::PreToolUse), &read).verdict;
        assert!(matches!(verdict, Verdict::Deny { .. }), "the lock survives the clear: {verdict:?}");
        let again = injected(&finishing(agent_after_clear(root, &[291_000]))).expect("the task end reads after the clear");
        assert!(again.contains("291 mil tokens") && again.contains("trabalho, 151 mil"), "{again}");
    }

    /// O motivo com que o gancho de antes da ferramenta recusa a chamada de
    /// `tool` do subagente `a1`, pelo despachante e pelo registro. `None`
    /// quando a chamada passa, com ou sem o prazo que a trava de comandos
    /// acrescenta ao terminal.
    fn refused(root: &Path, tool: &str, input: serde_json::Value) -> Option<String> {
        let call = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some(tool.to_string()),
            tool_input: input,
            ..agent_after_tool(root)
        };
        match crate::dispatch::run_event(Some(Trigger::PreToolUse), &call).verdict {
            Verdict::Deny { reason } => Some(reason),
            Verdict::Allow | Verdict::Rewrite { .. } => None,
            other => panic!("the size hook only refuses or lets through: {other:?}"),
        }
    }

    /// Depois da ordem de entregar, só o terminal passa, e só para ler ou
    /// gravar na spec (`mustard-rt run read` e `run write`, pelo caminho ou
    /// pelo nome, de dentro da cópia ou não) — ler o item do pedido que
    /// faltou deixa a entrega passar na conferência de leitura — e para o
    /// comando de compilar do projeto (com `rtk` na frente, com o programa
    /// pelo caminho completo, com variáveis na frente ou com a saída ligada a
    /// um `tail`). Todo o resto é recusado: as outras ferramentas, a suíte, o
    /// terminal comum. E só vale para o agente de onda que recebeu a ordem:
    /// quem conduz (mesmo com 450 mil tokens) e o subagente que não é de onda
    /// nunca recebem a medida nem são recusados.
    #[test]
    fn after_the_order_to_deliver_only_the_spec_commands_and_the_build_pass_and_only_for_that_wave_agent() {
        use serde_json::json;
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        let order = agent_finishes(root, &mut replies, 400_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("Grave a entrega"), "the order to deliver: {order}");

        let bash = |command: &str| refused(root, "Bash", json!({"command": command}));
        for command in [
            "/x/mustard-rt run write step --root /r --spec x --json '{\"text\":\"a && b; c\"}'",
            "mustard-rt run write delivered --json '{}'",
            "cd /copy && /x/mustard-rt run write delivered --root /r --spec x --json '{}'",
            "/x/mustard-rt run read item-MSTD-TASK-0001 --root /r --spec x",
            "cd /copy && mustard-rt run read request-1 --root /r --spec x",
            "cargo build",
            "cargo build -j 4",
            "rtk cargo build",
            "cd /copy && cargo build -j 4 2>&1 | tail -20",
            "$HOME/.cargo/bin/cargo build",
            "/home/u/.cargo/bin/cargo build -j 4 2>&1 | tail -n 30",
            "PATH=\"$HOME/.cargo/bin:$PATH\" cargo build",
            "cd /copy && CARGO_TARGET_DIR=/t rtk cargo build",
        ] {
            assert_eq!(bash(command), None, "{command} passes after the order");
        }
        for command in [
            "cargo test --locked -p mustard-rt",
            "$HOME/.cargo/bin/cargo test",
            "PATH=/p cargo clippy",
            "cargo clippy",
            "ls",
            "/x/mustard-rt run map search a",
            "echo mustard-rt run write",
            "cd /copy",
        ] {
            let reason = bash(command).unwrap_or_else(|| panic!("{command} is refused after the order"));
            assert!(reason.starts_with("[Mustard]"), "{command}: {reason}");
        }
        let file = root.join("a.rs").to_string_lossy().into_owned();
        for (tool, input) in [
            ("Read", json!({"file_path": file})),
            ("Edit", json!({"file_path": file, "old_string": "a", "new_string": "b"})),
            ("Grep", json!({"pattern": "a"})),
            ("Glob", json!({"pattern": "*.rs"})),
            ("Agent", json!({"description": "d", "prompt": "p"})),
        ] {
            assert!(refused(root, tool, input).is_some(), "{tool} is refused after the order");
        }

        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let transcript = root.join("t.jsonl");
        conductor_transcript_of(&transcript, 450_000);
        let conductor = HookInput { hook_event_name: Some("PreToolUse".to_string()), ..conductor_call(root, &transcript) };
        let outcome = crate::dispatch::run_event(Some(Trigger::PreToolUse), &conductor);
        assert!(!outcome.is_blocking(), "the conductor is not refused by size: {outcome:?}");

        let replies: Vec<String> = (1..=12).map(|n| agent_reply(100_000 + n * 20_000, Some("edit"))).collect();
        write_agent(root, "Explore o repositório e conte os arquivos.", &replies);
        assert_eq!(injected(&finishing(agent_after_tool(root))), None, "another kind of agent gets no reading");
        assert_eq!(refused(root, "Read", json!({"file_path": root.join("a.rs").to_string_lossy()})), None);
    }

    /// Depois da ordem de entregar, cada comando composto precisa continuar
    /// restrito ao encerramento. Substituições, comandos alheios e pipelines
    /// que executam trabalho novo são recusados. JSON entre aspas é só texto.
    #[test]
    fn after_the_order_to_deliver_another_command_on_the_same_line_is_refused() {
        use serde_json::json;
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        let order = agent_finishes(root, &mut replies, 400_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("Grave a entrega"), "the order to deliver: {order}");

        let bash = |command: &str| refused(root, "Bash", json!({"command": command}));
        assert_eq!(bash("mustard-rt run write step --json '{\"text\":\"a && b; c | d $(e) `f`\"}'"), None);
        for command in [
            "mustard-rt run write step --json {} | sh",
            "mustard-rt run write step --json \"$(cargo test)\"",
            "mustard-rt run write step --json `cargo test`",
            "cd $(cargo test) && mustard-rt run write step --json {}",
            "PATH=/p cargo build $(cargo test)",
            "cargo build | xargs cargo test",
            "cargo build 2>&1 | tail -20 src/a.rs",
        ] {
            let reason = bash(command).unwrap_or_else(|| panic!("{command} is refused after the order"));
            assert!(reason.starts_with("[Mustard]"), "{command}: {reason}");
        }
    }

    #[test]
    fn capacity_horizon_does_not_interrupt_work_and_declared_minimal_checks_remain_available() {
        use serde_json::json;
        let dir=open_project("x");
        let root=dir.path();
        std::fs::write(root.join("mustard.json"),r#"{"language":{"text":"pt-BR"},"buildCommand":"cargo build","testCommand":"cargo test --locked -p app","lintCommand":"cargo clippy -p app"}"#).unwrap();
        let mut replies=vec![agent_reply(30_000,Some("read")),agent_reply(190_000,Some("edit"))];
        write_agent(root,&wave_request(Locale::PtBr),&replies);
        assert_eq!(injected(&agent_after_tool(root)),None,"no stop order in the middle of a task");
        assert_eq!(refused(root,"Edit",json!({"file_path":root.join("a.rs"),"old_string":"a","new_string":"b"})),None);
        assert!(agent_finishes(root,&mut replies,200_000,Locale::PtBr).is_some());
        for command in ["cargo test --locked -p app specific_test","rtk cargo test --locked -p app specific_test","cargo clippy -p app", "cargo build && cargo test --locked -p app specific_test && cargo clippy -p app", "mustard-rt run read request-1 && cargo test --locked -p app"] {
            assert_eq!(refused(root,"Bash",json!({"command":command})),None,"{command}");
        }
        assert!(refused(root,"Agent",json!({"description":"next task","prompt":"start next task"})).is_some());
        assert_eq!(refused(root,REPORT_BACK_TOOL,json!({"summary":"finished"})),None);
    }

    #[test]
    fn a_wave_can_run_its_own_criterion_proof_after_the_capacity_horizon() {
        let dir = open_project("x"); let root = dir.path();
        let criterion = seed_running_wave(root, "x");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let mut fields = log.get(criterion).unwrap().fields.clone();
        fields.remove("code"); fields.remove("search");
        fields.insert("replaces".into(), serde_json::json!(criterion));
        fields.insert("proof".into(), serde_json::json!("./node_modules/.bin/jest --roots=src src/example.spec.ts"));
        store::write(&path, "criterion", fields, &[]).unwrap();
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        assert!(agent_finishes(root, &mut replies, 200_000, Locale::PtBr).is_some());
        assert_eq!(refused(root, "Bash", serde_json::json!({"command":"./node_modules/.bin/jest --roots=src src/example.spec.ts"})), None);
        assert!(refused(root, "Edit", serde_json::json!({"file_path":root.join("another-task.rs")})).is_some());
    }

    /// Depois da ordem de entregar, a devolução do relatório a quem conduz
    /// passa: o agente que gravou a entrega termina com o relatório entregue.
    /// No mesmo instante, a leitura de um arquivo segue recusada — a trava
    /// está fechada.
    #[test]
    fn after_the_order_to_deliver_the_report_back_to_the_conductor_passes() {
        use serde_json::json;
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        let order = agent_finishes(root, &mut replies, 400_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("Grave a entrega"), "the order to deliver: {order}");

        assert!(refused(root, "Read", json!({"file_path": root.join("a.rs").to_string_lossy()})).is_some(), "the lock is closed");
        assert_eq!(refused(root, REPORT_BACK_TOOL, json!({"message": "Onda 1 entregue."})), None, "the report back passes");
    }

    /// A medida do fim de tarefa fica calada onde não é de uma onda: na
    /// sessão principal, que só tem o aviso de quem conduz (e abaixo do degrau
    /// dele, nada), num subagente cujo primeiro texto não é o título de um
    /// pedido de onda — por maior que a conversa dele esteja, também quando
    /// um `/clear` de quem conduz a repartiu em dois pedaços —, e num pedido
    /// de onda no idioma que o projeto não usa.
    #[test]
    fn the_task_end_reading_is_quiet_outside_a_wave() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let replies = vec![agent_reply(40_000, Some("edit")), agent_reply(300_000, Some("edit"))];

        write_agent(root, &wave_request(Locale::PtBr), &replies);
        let main = finishing(conductor_after_tool(root, &root.join("t.jsonl")));
        conductor_transcript_of(&root.join("t.jsonl"), 150_000);
        assert_eq!(injected(&main), None, "the main session is not a wave agent");

        for opening in ["Explore o repositório e conte os arquivos.", "# x — wave 1\n\nThe request.", "olá\n# x — onda 1"] {
            write_agent(root, opening, &replies);
            assert_eq!(injected(&finishing(agent_after_tool(root))), None, "{opening:?} is not a wave request");
        }
        write_agent(root, "Explore o repositório e conte os arquivos.", &replies);
        assert_eq!(
            injected(&finishing(agent_after_clear(root, &[200_000, 300_000]))),
            None,
            "a plain agent split by a clear gets no reading"
        );
        write_agent(root, &wave_request(Locale::PtBr), &replies);
        assert!(injected(&finishing(agent_after_tool(root))).is_some(), "the same conversation, opened by a wave request, reads");
    }

    /// O conserto que a rodada devolve abre a trava do agente que recebeu a
    /// ordem de entregar. Com a volta gravada e ainda sem recusa, ler, editar
    /// e rodar a suíte seguem recusados; com o trecho de conserto da volta em
    /// disco, os três passam; depois da entrega nova, os três voltam a ser
    /// recusados, com o trecho velho ainda em disco.
    #[test]
    fn the_fix_the_round_sends_back_opens_the_lock_until_the_next_delivery() {
        use serde_json::json;
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        seed_running_wave(root, "x");
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        let order = agent_finishes(root, &mut replies, 400_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("Grave a entrega"), "the order to deliver: {order}");
        let file = root.join("a.rs").to_string_lossy().into_owned();
        let calls = || {
            [
                refused(root, "Read", json!({"file_path": file})),
                refused(root, "Edit", json!({"file_path": file, "old_string": "a", "new_string": "b"})),
                refused(root, "Bash", json!({"command": "cargo test --locked -p mustard-rt"})),
            ]
        };
        let delivery = json!({"wave": 1, "text": "Feito.", "files": ["a.rs"], "returned": true, "author": "wave"});

        seed(root, "delivered", delivery.clone());
        assert!(calls().iter().all(Option::is_some), "a return the round has not refused keeps the lock");

        let fix = crate::commands::flow::round::fix_file(root, "x", &read_log(root), 1).expect("the pending return");
        std::fs::create_dir_all(fix.parent().unwrap()).unwrap();
        std::fs::write(&fix, "Onda 1, rodada de conserto 1 de 2:\n- falta o teste da regra.\n").unwrap();
        assert_eq!(calls(), [None, None, None], "the refused return opens the lock for the fix");

        seed(root, "delivered", delivery);
        assert!(fix.is_file(), "the old section is still on disk");
        assert!(calls().iter().all(Option::is_some), "the new delivery closes the lock again");
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
                assert!(context.contains("mustard-rt run round --dispatch --spec x"), "{lang:?} {moment}: {context}");
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
            let codes = seed_decisions_after_round(root, said, 450);

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
            let waves: Vec<u64> = (2..152).collect();
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
            assert!(block.contains("mustard-rt run round --dispatch --spec x"), "{lang:?}: the next command: {block}");
        }
    }
}
