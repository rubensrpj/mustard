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
//!   ferramenta, a conversa que passa de [`CONDUCTOR_STEP`] tokens recebe, uma
//!   vez por degrau, o aviso de limpar ou compactar, com o bloco de retomada
//!   pronto para colar numa janela limpa; o degrau guardado acompanha a
//!   conversa que encolheu, então um `/compact` de verdade não cala o aviso do
//!   próximo degrau. Quem conduz nunca é recusado por tamanho. **No agente de
//!   onda** (o subagente cujo primeiro texto é o título de um pedido de onda),
//!   a medida chega só no fim de cada tarefa: no resultado do passo de término
//!   (`run write step` com o código de uma tarefa no `item`), o gancho de
//!   depois da ferramenta diz o tamanho da conversa, sem o resumo da onda
//!   anterior que ele leu, o limite de [`WAVE_LIMIT`] tokens e a ordem de
//!   seguir para a próxima tarefa ou de entregar. Manda entregar quando o
//!   tamanho passou do limite, ou quando o que resta até ele é menos que o
//!   gasto da maior tarefa já terminada na conversa: a diferença de tamanho
//!   entre dois passos de término seguidos, e a da primeira desde o começo.
//!   A ordem de entregar fecha a trava: dali em diante, o gancho de antes da
//!   ferramenta recusa tudo, menos `run read` e `run write` na spec e o
//!   comando de compilar do projeto, cada um sem outro comando na mesma
//!   linha. Enquanto a rodada espera o conserto da volta que recusou — o
//!   trecho de conserto dela está em disco —, a trava fica aberta, e a
//!   entrega nova a fecha de novo. Antes da ordem nada é recusado por
//!   tamanho, nem a conversa acima do limite no meio de uma tarefa. O resumo
//!   é o salto do tamanho entre a resposta que chama `run read delivered-<n>` ou `run read item-<código da entrega>` —
//!   o comando que o pedido da onda manda — e a resposta seguinte, somado
//!   quando o agente lê mais de um; sem resumo lido, conta a conversa inteira.
//!   O agente de onda nunca recebe o aviso de quem conduz, e quem conduz nunca
//!   recebe o da onda.
//!
//! O tamanho é a soma de `input_tokens`, `cache_read_input_tokens` e
//! `cache_creation_input_tokens` do último uso gravado na conversa. A de quem
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

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::mustard_id;
use mustard_core::domain::spec_events::type_spec;
use mustard_core::domain::wave_prompt::wave_of_title;
use mustard_core::io::spec_events as store;
use mustard_core::io::transcript::{agent_pieces, heading_of};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::{translate, ClaudePaths};
use serde_json::Value;

use crate::hooks::bash::lex::{segments, Segment};

/// O degrau de tamanho da conversa de quem conduz, em tokens: a cada degrau
/// novo, um aviso.
pub(crate) const CONDUCTOR_STEP: u64 = 200_000;

/// O tamanho que a conversa do agente de onda pode ter, em tokens, sem contar
/// o resumo da onda anterior que ele leu: o fim de tarefa que passou dele, ou
/// que deixa até ele menos que a maior tarefa feita, manda entregar.
pub(crate) const WAVE_LIMIT: u64 = 150_000;

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
/// agente de onda que recebeu a ordem de entregar é recusado.
pub struct SizeNotice;

impl Check for SizeNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        match ctx.trigger {
            Some(Trigger::PostToolUse) => {
                let context = if input.is_subagent() {
                    task_end_text(input, &root, ctx.config.language().text_or_default())
                } else {
                    conductor_text(input, &root)
                };
                Ok(context.map_or(Verdict::Allow, |context| Verdict::Inject { context }))
            }
            Some(Trigger::PreToolUse) => {
                Ok(wave_lock_reason(input, &root, ctx).map_or(Verdict::Allow, |reason| Verdict::Deny { reason }))
            }
            _ => Ok(Verdict::Allow),
        }
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
    /// O tamanho da conversa na primeira resposta: o começo dela.
    first: u64,
    /// O tamanho da conversa na última resposta.
    now: u64,
    /// Quanto do tamanho é o resumo da onda anterior que o agente leu.
    summary: u64,
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

/// A spec e a onda do pedido com que a conversa em `path` abre, no idioma
/// `lang`: a primeira mensagem do usuário nela é a que vale. `None` quando
/// ela não abre com o título de um pedido de onda.
fn wave_opened(path: &Path, lang: Locale) -> Option<(String, u64)> {
    use std::io::BufRead;

    let file = std::fs::File::open(path).ok()?;
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| user_text(&serde_json::from_str::<Value>(&line).ok()?))
        .and_then(|first| wave_of_title(heading_of(&first), lang))
}

/// A leitura da conversa do agente, nos pedaços `pieces`, do mais antigo ao
/// mais novo ([`agent_pieces`]): `None` quando o mais antigo não abre com o
/// título de um pedido de onda no idioma `lang` (um agente qualquer), ou
/// quando ainda não há nenhum uso gravado. O começo é o primeiro uso, e o
/// tamanho de agora, o último, o do pedaço da sessão atual, que é o mais
/// novo. O resumo pesa o salto do
/// tamanho entre a resposta que o lê (veja [`reads_summary`]) e a resposta
/// seguinte — a linha seguinte com o mesmo `message.id` é da mesma resposta
/// —, também quando o resumo foi lido num pedaço anterior.
fn wave_context(pieces: &[PathBuf], lang: Locale) -> Option<WaveContext> {
    use std::io::BufRead;

    wave_opened(pieces.first()?, lang)?;
    let (mut first, mut now, mut summary, mut before) = (None, None, 0_u64, None::<(Option<String>, u64)>);
    for piece in pieces {
        let Ok(file) = std::fs::File::open(piece) else { continue };
        for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
            let Some((value, usage)) = usage_of_line(&line) else { continue };
            let context = context_of(&usage);
            first.get_or_insert(context);
            now = Some(context);
            let message = value.get("message");
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
    Some(WaveContext { first: first?, now: now?, summary })
}

/// O pedaço da conversa do subagente de `input` na sessão atual, no arquivo
/// que o Claude Code guarda numa pasta `subagents/` ao lado da conversa
/// principal. `None` fora de um subagente ou sem o caminho da conversa
/// principal.
fn agent_transcript(input: &HookInput) -> Option<PathBuf> {
    let transcript = input.transcript_path()?;
    let name = input.subagent_transcript_name()?;
    let session_dir = transcript.strip_suffix(".jsonl").unwrap_or(transcript);
    Some(Path::new(session_dir).join("subagents").join(name))
}

/// Os dois arquivos de estado do agente de onda de `input`: o tamanho da
/// conversa, sem o resumo lido, em cada passo de término, um por linha, e a
/// ordem de entregar, que só existe depois dela. Ficam pelo nome do agente,
/// direto em `.claude/.session/`, fora da pasta de qualquer sessão: o
/// `/clear` de quem conduz troca a sessão no meio da onda, e nem as tarefas
/// já medidas nem a ordem dada antes dele se perdem. `None` fora de um
/// subagente.
fn wave_marks(root: &Path, input: &HookInput) -> Option<(PathBuf, PathBuf)> {
    let name = input.subagent_transcript_name()?;
    let agent = name.trim_end_matches(".jsonl");
    let mark = |kind: &str| {
        let name = format!("size-{kind}-{agent}");
        Some(state_dir(root)?.join(is_plain(&name).then_some(name)?))
    };
    Some((mark("steps")?, mark("deliver")?))
}

/// Se a chamada `input` gravou o fim de uma tarefa: o terminal, com um
/// comando só além dos `cd`, `mustard-rt run write step`, cujo `--json` traz
/// no `item` o código de uma tarefa, e cuja saída não é a recusa da gravação.
fn finishes_a_task(input: &HookInput) -> bool {
    if input.tool_name.as_deref() != Some("Bash") {
        return false;
    }
    let Some(command) = input.tool_input.get("command").and_then(Value::as_str) else { return false };
    let mut run = segments(command).into_iter().filter(|segment| program_name(segment) != "cd");
    let Some(main) = run.next() else { return false };
    let args: Vec<&str> = main.args.iter().map(|word| word.text.as_str()).collect();
    if program_name(&main) != "mustard-rt" || !args.starts_with(&["run", "write", "step"]) || run.next().is_some() {
        return false;
    }
    let json = args
        .windows(2)
        .find_map(|pair| (pair[0] == "--json").then_some(pair[1]))
        .or_else(|| args.iter().find_map(|arg| arg.strip_prefix("--json=")));
    let task = type_spec("task").map(|spec| spec.code);
    let names_a_task = json
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .and_then(|step| step.get("item")?.as_str().and_then(mustard_id::parse).map(|(kind, _)| Some(kind) == task))
        .unwrap_or(false);
    let stdout = input.raw.get("tool_response").and_then(|response| response.get("stdout")).and_then(Value::as_str);
    let refused = stdout
        .and_then(|stdout| serde_json::from_str::<Value>(stdout).ok())
        .is_some_and(|report| report.get("ok") == Some(&Value::Bool(false)));
    names_a_task && !refused
}

/// A medida do fim de tarefa ao agente de onda: no resultado do passo de
/// término ([`finishes_a_task`]) do subagente de `input`, o tamanho da
/// conversa, sem o resumo que ele leu, o limite, o gasto da maior tarefa já
/// terminada e a ordem. Manda entregar quando o tamanho passou de
/// [`WAVE_LIMIT`], quando o que resta até ele é menos que aquele gasto, ou
/// quando a ordem já saiu; senão, manda seguir. Guarda o tamanho do passo e,
/// com a ordem de entregar, fecha a trava ([`wave_lock_reason`]). `None` fora
/// de uma onda e em toda chamada que não termina uma tarefa: no meio dela,
/// nada a manda parar.
fn task_end_text(input: &HookInput, root: &Path, lang: Locale) -> Option<String> {
    if !finishes_a_task(input) {
        return None;
    }
    let context = wave_context(&agent_pieces(&agent_transcript(input)?), lang)?;
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
    let deliver = order.exists() || counted > WAVE_LIMIT || WAVE_LIMIT.saturating_sub(counted) < largest;
    if deliver {
        write_mark(&order, counted);
    }
    let thousands = |tokens: u64| (tokens / 1000).to_string();
    let key = if deliver { "conversation_size.wave_deliver" } else { "conversation_size.wave_continue" };
    Some(
        translate(key, lang)
            .replace("{now}", &thousands(context.now))
            .replace("{counted}", &thousands(counted))
            .replace("{limit}", &thousands(WAVE_LIMIT))
            .replace("{largest}", &thousands(largest)),
    )
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

/// Se o agente de onda ainda pode fazer a chamada `input` depois da ordem de
/// entregar: só o terminal, com um comando só além dos `cd` — ler ou gravar
/// na spec, ou compilar com `build`, cuja saída pode ir a um `tail` ou
/// `head`. O comando é lido como o shell o lê: outro comando na mesma linha,
/// encadeado (`&&`, `;`, `|`, nova linha) ou escondido numa palavra (`$(…)`,
/// crase), recusa.
fn passes_after_the_order(input: &HookInput, build: Option<&str>) -> bool {
    if input.tool_name.as_deref() != Some("Bash") {
        return false;
    }
    let Some(command) = input.tool_input.get("command").and_then(Value::as_str) else { return false };
    let mut run = segments(command).into_iter().filter(|segment| program_name(segment) != "cd");
    let Some(main) = run.next() else { return false };
    if uses_the_spec(&main) {
        return run.next().is_none();
    }
    let build = build.and_then(|build| segments(build).into_iter().next());
    build.is_some_and(|build| runs_build(&main, &build)) && run.all(|segment| trims_the_output(&segment))
}

/// Se a rodada recusou a volta da onda do agente de `input` e espera o
/// conserto dele: o pedaço mais antigo da conversa abre com o título do
/// pedido da onda no idioma `lang`, e o trecho de conserto da volta pendente
/// dela ([`fix_file`](crate::commands::flow::round::fix_file)) está em disco,
/// na spec de `root`. A entrega nova muda o nome do trecho, e a resposta
/// volta a ser `false` sozinha, sem marca a apagar.
fn awaits_its_fix(input: &HookInput, root: &Path, lang: Locale) -> bool {
    let pieces = agent_transcript(input).map(|transcript| agent_pieces(&transcript)).unwrap_or_default();
    let Some((spec, wave)) = pieces.first().and_then(|first| wave_opened(first, lang)) else { return false };
    let root = store::spec_root(root);
    let log = store::spec_file(&root, &spec).ok().and_then(|path| store::read(&path).ok().flatten());
    log.and_then(|log| crate::commands::flow::round::fix_file(&root, &spec, &log, wave)).is_some_and(|file| file.is_file())
}

/// A recusa ao agente de onda depois da ordem de entregar: a trava fechada
/// por [`task_end_text`], e a chamada `input` não é ler ou gravar na spec nem
/// o comando de compilar do projeto ([`passes_after_the_order`]). `None`
/// fora de um agente de onda, antes da ordem — por maior que a conversa
/// esteja —, para o que passa e enquanto a rodada espera o conserto da volta
/// que recusou ([`awaits_its_fix`]): quem conserta é o agente que fez a onda,
/// mesmo acima do limite, e a entrega nova fecha a trava de novo.
fn wave_lock_reason(input: &HookInput, root: &Path, ctx: &Ctx) -> Option<String> {
    if !input.is_subagent() {
        return None;
    }
    let (_, order) = wave_marks(root, input)?;
    if !order.exists() {
        return None;
    }
    let build = ctx.config.commands().build;
    if passes_after_the_order(input, build.as_deref()) {
        return None;
    }
    let lang = ctx.config.language().text_or_default();
    if awaits_its_fix(input, root, lang) {
        return None;
    }
    let build = build.map_or_else(String::new, |build| translate("conversation_size.wave_locked_build", lang).replace("{command}", &build));
    Some(translate("conversation_size.wave_locked", lang).replace("{build}", &build))
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
                    "60 thousand. The limit is 150 thousand, and the largest task took 30 thousand.",
                    "Only reading and writing the spec (`mustard-rt run read` and `run write`) and `cargo build` pass",
                )
            } else {
                (
                    "Siga para a próxima tarefa.",
                    "Grave a entrega, com as tarefas não começadas em `undone`.",
                    "60 mil. O limite é 150 mil, e a maior tarefa gastou 30 mil.",
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
    fn a_finished_task_is_told_to_deliver_when_what_is_left_is_less_than_the_largest_task() {
        let dir = open_project_in("x", Locale::PtBr);
        let root = dir.path();
        let read = || refused(root, "Read", serde_json::json!({"file_path": root.join("a.rs").to_string_lossy()}));
        let mut replies = vec![agent_reply(30_000, Some("read"))];
        for (tokens, order, locked) in [(80_000, "Siga", false), (100_000, "Siga", false), (101_000, "Grave a entrega", true)] {
            let reading = agent_finishes(root, &mut replies, tokens, Locale::PtBr).expect("the task end reads");
            assert!(reading.contains("a maior tarefa gastou 50 mil") && reading.contains(order), "{tokens}: {reading}");
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
        replies.push(agent_reply(158_000, Some("edit")));
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
        let last = injected(&finishing(agent_after_tool(root))).expect("the task end reads");
        assert!(last.contains("158 mil") && last.contains("Grave a entrega"), "{last}");
    }

    /// O resumo da onda anterior que o agente leu sai da conta do fim de
    /// tarefa: o salto do tamanho entre a resposta que o lê e a seguinte,
    /// pelo número da onda (`run read delivered-<n>`) ou pelo código da
    /// entrega (`run read item-<código>`), somado quando o agente lê dois, e
    /// também quando a leitura sai junto de outra ferramenta, na mesma
    /// resposta (duas linhas com o mesmo `message.id`). Com um resumo de 100
    /// mil, 250 mil de conversa contam 150 mil; com dois, de 60 e 50 mil, 260
    /// mil contam 150 mil. Ler o pedido ou o item de uma tarefa não tira
    /// nada: 230 mil contam inteiros.
    #[test]
    fn the_summary_read_comes_off_the_count_at_the_task_end() {
        let with_id = |tokens, action, id: &str| {
            let mut reply: Value = serde_json::from_str(&agent_reply(tokens, Some(action))).unwrap();
            reply["message"]["id"] = id.into();
            reply.to_string()
        };
        for (mut replies, now, counted) in [
            (vec![agent_reply(40_000, Some("summary")), agent_reply(140_000, Some("edit"))], 250_000, 150),
            (vec![agent_reply(40_000, Some("summary_item")), agent_reply(140_000, Some("edit"))], 250_000, 150),
            (
                vec![
                    agent_reply(40_000, Some("summary")),
                    agent_reply(100_000, Some("summary_item")),
                    agent_reply(150_000, Some("edit")),
                ],
                260_000,
                150,
            ),
            (vec![with_id(40_000, "summary", "r1"), with_id(40_000, "edit", "r1"), with_id(140_000, "edit", "r2")], 250_000, 150),
            (vec![agent_reply(30_000, Some("read")), agent_reply(230_000, Some("edit"))], 230_000, 230),
            (vec![agent_reply(30_000, Some("task_item")), agent_reply(230_000, Some("edit"))], 230_000, 230),
        ] {
            let dir = open_project_in("x", Locale::PtBr);
            let reading = agent_finishes(dir.path(), &mut replies, now, Locale::PtBr).expect("the task end reads");
            let told = format!("conversa em {} mil tokens; sem o resumo da onda anterior, {counted} mil.", now / 1000);
            assert!(reading.contains(&told), "{told}: {reading}");
        }
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
        let order = agent_finishes(root, &mut replies, 260_000, Locale::PtBr).expect("the task end reads");
        assert!(order.contains("160 mil") && order.contains("Grave a entrega"), "{order}");

        let read = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            tool_input: serde_json::json!({"file_path": root.join("a.rs").to_string_lossy()}),
            ..agent_after_clear(root, &[261_000])
        };
        let verdict = crate::dispatch::run_event(Some(Trigger::PreToolUse), &read).verdict;
        assert!(matches!(verdict, Verdict::Deny { .. }), "the lock survives the clear: {verdict:?}");
        let again = injected(&finishing(agent_after_clear(root, &[261_000]))).expect("the task end reads after the clear");
        assert!(again.contains("261 mil tokens; sem o resumo da onda anterior, 161 mil"), "{again}");
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

    /// Depois da ordem de entregar, o comando que passa sozinho é recusado
    /// quando outro vem na mesma linha: encadeado depois dele (`&&`, `||`,
    /// `;`, nova linha), recebendo a saída dele por `|` (fora o `tail` ou
    /// `head` sem arquivo depois de compilar) ou escondido numa palavra
    /// (`$(…)`, crase), inclusive na pasta de um `cd`. O mesmo texto dentro de
    /// aspas simples, como no `--json` de uma gravação, é só texto.
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
            "mustard-rt run write step --json {} && cargo test --workspace",
            "mustard-rt run write step --json {} || cargo test",
            "mustard-rt run write step --json {}; cargo test",
            "mustard-rt run write step --json {}\ncargo test",
            "mustard-rt run write step --json {} | sh",
            "mustard-rt run write step --json \"$(cargo test)\"",
            "mustard-rt run write step --json `cargo test`",
            "/x/mustard-rt run read request-1 --root /r --spec x && cargo test",
            "mustard-rt run read request-1 | head -5",
            "cd $(cargo test) && mustard-rt run write step --json {}",
            "cargo build && cargo test",
            "cargo build; cargo test",
            "$HOME/.cargo/bin/cargo build && cargo test",
            "PATH=/p cargo build $(cargo test)",
            "cargo build | xargs cargo test",
            "cargo build 2>&1 | tail -20 src/a.rs",
        ] {
            let reason = bash(command).unwrap_or_else(|| panic!("{command} is refused after the order"));
            assert!(reason.starts_with("[Mustard]"), "{command}: {reason}");
        }
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
