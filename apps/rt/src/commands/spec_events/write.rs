//! `mustard-rt run write <tipo> --spec <nome> --json '{…}'` — grava um evento
//! no arquivo de eventos da spec, a única porta de escrita dele.
//!
//! O binário põe a versão do formato, o número, o código do item, a hora com o
//! fuso e o campo de busca; o resto vem do `--json`, que não pode trazer o
//! código. Quem aponta um item (`replaces`, os alvos de `remove` e `purge`)
//! usa o número do evento ou o código que a página mostra. A saída diz o
//! número e o código gravados e, num `remove` ou num `purge`, os números
//! afetados:
//!
//! ```text
//! {"ok": true, "spec": "teste", "id": 41, "type": "remove", "code": "MSTD-RMV-0002", "removed": [12, 13]}
//! ```
//!
//! A linha da spec no índice das specs é refeita a cada gravação, ainda com a
//! trava do arquivo de eventos presa. A página e o `.md` não: refazer os dois
//! custa segundos na spec real, e gravar um evento tem de custar o tempo de
//! escrever uma linha. Eles saem no fim de cada passo do fluxo, no fim de cada
//! onda — o `entregou`, que passa por aqui — e no comando de página.
//!
//! Com o tipo `lesson`, a gravação vai para o banco de lições
//! (`.claude/spec/lessons.ndjson`), e não para a spec: a classe vem em
//! `class`, a lição diz onde vale (`applies_to`) e onde nasceu (`found_in`), e
//! o `--spec`, opcional só aqui, diz a spec em que ela nasceu quando a lição
//! não diz. A página e o índice não mudam:
//!
//! ```text
//! {"ok": true, "id": 8, "type": "lesson", "class": "defect"}
//! ```
//!
//! Uma onda gravada depois da aprovação que leva a spec a ter mais ondas do
//! que tinha quando foi aprovada avisa o crescimento em `warnings`, com as
//! duas contas; o aviso nunca recusa. Um pedido (`request`) devolve em `next`
//! o passo seguinte, pelo `effect`: gravar as ondas novas no fim ou as versões
//! novas das que mudam, na mesma spec e na mesma branch.
//!
//! Um pedido adiado (`deferred`) aponta uma pendência aberta da lista do
//! projeto, a do checkout principal num worktree: o número pode vir escrito
//! `P-12`, e é gravado `12`. O número que a lista não tem, ou que já fechou, é
//! recusado com o comando que cria a pendência; o `deferred` nunca cria uma.
//!
//! Num worktree, o evento vai para o arquivo do checkout principal. As
//! citações de arquivo de um ponto são conferidas a partir de onde o comando
//! roda, e os nomes de código citados, no mapa do projeto: o nome que o mapa
//! não confirma entra em `warnings`, e o ponto é gravado.
//!
//! Numa pasta de spec do formato antigo, cujo `spec.md` é o documento e não a
//! página refeita, o binário não grava nada: a gravação recusa ali, e a pasta
//! fica com os mesmos bytes. A conferência é a do
//! [`super::pages::old_format_spec`].
//!
//! Quem grava por dentro do binário, como a testemunha da aprovação, usa
//! [`record`], a mesma gravação deste comando.
//!
//! Este comando também não grava a execução de um critério (`criterion_run`),
//! o veredito (`verdict`), o envio do pedido (`send`), o que uma onda entregou
//! (`delivered`), o commit (`commit`) nem a resposta do assistente
//! (`response`), nem tira ou revê um deles: quem os grava é o binário — a
//! rodada, o fechamento e o despachante. O autor `binary` é só das gravações
//! de dentro do binário.
//!
//! O clique (a `message` com `witness`, de qualquer autor: a resposta a uma
//! pergunta com opções) também não passa por aqui, nem para ser tirado ou
//! revisto: quem o grava é a testemunha, e é isso que faz dele um fato que o
//! modelo não escreve. A fala digitada do usuário (a `message` de autor
//! `user`) também não: ela chega só pelo gancho da entrada, e o `run write`
//! recusa gravá-la, tirá-la ou revê-la.
//!
//! O expurgo (`purge`) vale para qualquer item, inclusive os que só o binário
//! grava e o clique: ele não tira o item da leitura, só troca por "…", no
//! próprio arquivo, o trecho que nunca podia ter sido gravado. O trecho é o
//! que o pedido indica em `excerpt` — que nunca vai para o arquivo — ou, sem
//! ele, o que a procura de segredo acha nos campos de texto do item, a
//! lacuna do ponto inclusive; o item em que o trecho não aparece é recusado.
//! Num ponto do levantamento, o trecho sai do par inteiro: do original e do
//! fechamento, que copiou a lacuna dele.
//!
//! Este comando não grava o tipo `state`: o estado da spec é dos comandos do
//! fluxo e da testemunha da aprovação. As portas que gravam `state` são
//! cinco, todas aqui — o [`record`] da testemunha, o [`record_birth`], o
//! [`record_open`] do `open`, o [`record_phase`] e o [`record_pr_open`] do
//! `pr-open` — e passam pela regra única
//! da mudança de fase
//! (`mustard_core::domain::spec_state::phase_write_allowed`), conferida com a
//! trava presa, no arquivo como ele ficaria. As outras gravações deste comando
//! também são conferidas: nenhuma delas muda o estado, nem removendo nem
//! revendo um `state`.
//!
//! Numa spec em levantamento, o objetivo, o primeiro `context` como o índice
//! o lê, é a frase do usuário palavra por palavra, ou a sugestão que ele
//! aprovou. Toda gravação que troca o objetivo (o primeiro `context`, a
//! revisão dele e a remoção que passa o lugar para outro `context`) deixa um
//! objetivo que aponta em `origin` uma mensagem do usuário e repete o texto
//! dela, ou repete palavra por palavra uma frase da última resposta do
//! assistente antes dela, a que o usuário respondeu
//! (`mustard_core::domain::spec_state::goal_rule`), na mesma conferência. A
//! resposta do turno em que a spec nasce conta: ela é gravada sem `reply_to`,
//! porque ainda não há mensagem do usuário a que responder, e só ela pode
//! faltar o campo (`mustard_core::domain::spec_state::reply_rule`).
//!
//! O tipo de trabalho (`work_type`) é gravado pelo `grill`, que monta a lista
//! de pontos junto: este comando não o grava, nem o tira ou o revê.
//!
//! Depois da aprovação, o item combinado novo nasce com dono
//! (`mustard_core::domain::wave_prompt::owner_rule`): as ondas das tarefas
//! que o cobrem, as que ele diz em `waves` — mesmo a que ainda vai entrar no
//! plano —, ou o projeto todo, em `applies_to`. O item sem dono é recusado,
//! e nada é gravado.
//!
//! Numa spec em levantamento com o tipo de trabalho ou algum ponto gravado,
//! a saída traz o passo seguinte (`mustard_core::domain::survey::next_step`):
//! em `next`, o que fazer; em `points`, enquanto alguma lacuna do tipo não
//! tem ponto, os pontos que faltam gravar; em `point`, o próximo ponto
//! aberto, o mesmo enquanto ele não fecha; em `review`, o bloco cujo último ponto a gravação fechou, com a
//! pergunta da revisão e as opções, a de um revisor de fora no último bloco;
//! em `unrouted`, no fim, as mensagens do usuário que nenhum registro aponta.
//! O ponto aberto só sai fechado, por um `point` que o aponta em `closes`: o
//! `remove` dele é recusado. A passagem do levantamento para o plano, e a
//! aprovação, pedem nenhum ponto aberto (`survey_rule`), na mesma conferência
//! de toda porta que grava o estado.

use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::LESSON;
use mustard_core::domain::spec_events::{type_spec, Hidden, Refusal, SpecLog, PHASES};
use mustard_core::domain::spec_index;
use mustard_core::domain::spec_state::{
    birth_event, goal_rule, phase_write_allowed, reply_rule, survey_rule, waves_grown_by, PhaseWriter, SpecState,
    State,
};
use mustard_core::domain::survey::{self, SurveyStep};
use mustard_core::domain::wave_prompt::owner_rule;
use mustard_core::io::{lessons, project_map, spec_events as store};
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::pages::SpecPages;
use crate::shared::spec_state::DiskSpecState;

/// Os tipos que só o binário grava: a execução de um critério, que o
/// fechamento grava ao rodar a prova; o veredito, o envio do pedido de uma
/// onda, o que ela entregou e o commit, que só a rodada grava; e a resposta do
/// assistente, que o despachante grava no fim de cada resposta.
/// O `run write` não os grava, nem tira ou revê um deles.
const BINARY_ONLY: &[&str] = &["criterion_run", "verdict", "send", "delivered", "commit", "response"];

/// Os números dos eventos `event_type` que a leitura de `log` mostra.
fn visible_of(log: &SpecLog, event_type: &str) -> Vec<u64> {
    log.visible().into_iter().filter(|event| event.event_type == event_type).map(|event| event.id).collect()
}

/// A mensagem é do usuário.
fn by_user(event: &Map<String, Value>) -> bool {
    event.get("author").and_then(Value::as_str).map(str::trim) == Some("user")
}

/// A mensagem traz a testemunha: é um clique numa pergunta com opções, que só
/// a testemunha grava. Vale para qualquer autor — a rodada e a aprovação leem
/// o clique pela testemunha, e um "Aceitar" com outro autor escrito à mão não
/// pode passar por esta porta para depois ser revisto com o autor do usuário.
fn carries_witness(event: &Map<String, Value>) -> bool {
    event.get("witness").is_some_and(|w| !w.is_null())
}

/// A mensagem só um gancho grava: o clique, de qualquer autor, que vem da
/// testemunha, e a fala do usuário, que vem do gancho da entrada.
fn hook_only_message(event: &Map<String, Value>) -> bool {
    carries_witness(event) || by_user(event)
}

/// Os números das mensagens que só um gancho grava, como a leitura de `log` as
/// mostra, contando também as que o formato antigo do expurgo esvaziou: o
/// expurgo tira o texto de um segredo, e não a fala do usuário da conversa.
fn hook_only_messages(log: &SpecLog) -> Vec<u64> {
    let hidden = log.hidden();
    log.events
        .iter()
        .filter(|event| event.event_type == "message" && hook_only_message(&event.fields))
        .filter(|event| hidden.get(&event.id).is_none_or(|why| matches!(why, Hidden::Purged { .. })))
        .map(|event| event.id)
        .collect()
}

/// Options for `mustard-rt run write`.
pub struct WriteOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que recebe o evento; na lição, a spec em que ela nasceu.
    pub spec: Option<String>,
    pub event_type: String,
    /// Os campos do evento, num objeto JSON.
    pub json: String,
}

/// O núcleo testável de [`run`]: o relatório da gravação ou a recusa. Nunca
/// entra em pânico.
pub(crate) fn write_at(opts: &WriteOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let mut draft = match serde_json::from_str::<Value>(&opts.json) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let shown: String = other.to_string().chars().take(80).collect();
            return refuse(Refusal::NotAnObject { detail: shown });
        }
        Err(e) => return refuse(Refusal::NotAnObject { detail: e.to_string() }),
    };
    if draft.get("author").and_then(Value::as_str).map(str::trim) == Some("binary") {
        return refuse(Refusal::BinaryAuthor);
    }
    let event_type = opts.event_type.trim();
    if event_type == LESSON {
        return write_lesson(&project, opts.spec.as_deref(), draft);
    }
    let Some(spec) = opts.spec.as_deref() else {
        // Sem spec, um tipo que não existe continua recusado pelo nome.
        return refuse(if type_spec(event_type).is_some() {
            Refusal::SpecRequired { event_type: event_type.to_string() }
        } else {
            Refusal::UnknownType { found: event_type.to_string() }
        });
    };
    if event_type == "state" {
        return refuse(Refusal::StateByFlowOnly { spec: spec.trim().to_string() });
    }
    if event_type == "work_type" {
        return refuse(Refusal::WorkTypeByGrill);
    }
    if BINARY_ONLY.contains(&event_type) {
        return refuse(Refusal::BinaryOnlyType { event_type: event_type.to_string(), spec: spec.trim().to_string() });
    }
    if event_type == "message" && hook_only_message(&draft) {
        return refuse(Refusal::UserMessageByHook { spec: spec.trim().to_string() });
    }
    if event_type == "deferred"
        && let Err(refusal) = point_to_open_pending(&opts.root, &mut draft)
    {
        return refuse(refusal);
    }
    if let Err(refusal) = spec_was_opened(&project.root, spec) {
        return refuse(refusal);
    }
    // O passo seguinte de um pedido, pelo efeito dele; a conferência do tipo
    // recusa um efeito que não existe antes de o relatório sair.
    let next = (event_type == "request")
        .then(|| draft.get("effect").and_then(Value::as_str).map(|effect| format!("request.{}", effect.trim())))
        .flatten();
    match record_in(&project, &opts.root, spec, event_type, draft, None) {
        Ok(Recorded { written, pages, grew, survey }) => {
            let mut report = json!({
                "ok": true,
                "spec": spec.trim(),
                "id": written.id,
                "type": event_type,
            });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            if !written.removed.is_empty() {
                report["removed"] = json!(written.removed);
            }
            if !written.purged.is_empty() {
                report["purged"] = json!(written.purged);
            }
            // Se não deu para gravar a página e o `.md`, ou para refazer a
            // linha da spec no índice, o evento já está no arquivo: fica o
            // aviso. O nome citado num fato que o mapa não confirma também
            // só avisa.
            let mut warnings = Vec::new();
            if let Some(Err(refusal)) = &pages {
                warnings.push(refusal.message(lang));
            }
            if let Some(refusal) = &written.index_warning {
                warnings.push(spec_index::write_warning(refusal, lang));
            }
            for (fact, finding) in &written.citation_warnings {
                warnings.extend(finding.warning(*fact, lang));
            }
            // O crescimento das ondas depois da aprovação só avisa.
            if let Some((approved, now)) = grew {
                warnings.push(
                    translate("spec_events.waves_grew", lang)
                        .replace("{approved}", &approved.to_string())
                        .replace("{now}", &now.to_string()),
                );
            }
            if !warnings.is_empty() {
                report["warnings"] = json!(warnings);
            }
            if let Some(key) = next {
                report["next"] = json!(translate(&key, lang));
            } else if let Some(survey) = survey {
                for (key, value) in survey {
                    report[key.as_str()] = value;
                }
            }
            report
        }
        Err(refusal) => refuse(refusal),
    }
}

/// Para os testes: grava como a produção grava. A mensagem do usuário chega
/// pelos ganchos, e o que uma onda entregou e o commit, pela rodada; os três
/// passam por [`record`], como lá. O resto passa pelo `run write`. O relatório
/// tem a forma do `run write`, e a spec que não foi aberta é recusada do
/// mesmo jeito.
#[cfg(test)]
pub(crate) fn seed_at(opts: &WriteOpts) -> Value {
    let event_type = opts.event_type.trim();
    let draft = serde_json::from_str::<Value>(&opts.json).ok().and_then(|v| v.as_object().cloned());
    let (Some(spec), Some(draft)) = (opts.spec.as_deref(), draft) else {
        return write_at(opts);
    };
    let project = super::project(&opts.root);
    let by_hooks = matches!(event_type, "delivered" | "commit") || (event_type == "message" && by_user(&draft));
    if !by_hooks || spec_was_opened(&project.root, spec).is_err() {
        return write_at(opts);
    }
    match record(&opts.root, spec, event_type, draft, PhaseWriter::Binary) {
        Ok(Recorded { written, .. }) => {
            let mut report = json!({ "ok": true, "spec": spec.trim(), "id": written.id, "type": event_type });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            report
        }
        Err(refusal) => super::refused(&refusal, project.lang),
    }
}

/// A spec `spec` foi aberta: o arquivo de eventos dela existe, ou a pasta é
/// do formato antigo, que tem a recusa própria na gravação.
///
/// Uma gravação do modelo nunca faz uma spec nascer. O arquivo de eventos
/// nasce no comando que abre a spec, junto com a branch de mesmo nome; sem
/// ele, a gravação criava a pasta e o arquivo, e depois o comando de abrir
/// recusava o nome, já tomado por uma spec que ninguém abriu.
///
/// # Errors
///
/// [`Refusal::SpecNotOpen`], que nomeia o comando que abre a spec.
fn spec_was_opened(root: &Path, spec: &str) -> Result<(), Refusal> {
    if super::pages::old_format_spec(root, spec) {
        return Ok(());
    }
    if store::spec_file(root, spec)?.is_file() {
        return Ok(());
    }
    Err(Refusal::SpecNotOpen { spec: spec.trim().to_string() })
}

/// O pedido adiado aponta uma pendência aberta da lista do projeto, vista de
/// `start`: o número escrito `P-12` vira `12`, e o que a lista não tem, ou que
/// já fechou, é recusado. Um valor que não é número fica como veio, para a
/// conferência do tipo recusar.
fn point_to_open_pending(start: &Path, draft: &mut Map<String, Value>) -> Result<(), Refusal> {
    let number = match draft.get("pending") {
        // Um número que não é inteiro positivo nunca é o de uma pendência: a
        // cobrança da entrega o descartaria calada.
        Some(Value::Number(number)) => match number.as_u64().filter(|n| *n > 0) {
            Some(n) => Some(n),
            None => return Err(Refusal::DeferredUnknownPending { pending: number.to_string() }),
        },
        Some(Value::String(text)) => crate::commands::event::pending::pending_id(text)
            .and_then(|id| id.trim_start_matches("P-").parse::<u64>().ok()),
        _ => None,
    };
    let Some(number) = number else {
        return Ok(());
    };
    draft.insert("pending".to_string(), json!(number));
    let pending = format!("P-{number}");
    match crate::commands::event::pending::pending_is_open(start, &pending) {
        Some(true) => Ok(()),
        Some(false) => Err(Refusal::DeferredClosedPending { pending }),
        None => Err(Refusal::DeferredUnknownPending { pending }),
    }
}

/// O que uma gravação deixou: o evento e, no fim de uma onda, onde a página e
/// o `.md` foram refeitos ou por que não foram gravados.
pub struct Recorded {
    pub(crate) written: store::Written,
    /// Onde a página e o `.md` foram gravados, ou a recusa da gravação deles.
    /// Só no `entregou` de uma onda: as outras gravações não os refazem.
    pub(crate) pages: Option<Result<SpecPages, Refusal>>,
    /// As ondas aprovadas e as de agora, quando a onda gravada fez a spec
    /// passar das ondas que tinha na aprovação que vale.
    pub(crate) grew: Option<(usize, usize)>,
    /// O passo do levantamento depois de uma gravação do modelo, como o
    /// relatório o mostra ([`survey_report`]).
    pub(crate) survey: Option<Map<String, Value>>,
}

/// Grava um evento da spec `spec`, vista de `start`, pela mesma gravação do
/// `run write`: a linha no arquivo de eventos e a linha da spec no índice. A
/// página e o `.md` só no `entregou` de uma onda. `by` diz quem grava, para a
/// regra da mudança de fase.
///
/// # Errors
///
/// A recusa da gravação, das conferências do evento às da mudança de fase.
pub fn record(
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
    by: PhaseWriter,
) -> Result<Recorded, Refusal> {
    record_in(&super::project(start), start, spec, event_type, draft, Some(by))
}

/// A única gravação no arquivo de eventos de uma spec: toda porta chega
/// aqui, e a regra da mudança de fase confere o arquivo antes e depois, com a
/// trava presa.
///
/// Numa pasta do formato antigo nada é gravado: o `spec.md` dela é o
/// documento, e um arquivo de eventos ali levaria a página a apagá-lo.
fn record_in(
    project: &super::Project,
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
    by: Option<PhaseWriter>,
) -> Result<Recorded, Refusal> {
    if super::pages::old_format_spec(&project.root, spec) {
        return Err(Refusal::OldFormatSpec { spec: spec.trim().to_string() });
    }
    let path = store::spec_file(&project.root, spec)?;
    let roots = store::citation_roots(start, &project.root);
    let (carried, replaces) = phase_carried(event_type, &draft);
    let name = spec.trim().to_string();
    // A página e o `.md` saem no fim de cada passo e no fim de cada onda, não
    // a cada gravação. O fim de uma onda é o `entregou`, e ele passa por
    // aqui: os dois são refeitos antes de a trava soltar, do que acabou de
    // ser gravado, e a gravação seguinte, de outra sessão, só entra depois. A
    // conta das ondas também sai dali, com a trava presa: duas ondas gravadas
    // ao mesmo tempo nunca avisam a mesma conta.
    let mut pages = None;
    let mut grew = None;
    let mut survey = None;
    let wave = event_type == "wave";
    let ends_a_wave = event_type == "delivered";
    let lang = project.lang;
    let written = store::write_guarded(
        &path,
        event_type,
        draft,
        &roots,
        &super::pages::secret::secret_excerpts,
        |before, after| {
            record_rules(&name, before, after, carried.as_deref(), replaces, by)?;
            // O passo do levantamento só vai ao relatório do modelo.
            if by.is_none() {
                survey = survey_report(&project.root, &name, before, after, lang);
            }
            Ok(())
        },
        |log| {
            if wave {
                grew = waves_grown_by(log, log.max_id());
            }
            if ends_a_wave {
                pages = Some(super::pages::rebuild(&project.root, spec, log, project.lang));
            }
        },
    )?;
    Ok(Recorded { written, pages, grew, survey })
}

/// A fase que uma gravação de `state` traz e o `state` que ela revê; nos
/// outros tipos, nenhum dos dois.
fn phase_carried(event_type: &str, draft: &Map<String, Value>) -> (Option<String>, Option<u64>) {
    if event_type != "state" {
        return (None, None);
    }
    let carried = draft.get("phase").and_then(Value::as_str).map(|phase| phase.trim().to_string());
    (carried, draft.get("replaces").and_then(Value::as_u64))
}

/// As regras que toda gravação na spec `spec` cumpre, sobre o arquivo antes e
/// depois dela: a mudança de fase, a mensagem que a resposta responde, o
/// objetivo, o levantamento e o dono do item combinado.
fn record_rules(
    spec: &str,
    before: &SpecLog,
    after: &SpecLog,
    carried: Option<&str>,
    replaces: Option<u64>,
    by: Option<PhaseWriter>,
) -> Result<(), Refusal> {
    phase_rule(spec, before, after, carried, replaces, by)?;
    reply_rule(before, after)?;
    goal_rule(spec, before, after)?;
    survey_rule(spec, before, after)?;
    owner_rule(before, after)
}

/// Gravações do binário na spec conferidas antes, sem gravar nada: cada uma
/// passa pela mesma conferência de [`record`] — a do evento, a do arquivo e as
/// regras da spec —, sobre o arquivo como as anteriores o deixariam. É assim
/// que a rodada sabe, antes do commit, que nenhuma gravação depois dele será
/// recusada.
pub(crate) struct RecordCheck {
    dry: store::DryRun<'static>,
    name: String,
    by: PhaseWriter,
}

impl RecordCheck {
    /// Começa a conferência na spec `spec`, vista de `start`, com as
    /// gravações de `by`.
    ///
    /// # Errors
    ///
    /// A recusa que [`record`] daria antes de ler o arquivo, ou a da leitura.
    pub(crate) fn open(start: &Path, spec: &str, by: PhaseWriter) -> Result<Self, Refusal> {
        let project = super::project(start);
        if super::pages::old_format_spec(&project.root, spec) {
            return Err(Refusal::OldFormatSpec { spec: spec.trim().to_string() });
        }
        let path = store::spec_file(&project.root, spec)?;
        let roots = store::citation_roots(start, &project.root);
        let dry = store::DryRun::open(&path, roots, &super::pages::secret::secret_excerpts)?;
        Ok(Self { dry, name: spec.trim().to_string(), by })
    }

    /// O arquivo como as gravações conferidas até aqui o deixariam.
    pub(crate) fn log(&self) -> &SpecLog {
        self.dry.log()
    }

    /// Confere a gravação que [`record`] faria com estes campos.
    ///
    /// # Errors
    ///
    /// A recusa que a gravação daria.
    pub(crate) fn record(&mut self, event_type: &str, draft: Map<String, Value>) -> Result<(), Refusal> {
        let (carried, replaces) = phase_carried(event_type, &draft);
        let (name, by) = (&self.name, self.by);
        self.dry.write(event_type, draft, |before, after| {
            record_rules(name, before, after, carried.as_deref(), replaces, Some(by))
        })
    }
}

/// O passo do levantamento depois de uma gravação na spec `spec`, lido do
/// arquivo antes e depois dela, como o relatório o mostra: em `next`, o que
/// fazer, no idioma do projeto; em `point`, o ponto a apresentar; em
/// `points`, os pontos que faltam gravar, como o `grill` os lista, ou os
/// pontos abertos do levantamento condensado, mostrados de uma vez; em
/// `review`, o bloco que fechou, com os pontos, os registros que os fecharam,
/// a pergunta e as opções, "Seguir" por último; em `unrouted`, as mensagens
/// do usuário sem destino. Com a revisão e outro passo juntos, `next` traz os
/// dois, na ordem. `None` quando não há passo.
fn survey_report(
    root: &Path,
    spec: &str,
    before: &SpecLog,
    after: &SpecLog,
    lang: Locale,
) -> Option<Map<String, Value>> {
    let steps = survey::next_step(before, after);
    if steps.is_empty() {
        return None;
    }
    let codes = after.codes();
    let code_of = |id: &u64| codes.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut out = Map::new();
    let mut next: Vec<String> = Vec::new();
    for step in steps {
        match step {
            SurveyStep::ReviewBlock { block, closed, records, outside_review } => {
                let go_on = translate("survey.continue_option", lang);
                next.push(translate("survey.review_step", lang).replace("{block}", &block).replace("{continue}", go_on));
                let mut options = Vec::new();
                if outside_review {
                    options.push(translate("survey.outside_review_question", lang));
                }
                options.push(go_on);
                out.insert(
                    "review".to_string(),
                    json!({
                        "block": block,
                        "points": closed.iter().map(code_of).collect::<Vec<_>>(),
                        "records": records.iter().map(code_of).collect::<Vec<_>>(),
                        "question": translate("survey.review_question", lang),
                        "options": options,
                    }),
                );
            }
            SurveyStep::Point(point) if point.str_field("block").map(str::trim) == Some(survey::CONDENSED) => {
                next.push(translate("survey.present_all", lang).to_string());
                let open: Vec<Value> = survey::open_points(after).into_iter().map(|p| super::shown(p, &codes)).collect();
                out.insert("points".to_string(), json!(open));
            }
            SurveyStep::Point(point) => {
                next.push(
                    translate("survey.present_point", lang)
                        .replace("{code}", &code_of(&point.id))
                        .replace("{id}", &point.id.to_string()),
                );
                out.insert("point".to_string(), super::shown(point, &codes));
            }
            SurveyStep::Done { unrouted } => {
                next.push(translate("survey.done", lang).to_string());
                let listed: Vec<Value> = unrouted.into_iter().map(|m| super::shown(m, &codes)).collect();
                out.insert("unrouted".to_string(), json!(listed));
            }
            SurveyStep::Record(_) => {
                next.push(translate("survey.record_points", lang).replace("{spec}", spec));
                out.insert("points".to_string(), json!(unrecorded_points(root, spec, after, lang)));
            }
        }
    }
    out.insert("next".to_string(), json!(next.join(" ")));
    Some(out)
}

/// Os itens da lista do `grill` para as lacunas do tipo de trabalho que ainda
/// não têm ponto, como o `grill` os mostra: os campos que o assistente copia,
/// com a mensagem do objetivo como origem e o que o mapa do projeto já
/// responde.
fn unrecorded_points(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Vec<Value> {
    let kinds = survey::work_type(log).map(survey::kinds_of).unwrap_or_default();
    let goal = survey::goal(log);
    let map = project_map::read(root).ok();
    let list = survey::build(&survey::Sources {
        kinds: &kinds,
        goal: goal.and_then(|g| g.str_field("text")).map(str::trim).unwrap_or_default(),
        current: spec,
        bank: None,
        lessons_file: "",
        index: &[],
        prior: &[],
        map: map.as_ref(),
        condensed: survey::condensed(log),
        lang,
    });
    let origin = goal.and_then(|g| g.int("origin"));
    survey::missing(log, &list).into_iter().map(|item| item.to_value(origin)).collect()
}

/// A regra da mudança de fase sobre o arquivo antes e depois da gravação.
/// Sem porta do binário (`by` vazio), é o modelo pelo `run write`: nenhuma
/// gravação dele muda o estado.
///
/// Uma revisão (`replaces`) que repete a fase do item revisto não muda a
/// fase: não traz fase nenhuma para a regra. É o caso da branch que falta,
/// completada no `state` da própria aprovação.
fn phase_rule(
    spec: &str,
    before: &SpecLog,
    after: &SpecLog,
    carried: Option<&str>,
    replaces: Option<u64>,
    by: Option<PhaseWriter>,
) -> Result<(), Refusal> {
    let (was, now) = (State::from_log(before), State::from_log(after));
    let revised = replaces.and_then(|id| before.get(id)).and_then(|event| event.str_field("phase")).map(str::trim);
    let carried = carried.filter(|phase| revised != Some(*phase));
    let Some(by) = by else {
        if let Some(event_type) = BINARY_ONLY.iter().find(|t| visible_of(before, t) != visible_of(after, t)) {
            return Err(Refusal::BinaryOnlyType { event_type: (*event_type).to_string(), spec: spec.to_string() });
        }
        // O tipo de trabalho é do `grill`: o modelo não o tira nem o revê.
        if visible_of(before, "work_type") != visible_of(after, "work_type") {
            return Err(Refusal::WorkTypeByGrill);
        }
        // O que só um gancho grava, o modelo não tira nem revê.
        if hook_only_messages(before) != hook_only_messages(after) {
            return Err(Refusal::UserMessageByHook { spec: spec.to_string() });
        }
        return if was == now { Ok(()) } else { Err(Refusal::StateByFlowOnly { spec: spec.to_string() }) };
    };
    if phase_write_allowed(&was, &now, carried, by) {
        return Ok(());
    }
    Err(Refusal::PhaseChangeRefused {
        spec: spec.to_string(),
        from: was.phase.unwrap_or("-").to_string(),
        to: carried.or(now.phase).unwrap_or("-").to_string(),
    })
}

/// A porta do fechamento e da entrega: grava no estado da spec `spec`, vista
/// de `start`, a fase `phase` (`closed` no fechamento, `delivered` no merge),
/// pela mesma gravação do `run write`.
///
/// Chamam esta porta o `close`, o `pr-merge` e o início da sessão, no merge
/// feito por outra pessoa. É por ela que gravam a fase e armam a cobrança, e é
/// a única que arma.
///
/// Só grava quando a spec tem arquivo de eventos (uma branch que o Mustard
/// não abriu fica como está) e quando a fase de agora vem antes de `phase` na
/// ordem das fases: repetir um fechamento não grava outro, e uma spec entregue
/// não volta a fechada. `true` quando gravou.
///
/// No mesmo passo, arma a cobrança das pendências nascidas na spec para o
/// número do `state` gravado, no checkout principal: o fim da resposta a lê
/// sem perguntar qual é a spec atual, então a arrumação que troca de branch,
/// a sessão desligada e o worktree não a perdem.
///
/// A sessão de quem fecha é dita por quem chama: uma entrada `run` a lê do
/// ambiente, um gancho a sabe pelo evento que recebeu. O contador guarda
/// essa sessão, e só ela é cobrada; sem sessão, qualquer sessão principal é.
pub(crate) fn record_phase(start: &Path, spec: &str, phase: &str, session: Option<&str>) -> bool {
    let Some(recorded) = advance_phase(start, spec, phase, Map::new()) else {
        return false;
    };
    // A cobrança dispara com o fechamento e com o merge; a entrada na
    // execução não cobra nada.
    if matches!(phase, "closed" | "delivered") {
        let _ = crate::commands::event::pending::arm_charge(start, spec.trim(), recorded.written.id, session);
    }
    true
}

/// A porta do pull request aberto: grava no estado da spec `spec`, vista de
/// `start`, a fase `pr_open` com o número e o endereço do pull request, pela
/// mesma gravação do `run write`. Quem chama é o `pr-open`, depois que o
/// provedor abriu o pull request ou reescreveu o corpo do que já existia.
///
/// Só grava a partir de uma spec fechada: o pull request é o passo depois do
/// fechamento, e uma spec em execução que pulasse para esta fase nunca mais
/// fecharia. Repetir a abertura não grava outra fase. Não arma a cobrança das
/// pendências: ela é do fechamento e da entrega. `true` quando gravou.
pub(crate) fn record_pr_open(start: &Path, spec: &str, number: u64, url: Option<&str>) -> bool {
    let closed = DiskSpecState::new(start).state(spec).is_some_and(|state| state.phase == Some("closed"));
    if !closed {
        return false;
    }
    let mut pr = Map::new();
    pr.insert("number".to_string(), json!(number));
    if let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) {
        pr.insert("url".to_string(), json!(url));
    }
    let mut fields = Map::new();
    fields.insert("pr".to_string(), Value::Object(pr));
    advance_phase(start, spec, "pr_open", fields).is_some()
}

/// A gravação comum das portas de fase: o `state` com a fase `phase` e os
/// campos `fields`, quando a spec tem arquivo de eventos e a fase de agora vem
/// antes de `phase` na ordem das fases. `None` quando nada foi gravado.
fn advance_phase(start: &Path, spec: &str, phase: &str, mut fields: Map<String, Value>) -> Option<Recorded> {
    let order = |name: &str| PHASES.iter().position(|known| *known == name);
    let target = order(phase)?;
    let state = DiskSpecState::new(start).state(spec)?;
    if state.phase.and_then(order).is_some_and(|now| now >= target) {
        return None;
    }
    fields.insert("phase".to_string(), json!(phase));
    fields.insert("author".to_string(), json!("binary"));
    record(start, spec, "state", fields, PhaseWriter::Binary).ok()
}

/// O nascimento de uma spec com arquivo de eventos e sem nenhum `state`, pela
/// testemunha da aprovação: um `state` na fase `plan`, com a branch da spec,
/// quando se sabe, pela mesma gravação do `run write`. O `meta.json` nunca é
/// lido.
///
/// A branch é `branch`, quando o chamador a sabe; senão, a do checkout,
/// quando ela é a desta spec. Uma spec que já tem fase nunca volta ao plano, e
/// nada desfaz uma aprovação. `Ok(true)` quando gravou.
///
/// Numa spec que já nasceu, completa a branch que falta, quando se sabe,
/// revendo o `state` do nascimento: a fase fica como está, e uma branch já
/// gravada nunca é trocada.
pub(crate) fn record_birth(start: &Path, spec: &str, branch: Option<&str>) -> Result<bool, Refusal> {
    let born = DiskSpecState::new(start).log(spec).filter(|log| State::from_log(log).phase.is_some());
    // A branch é a dita, ou a do checkout em `start` quando ela é a da spec.
    let branch = branch.map(str::to_string).or_else(|| branch_of_spec(start, spec));
    if let Some(log) = born {
        return complete_missing(start, spec, &log, branch);
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("plan"));
    draft.insert("author".to_string(), json!("binary"));
    if let Some(branch) = branch {
        draft.insert("branch".to_string(), json!(branch));
    }
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// O nascimento de uma spec aberta pelo `open`: um `state` em levantamento,
/// com a branch que ele acabou de criar e a base de que ela saiu, pela mesma
/// gravação do `run write`. Uma spec que já tem fase nunca volta ao
/// levantamento: aí nada é gravado, e a resposta é `Ok(false)`.
pub(crate) fn record_open(start: &Path, spec: &str, branch: &str, base: &str) -> Result<bool, Refusal> {
    if DiskSpecState::new(start).log(spec).is_some_and(|log| State::from_log(&log).phase.is_some()) {
        return Ok(false);
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("survey"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("branch".to_string(), json!(branch));
    draft.insert("base".to_string(), json!(base));
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// Completa a branch que falta no estado de uma spec que já nasceu, revendo o
/// `state` do nascimento com os campos dele e a branch: a revisão entra no
/// lugar do nascimento na dobra, e a fase de agora não muda. Nada é trocado:
/// só o que falta entra. `Ok(false)` quando a branch já está gravada ou não se
/// sabe.
fn complete_missing(start: &Path, spec: &str, log: &SpecLog, branch: Option<String>) -> Result<bool, Refusal> {
    /// Os campos que o binário carimba e que uma revisão não traz.
    const STAMPED: &[&str] = &["v", "id", "code", "at", "type", "search", "replaces", "author"];
    let state = State::from_log(log);
    let Some(branch) = branch.filter(|_| state.branch.is_none()) else {
        return Ok(false);
    };
    let Some(birth) = birth_event(log) else {
        return Ok(false);
    };
    let mut draft: Map<String, Value> = birth
        .fields
        .iter()
        .filter(|(key, _)| !STAMPED.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("replaces".to_string(), json!(birth.id));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("branch".to_string(), json!(branch));
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// A branch do checkout em `start`, quando ela é a da spec `spec`.
pub(crate) fn branch_of_spec(start: &Path, spec: &str) -> Option<String> {
    use crate::commands::event::work_branch::slug_of_work_branch;
    let config = mustard_core::ProjectConfig::load(start);
    let current = mustard_core::current_branch(start)?;
    (slug_of_work_branch(&current, &config).as_deref() == Some(spec.trim())).then_some(current)
}

/// Grava uma lição no banco de lições do projeto. `spec`, quando vem, diz em
/// que spec a lição nasceu.
fn write_lesson(project: &super::Project, spec: Option<&str>, draft: Map<String, Value>) -> Value {
    let refuse = |refusal: Refusal| super::refused(&refusal, project.lang);
    let path = match ClaudePaths::for_project(&project.root) {
        Ok(paths) => paths.lessons_path(),
        Err(e) => return refuse(Refusal::Io { detail: e.to_string() }),
    };
    match lessons::write(&path, draft, spec) {
        Ok(written) => json!({ "ok": true, "id": written.id, "type": LESSON, "class": written.class }),
        Err(refusal) => refuse(refusal),
    }
}

/// Run `write` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &WriteOpts) {
    let report = write_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::run_event;
    use mustard_core::domain::model::contract::{HookInput, Trigger};
    use mustard_core::platform::git;
    use tempfile::tempdir;

    /// Deixa a spec aberta, como o comando que abre uma spec a deixa: com o
    /// arquivo de eventos no lugar. Uma pasta do formato antigo, que tem o
    /// `spec.md` como documento, fica exatamente como está.
    fn open_spec(root: &std::path::Path, spec: &str) {
        // A spec mora no checkout principal, também quando a gravação sai de
        // um worktree: é por lá que a abertura passa.
        let Ok(path) = store::spec_file(&store::spec_root(root), spec) else { return };
        let Some(dir) = path.parent() else { return };
        if path.exists() || dir.join("spec.md").exists() {
            return;
        }
        std::fs::create_dir_all(dir).expect("spec folder");
        std::fs::File::create(&path).expect("the event file");
    }

    fn write_to(root: &std::path::Path, spec: Option<&str>, event_type: &str, json: &str) -> Value {
        if let Some(spec) = spec {
            open_spec(root, spec);
        }
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: json.into(),
        })
    }

    fn write(root: &std::path::Path, event_type: &str, json: &str) -> Value {
        write_to(root, Some("teste"), event_type, json)
    }

    #[test]
    fn a_write_reports_its_number_and_what_a_removal_took_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = write(root, "message", r#"{"text":"um"}"#);
        assert_eq!(
            first,
            json!({"ok": true, "spec": "teste", "id": 1, "type": "message", "code": "MSTD-MSG-0001"})
        );
        write(root, "message", r#"{"text":"dois"}"#);
        let removal = write(root, "remove", r#"{"targets":[1,2],"reason":"engano"}"#);
        assert_eq!(removal["removed"], json!([1, 2]), "{removal}");
        assert!(root.join(".claude").join("spec").join("teste").join("spec.ndjson").is_file());
    }

    /// Um fato que cita um nome que o mapa não conhece entra, e o relatório
    /// avisa, com o número do fato; o nome declarado no arquivo citado passa
    /// calado.
    #[test]
    fn a_cited_name_the_map_does_not_know_warns_and_the_point_is_written() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "fn um() {}\nfn dois_passos() {}\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            mustard_core::io::project_map::model_path(root),
            r#"{"modules":[{"path":"src/real.rs","declarations":[{"kind":"function","name":"dois_passos","line":2}]}]}"#,
        )
        .unwrap();
        let msg = write(root, "message", r#"{"author":"user","text":"oi"}"#)["id"].as_u64().unwrap();
        let point = json!({
            "block": "limits", "gap": "g", "from": "gap", "status": "open", "origin": msg,
            "facts": [
                {"text": "quem lê é `ler_tudo`", "source": "src/real.rs:2"},
                {"text": "e depois `dois_passos`", "source": "src/real.rs:1"}
            ]
        });
        let report = write(root, "point", &point.to_string());
        assert_eq!(report["ok"], json!(true), "{report}");
        let warnings = report["warnings"].as_array().unwrap_or_else(|| panic!("no warnings: {report}"));
        assert_eq!(warnings.len(), 1, "{report}");
        let warning = warnings[0].as_str().unwrap();
        assert!(warning.contains("`ler_tudo`") && warning.contains('1'), "{warning}");
        assert!(!warning.contains("dois_passos"), "{warning}");
        let log = store::read(&root.join(".claude/spec/teste/spec.ndjson")).unwrap().unwrap();
        assert!(log.visible().iter().any(|event| event.event_type == "point"), "the point is in the file");
    }

    /// O autor `binary` é só das gravações de dentro do binário. Numa spec
    /// cujo `spec.md` traz a seção de critérios de aceitação, o `spec.md` é o
    /// documento: dali em diante nenhuma gravação entra, e o texto dele fica
    /// com os mesmos bytes.
    #[test]
    fn run_write_refuses_the_binary_author_and_every_write_to_an_old_format_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let msg = write(root, "message", r#"{"author":"user","text":"oi"}"#)["id"].as_u64().unwrap();
        let decision = r#"{"author":"binary","text":"t","keys":["k"],"why":"w"}"#;
        assert_eq!(write(root, "decision", decision)["reason"], json!("binary-author"));

        let criterion = format!(r#"{{"when":"w","then":"t","proof":"cd .","origin":{msg}}}"#);
        let accepted = write(root, "criterion", &criterion);
        assert_eq!(accepted["ok"], json!(true), "a spec rendered from its events takes a criterion: {accepted}");
        let md = root.join(".claude").join("spec").join("teste").join("spec.md");
        let document = "# T\n\n## Acceptance Criteria\n\n- **AC-1** — a. Command: `cd .`\n";
        std::fs::write(&md, document).unwrap();
        let events = lines(root);

        let removal = format!(r#"{{"targets":[{}],"reason":"engano"}}"#, accepted["id"]);
        let revision = format!(r#"{{"when":"w","then":"t","proof":"cd ..","origin":{msg},"replaces":{}}}"#, accepted["id"]);
        for (event_type, payload) in [
            ("criterion", criterion.as_str()),
            ("remove", removal.as_str()),
            ("criterion", revision.as_str()),
            ("message", r#"{"author":"user","text":"e mais um"}"#),
        ] {
            let refused = write(root, event_type, payload);
            assert_eq!(refused["reason"], json!("old-format-spec"), "{event_type}: {refused}");
        }
        assert_eq!(lines(root), events, "nothing was written");
        assert_eq!(std::fs::read_to_string(&md).unwrap(), document, "the document is left alone");
    }

    /// A execução de um critério, o veredito e a resposta do assistente são
    /// gravados só pelo binário: o `run write` recusa os três, e recusa tirar
    /// uma execução gravada. Uma execução aprovada escrita à mão nunca abre o
    /// fechamento.
    #[test]
    fn criteria_runs_and_verdicts_are_written_by_the_binary_only() {
        use crate::shared::spec_state::{seed_run, seed_runs};
        let dir = tempdir().unwrap();
        let root = dir.path();
        let criteria = seed_runs(root, "teste", &[None]);
        let failing = seed_run(root, "teste", criteria[0], "fail");

        let run = format!(r#"{{"criterion":{},"result":"pass","exit":0,"ms":1}}"#, criteria[0]);
        let refused = write(root, "criterion_run", &run);
        assert_eq!(refused["reason"], json!("binary-only-type"), "{refused}");
        let verdict = format!(
            r#"{{"wave":1,"result":"approved","text":"ok","criteria":[{{"criterion":{},"tests_rule":true}}]}}"#,
            criteria[0]
        );
        let refused = write(root, "verdict", &verdict);
        assert_eq!(refused["reason"], json!("binary-only-type"), "{refused}");
        let removal = write(root, "remove", &format!(r#"{{"targets":[{failing}],"reason":"engano"}}"#));
        assert_eq!(removal["reason"], json!("binary-only-type"), "taking the red run out is refused too: {removal}");

        // A resposta do assistente também é do binário: escrita à mão, ela é
        // recusada. A mensagem do usuário, que chega pelo gancho, fica.
        let asked = message(root, "user", "e agora?");
        let reply = format!(r#"{{"author":"assistant","text":"Pronto.","reply_to":{asked}}}"#);
        let before = lines(root);
        let refused = write(root, "response", &reply);
        assert_eq!(refused["reason"], json!("binary-only-type"), "{refused}");
        assert_eq!(lines(root), before, "nothing was written");

        let fechamento = crate::commands::flow::close::close_at(&crate::commands::flow::close::CloseOpts {
            spec: Some("teste".to_string()),
            report: None,
            root: root.to_path_buf(),
        });
        assert_eq!(fechamento["ok"], json!(false), "the close still refuses: {fechamento}");
    }

    /// Nenhuma gravação refaz a página nem o `.md`: os dois saem no fim do
    /// passo, pela porta que os refaz. Depois dela, uma decisão revista mostra
    /// só a versão nova fora da conversa, onde a antiga aparece marcada como
    /// substituída; um item removido some dos dois e continua no arquivo de
    /// eventos, com o motivo.
    #[test]
    fn the_page_and_the_md_come_out_at_the_end_of_the_step_and_not_at_each_write() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"decida"}"#);
        write(root, "decision", r#"{"text":"Texto antigo.","keys":["k"],"why":"w","origin":1}"#);
        let revised =
            write(root, "decision", r#"{"text":"Texto novo.","keys":["k"],"why":"w","origin":1,"replaces":2}"#);
        assert_eq!(revised["code"], json!("MSTD-DEC-0001"), "the new version keeps the code");
        write(root, "note", r#"{"text":"Anotação que sai.","keys":["n"],"origin":1}"#);
        let removal = write(root, "remove", r#"{"targets":[4],"reason":"engano"}"#);
        assert!(removal.get("warnings").is_none(), "{removal}");

        let spec = root.join(".claude").join("spec").join("teste");
        assert!(!spec.join("spec.html").exists(), "nenhuma gravação refez a página");
        assert!(!spec.join("spec.md").exists(), "nenhuma gravação refez o `.md`");
        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("os dois saem no fim do passo");
        let md = std::fs::read_to_string(spec.join("spec.md")).unwrap();
        let html = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        let (html_before, html_talk) = html.split_once("<section id=\"conversation\" class=\"block\"").unwrap();
        let (md_before, md_talk) = md.rsplit_once("\n## ").unwrap();
        for (before, talk) in [(html_before, html_talk), (md_before, md_talk)] {
            assert!(before.contains("Texto novo.") && !before.contains("Texto antigo."), "{before}");
            assert!(talk.contains("Texto antigo."), "{talk}");
            assert!(!before.contains("Anotação que sai.") && !talk.contains("Anotação que sai."));
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Anotação que sai.") && events.contains("engano"), "{events}");
    }

    /// Remover pelo código que a página mostra tira o item da leitura, da
    /// página e do `.md`, e ele continua no arquivo com o motivo. Um código
    /// que não existe é recusado citando o código, e nada é gravado.
    #[test]
    fn removing_by_the_code_takes_the_item_out_of_the_reading_the_page_and_the_md() {
        use crate::commands::spec_events::read::{read_at, ReadOpts};
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine as regras"}"#);
        for text in ["Regra um.", "Regra dois.", "Regra três."] {
            let json = json!({"text": text, "keys": ["k"], "example": "e", "origin": 1}).to_string();
            assert_eq!(write(root, "rule", &json)["ok"], json!(true));
        }
        let removal = write(root, "remove", r#"{"targets":["MSTD-RULE-0002"],"reason":"Regra repetida."}"#);
        assert_eq!(removal["removed"], json!([3]), "{removal}");
        assert!(removal.get("warnings").is_none(), "{removal}");

        let agreed = read_at(&ReadOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            block: "agreed".into(),
            term: None,
        })
        .unwrap();
        assert!(!agreed.contains("Regra dois.") && agreed.contains("Regra três."), "{agreed}");
        let spec = root.join(".claude").join("spec").join("teste");
        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("a página do fim do passo");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(!shown.contains("Regra dois."), "{page}: {shown}");
            assert!(shown.contains("Regra um.") && shown.contains("Regra três."), "{page}");
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Regra dois.") && events.contains("Regra repetida."), "{events}");

        let unknown = write(root, "remove", r#"{"targets":["MSTD-RULE-0009"],"reason":"r"}"#);
        assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
        assert!(unknown["hint"].as_str().unwrap().contains("MSTD-RULE-0009"), "{unknown}");
        let revised = write(root, "rule", r#"{"text":"Regra três, revista.","keys":["k"],"example":"e","origin":1,"replaces":"MSTD-RULE-0003"}"#);
        assert_eq!(revised["code"], json!("MSTD-RULE-0003"), "{revised}");
        let with_code = write(root, "note", r#"{"text":"t","keys":["k"],"origin":1,"code":"MSTD-NOTE-0001"}"#);
        assert_eq!(with_code["reason"], json!("binary-only-field"), "{with_code}");
        let after = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert_eq!(after.lines().count(), events.lines().count() + 1, "only the revision was written");
    }

    /// Numa pasta de spec do formato antigo, que tem o `meta.json` e nenhum
    /// arquivo de eventos, o binário não cria o arquivo: a gravação recusa, o
    /// `spec.md` dela fica com os mesmos bytes e a página não nasce.
    #[test]
    fn the_binary_never_creates_an_event_file_in_an_old_format_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("teste");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        let document = "# Rascunho\n\n## Contexto\n\nO texto da spec.\n";
        std::fs::write(spec.join("spec.md"), document).unwrap();

        let out = write(root, "message", r#"{"author":"user","text":"um recado"}"#);
        assert_eq!(out["reason"], json!("old-format-spec"), "{out}");
        assert!(out["hint"].as_str().unwrap().contains("teste"), "{out}");
        // A testemunha da aprovação chega pela mesma gravação, e recusa igual.
        assert_eq!(record_birth(root, "teste", None).unwrap_err().reason(), "old-format-spec");
        // E o `page --spec` também.
        assert_eq!(
            super::super::pages::refresh(root, "teste", Locale::PtBr).unwrap_err().reason(),
            "old-format-spec"
        );

        assert_eq!(std::fs::read_to_string(spec.join("spec.md")).unwrap(), document, "the document is left alone");
        assert!(!spec.join("spec.ndjson").exists(), "no event file in an old spec");
        assert!(!spec.join("spec.html").exists(), "no page over an old spec");
    }

    /// Uma spec aberta pelo `open` tem a página e o `.md` refeitos no fim do
    /// passo, e não a cada gravação, mesmo com um `meta.json` posto ao lado
    /// por uma porta antiga. A linha da spec no índice continua saindo a cada
    /// gravação: refazê-la custa uma linha.
    #[test]
    fn a_spec_opened_by_open_gets_its_page_at_the_end_of_the_step() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("teste");
        write(root, "message", r#"{"author":"user","text":"um recado"}"#);
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("a página do primeiro passo");

        let before = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        let out = write(root, "message", r#"{"author":"user","text":"e outro recado"}"#);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(
            std::fs::read_to_string(spec.join("spec.html")).unwrap(),
            before,
            "a gravação não mexeu na página"
        );
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        assert!(index.contains("\"teste\""), "{index}");

        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("a página do passo seguinte");
        let after = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        assert_ne!(before, after, "a página sai no fim do passo");
        assert!(after.contains("e outro recado"), "{after}");
        assert!(std::fs::read_to_string(spec.join("spec.md")).unwrap().contains("e outro recado"));
    }

    /// O fim de uma onda é o `entregou` dela, e ele refaz a página e o `.md`
    /// dentro da própria gravação, ainda com a trava do arquivo presa.
    #[test]
    fn the_delivered_of_a_wave_rebuilds_the_page_inside_the_write() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("teste");
        write(root, "message", r#"{"author":"user","text":"o plano"}"#);
        assert!(!spec.join("spec.html").exists(), "a mensagem não refez a página");

        let delivered = r#"{"wave":1,"text":"A onda 1 ficou pronta.","files":["src/a.rs"]}"#;
        let out = write(root, "delivered", delivered);
        assert_eq!(out["ok"], json!(true), "{out}");
        let page = std::fs::read_to_string(spec.join("spec.html")).expect("a página sai no fim da onda");
        assert!(page.contains("A onda 1 ficou pronta."), "{page}");
        assert!(std::fs::read_to_string(spec.join("spec.md")).unwrap().contains("A onda 1 ficou pronta."));
    }

    /// O que cada onda entregou, o commit, o clique e a fala digitada do
    /// usuário não são gravados à mão: o `run write` recusa os quatro, e recusa
    /// tirar ou rever uma entrega, um clique ou uma fala do usuário, sem gravar
    /// nada. A mensagem do assistente segue aceita, e o expurgo da fala do
    /// usuário também: o segredo colado na conversa precisa poder sair.
    #[test]
    fn deliveries_commits_and_user_clicks_are_never_written_by_hand() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = message(root, "user", "a senha é abc123");
        let clicked = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            event_type: "message".into(),
            json: json!({"author": "user", "text": "Seguir?\nSim", "witness": {"question": "Seguir?", "answer": "Sim"}})
                .to_string(),
        });
        let clicked = clicked["id"].as_u64().unwrap();
        let delivered = write(root, "delivered", r#"{"wave":1,"text":"A onda 1 saiu.","files":["src/a.rs"]}"#);
        let delivered = delivered["id"].as_u64().unwrap();
        let by_hand = |event_type: &str, body: Value| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("teste".into()),
                event_type: event_type.into(),
                json: body.to_string(),
            })
        };
        let witness = json!({"question": "Seguir?", "answer": "Sim"});
        let before = lines(root);
        for (event_type, body, reason) in [
            ("delivered", json!({"wave": 1, "text": "Pronta.", "files": ["src/a.rs"]}), "binary-only-type"),
            ("commit", json!({"sha": "abc", "title": "t", "waves": [1], "files": ["src/a.rs"], "repo": "r"}), "binary-only-type"),
            ("remove", json!({"targets": [delivered], "reason": "engano"}), "binary-only-type"),
            ("message", json!({"author": "user", "text": "Seguir?\nSim", "witness": witness}), "user-message-by-hook"),
            ("message", json!({"author": "user", "text": "outro", "replaces": clicked}), "user-message-by-hook"),
            ("remove", json!({"targets": [clicked], "reason": "engano"}), "user-message-by-hook"),
            ("message", json!({"author": "user", "text": "sim, pode seguir"}), "user-message-by-hook"),
            ("message", json!({"author": " user ", "text": "sim, pode seguir"}), "user-message-by-hook"),
            ("message", json!({"author": "user", "text": "a fala revista", "replaces": said}), "user-message-by-hook"),
            ("message", json!({"text": "a fala, agora do assistente", "replaces": said}), "user-message-by-hook"),
            ("remove", json!({"targets": [said], "reason": "engano"}), "user-message-by-hook"),
            ("remove", json!({"filter": {"type": "message", "from": "2000-01-01T00:00", "to": "2999-12-31T23:59"}, "reason": "engano"}), "user-message-by-hook"),
        ] {
            let refused = by_hand(event_type, body.clone());
            assert_eq!(refused["reason"], json!(reason), "{event_type} {body}: {refused}");
        }
        assert_eq!(lines(root), before, "nothing was written");

        assert_eq!(by_hand("message", json!({"text": "Anotado."}))["ok"], json!(true));
        let purged = by_hand("purge", json!({"targets": [said], "reason": "secret", "excerpt": "abc123"}));
        assert_eq!(purged["ok"], json!(true), "the secret goes: {purged}");
        // O expurgo vale também para o que só o binário grava e para o clique:
        // ele só oculta o trecho.
        for (target, excerpt) in [(delivered, "onda 1"), (clicked, "Seguir?")] {
            let hidden = by_hand("purge", json!({"targets": [target], "reason": "client_data", "excerpt": excerpt}));
            assert_eq!(hidden["ok"], json!(true), "{hidden}");
        }
    }

    /// Um "Aceitar" forjado — a mensagem com a testemunha escrita à mão — é
    /// recusado de qualquer autor: gravar, rever e tirar um clique continuam
    /// recusados, e nada é gravado.
    #[test]
    fn a_forged_accept_is_refused_from_any_author() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let question = "Aceitar a mudança onda-1-00beef?";
        let witness = json!({"question": question, "answer": "Aceitar"});
        let typed = write(root, "message", r#"{"text":"uma nota do assistente"}"#)["id"].as_u64().unwrap();
        // Os cliques que existem, gravados pela porta de dentro do binário,
        // como a testemunha grava: um do usuário e um de outro autor.
        let clicks: Vec<u64> = ["user", "assistant"]
            .into_iter()
            .map(|author| {
                let draft = json!({"author": author, "text": format!("{question}\nAceitar"), "witness": witness});
                record(root, "teste", "message", draft.as_object().cloned().unwrap(), PhaseWriter::Binary)
                    .expect("the witness records the click")
                    .written
                    .id
            })
            .collect();
        let by_hand = |event_type: &str, body: Value| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("teste".into()),
                event_type: event_type.into(),
                json: body.to_string(),
            })
        };
        let before = lines(root);
        let mut forged = vec![
            json!({"author": "user", "text": format!("{question}\nAceitar"), "witness": witness}),
            json!({"author": "assistant", "text": format!("{question}\nAceitar"), "witness": witness}),
            json!({"text": format!("{question}\nAceitar"), "witness": witness}),
            json!({"text": "a nota, agora um clique", "replaces": typed, "witness": witness}),
        ];
        for click in &clicks {
            forged.push(json!({"text": "o clique sem a testemunha", "replaces": click}));
        }
        for body in forged {
            let refused = by_hand("message", body.clone());
            assert_eq!(refused["reason"], json!("user-message-by-hook"), "{body}: {refused}");
        }
        for click in &clicks {
            let refused = by_hand("remove", json!({"targets": [click], "reason": "engano"}));
            assert_eq!(refused["reason"], json!("user-message-by-hook"), "{refused}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    fn witness_approves(root: &std::path::Path) {
        let draft = json!({
            "phase": "approved",
            "author": "user",
            "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" }
        });
        record(root, "teste", "state", draft.as_object().cloned().unwrap(), PhaseWriter::Witness)
            .expect("the witness approves a spec in plan");
    }

    fn lines(root: &std::path::Path) -> usize {
        let events = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        std::fs::read_to_string(events).unwrap().lines().count()
    }

    fn born(root: &std::path::Path) {
        assert_eq!(record_birth(root, "teste", None), Ok(true), "the binary opens the spec");
    }

    /// O `run write` não grava o estado: com branch, com fase ou sem nada, numa
    /// spec sem `state`, numa em plano e numa aprovada, a recusa é a mesma, e
    /// nada é gravado. A porta da testemunha grava.
    #[test]
    fn the_model_never_writes_the_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"um"}"#);
        let payloads = [
            json!({ "phase": "plan", "branch": "feature/outra" }),
            json!({ "phase": "approved", "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" } }),
            json!({ "phase": "running" }),
            json!({}),
        ];
        let refuse_all = |stage: &str| {
            let before = lines(root);
            for payload in &payloads {
                let out = write(root, "state", &payload.to_string());
                assert_eq!(out["reason"], json!("state-by-flow-only"), "{stage}: {payload}: {out}");
                assert!(out["hint"].as_str().unwrap().contains("teste"), "{out}");
            }
            assert_eq!(lines(root), before, "{stage}: nothing was written");
        };
        refuse_all("no state");
        born(root);
        refuse_all("in plan");
        witness_approves(root);
        refuse_all("approved");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// Nenhuma outra gravação do modelo muda o estado: tirar o `state` da
    /// aprovação é recusado; tirar um item que não é estado passa.
    #[test]
    fn a_removal_by_the_model_never_changes_the_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        witness_approves(root);
        let approval = lines(root) as u64;
        let removed = write(root, "remove", &json!({ "targets": [approval], "reason": "engano" }).to_string());
        assert_eq!(removed["reason"], json!("state-by-flow-only"), "{removed}");
        let note = write(root, "message", r#"{"text":"sai"}"#);
        let id = note["id"].as_u64().unwrap();
        let ok = write(root, "remove", &json!({ "targets": [id], "reason": "engano" }).to_string());
        assert_eq!(ok["ok"], json!(true), "{ok}");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// Depois da aprovação, o item combinado que o modelo grava nasce com
    /// dono: sem onda nem o projeto todo, a gravação é recusada com o jeito de
    /// dar dono, e nada é gravado; a onda que ainda vai entrar no plano vale.
    /// Antes da aprovação, o dono vem do plano, e a gravação passa.
    #[test]
    fn after_the_approval_a_new_agreed_item_is_written_only_with_an_owner() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        let said = write(root, "message", r#"{"author":"user","text":"decidi"}"#)["id"].as_u64().unwrap();
        let crit = write(root, "criterion", &json!({"when": "a", "then": "b", "proof": "p", "origin": said}).to_string());
        let wave = json!({"n": 1, "text": "Onda.", "criteria": [crit["id"]], "done_when": "passa", "origin": said});
        assert_eq!(write(root, "wave", &wave.to_string())["ok"], json!(true));
        let decision = |extra: Value| {
            let mut body = json!({"text": "Decidido.", "keys": ["d"], "why": "o usuário disse", "origin": said});
            body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
            body.to_string()
        };
        assert_eq!(write(root, "decision", &decision(json!({})))["ok"], json!(true), "em plano o dono vem do plano");

        witness_approves(root);
        for refused_owner in [json!({}), json!({"applies_to": {"files": ["src/**"]}})] {
            let before = lines(root);
            let refused = write(root, "decision", &decision(refused_owner.clone()));
            assert_eq!(refused["reason"], json!("owner-missing"), "{refused_owner}: {refused}");
            assert!(refused["hint"].as_str().unwrap().contains("`\"waves\":[3]`"), "{refused}");
            assert_eq!(lines(root), before, "nada foi gravado");
        }
        for owner in [json!({"waves": [1]}), json!({"waves": [2]}), json!({"applies_to": {"files": ["**"]}})] {
            let out = write(root, "decision", &decision(owner.clone()));
            assert_eq!(out["ok"], json!(true), "{owner}: {out}");
        }
    }

    /// A ponte do fechamento não fecha uma spec em plano: o fechamento só vem
    /// depois da aprovação.
    #[test]
    fn the_bridge_never_closes_a_spec_in_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        assert!(!record_phase(root, "teste", "closed", None), "no closing before the approval");
        assert_eq!(lines(root), 1);
        witness_approves(root);
        assert!(record_phase(root, "teste", "closed", None), "the approved spec closes");
    }

    /// A branch que falta é completada quando a do checkout é a da spec, sem
    /// mexer na fase; uma branch já gravada nunca é trocada.
    #[test]
    fn a_missing_branch_is_completed_and_a_recorded_one_never_changes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        let git = |args: &[&str]| {
            assert!(git::run(root, args).ok, "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["checkout", "-q", "-b", "dev"]);
        std::fs::write(root.join("README.md"), "oi\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        let state = || DiskSpecState::new(root).state("teste").unwrap();

        // Rascunhada na base: nasce sem branch, e é aprovada.
        born(root);
        assert_eq!(state().branch, None);
        witness_approves(root);

        // Cortada depois: a branch que faltava é completada, e a aprovação fica.
        git(&["checkout", "-q", "-b", "feature/teste"]);
        assert_eq!(record_birth(root, "teste", None), Ok(true));
        assert_eq!(state().branch.as_deref(), Some("feature/teste"));
        assert!(state().approved, "the approval stays");

        // Uma branch gravada nunca é trocada.
        git(&["checkout", "-q", "-b", "feature/outra"]);
        assert_eq!(record_birth(root, "teste", Some("feature/zzz")), Ok(false));
        assert_eq!(state().branch.as_deref(), Some("feature/teste"));
    }

    /// Um repositório em `root`, com o fluxo `dev` → `main`, no checkout
    /// `branch`.
    fn repo_on(root: &std::path::Path, branch: &str) {
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        let git = |args: &[&str]| {
            assert!(git::run(root, args).ok, "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["checkout", "-q", "-b", "dev"]);
        std::fs::write(root.join("README.md"), "oi\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        if branch != "dev" {
            git(&["checkout", "-q", "-b", branch]);
        }
    }

    /// Um `state` gravado direto no arquivo da spec, sem regra nenhuma.
    fn seed_state(root: &std::path::Path, spec: &str, fields: Value) {
        let path = store::spec_file(root, spec).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store::write(&path, "state", fields.as_object().cloned().unwrap(), &[]).unwrap();
    }

    /// Numa spec cujo primeiro `state` é a própria aprovação, a branch que
    /// falta é completada: a revisão repete a fase aprovada, e isso não é
    /// mudança de fase.
    #[test]
    fn a_missing_branch_is_completed_on_the_approval_itself() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "feature/teste");
        seed_state(
            root,
            "teste",
            json!({ "phase": "approved", "author": "user", "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" } }),
        );
        assert_eq!(record_birth(root, "teste", None), Ok(true));
        let state = DiskSpecState::new(root).state("teste").unwrap();
        assert_eq!(state.branch.as_deref(), Some("feature/teste"));
        assert!(state.approved, "the approval stays");
    }

    #[test]
    fn what_is_not_a_json_object_is_refused() {
        let dir = tempdir().unwrap();
        open_spec(dir.path(), "teste");
        let file = store::spec_file(dir.path(), "teste").unwrap();
        for json in ["[1,2]", "{quebrado", "\"texto\""] {
            let out = write(dir.path(), "note", json);
            assert_eq!(out["reason"], json!("not-an-object"), "{json}: {out}");
        }
        assert_eq!(std::fs::read(&file).unwrap(), b"", "a refusal writes nothing");
    }

    /// O campo que o tipo não declara é recusado pelo nome, e a recusa diz
    /// quais campos o tipo aceita, nos dois idiomas. Nada é gravado: um nome
    /// escrito errado entraria calado e nunca mais seria lido por nada.
    #[test]
    fn a_field_the_type_does_not_declare_is_refused_and_the_accepted_ones_are_named() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let file = store::spec_file(root, "teste").unwrap();

        let strange = write(root, "rule", r#"{"text":"t","keys":["k"],"example":"e","origin":1,"ordem":[1]}"#);
        assert_eq!(strange["reason"], json!("unknown-field"), "{strange}");
        let hint = strange["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("ordem"), "{hint}");
        assert!(hint.contains("example"), "a recusa diz o que o tipo aceita: {hint}");
        assert_eq!(std::fs::read(&file).unwrap_or_default(), b"", "a recusa não grava nada");

        // O mesmo campo, agora declarado pelo tipo da onda, passa.
        let said = write(root, "message", r#"{"author":"user","text":"o pedido"}"#)["id"].as_u64().unwrap();
        let crit = write(root, "criterion",
            &json!({"when": "a", "then": "b", "proof": "p", "origin": said}).to_string())["id"]
            .as_u64()
            .unwrap();
        let wave = write(root, "wave",
            &json!({"n": 1, "text": "Onda.", "criteria": [crit], "done_when": "passa", "order": [crit], "origin": said}).to_string());
        assert_eq!(wave["ok"], json!(true), "{wave}");
    }

    /// Um tipo que não existe e um campo que falta são recusados pelo nome; um
    /// tipo da spec sem `--spec` pede a spec, e nada é gravado.
    #[test]
    fn an_unknown_type_and_a_missing_field_are_refused_by_name() {
        let dir = tempdir().unwrap();
        let unknown = write(dir.path(), "licao", r#"{"text":"x"}"#);
        assert_eq!(unknown["reason"], json!("unknown-type"));
        assert!(unknown["hint"].as_str().unwrap().contains("licao"));
        let missing = write(dir.path(), "rule", r#"{"text":"t","keys":["k"],"origin":1}"#);
        assert_eq!(missing["reason"], json!("missing-field"));
        assert!(missing["hint"].as_str().unwrap().contains("example"));
        let no_spec = write_to(dir.path(), None, "rule", r#"{"text":"t","keys":["k"],"example":"e","origin":1}"#);
        assert_eq!(no_spec["reason"], json!("spec-required"), "{no_spec}");
        assert!(no_spec["hint"].as_str().unwrap().contains("--spec"), "{no_spec}");
        let unknown_no_spec = write_to(dir.path(), None, "licao", "{}");
        assert_eq!(unknown_no_spec["reason"], json!("unknown-type"), "{unknown_no_spec}");
        assert_eq!(
            std::fs::read(store::spec_file(dir.path(), "teste").unwrap()).unwrap(),
            b"",
            "a refusal writes nothing",
        );
    }

    /// Uma gravação numa spec que ninguém abriu é recusada, e a recusa manda
    /// abrir a spec: nem a pasta nem o arquivo de eventos nascem por ali, e
    /// o nome não fica tomado por uma spec que a abertura nunca viu.
    #[test]
    fn a_write_into_a_spec_that_was_never_opened_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let out = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("nunca-aberta".into()),
            event_type: "message".into(),
            json: r#"{"text":"oi"}"#.into(),
        });
        assert_eq!(out["reason"], json!("spec-not-open"), "{out}");
        assert!(out["hint"].as_str().unwrap().contains("run open"), "{out}");
        assert!(!root.join(".claude").join("spec").join("nunca-aberta").exists(), "nothing was created");
    }

    /// A origem de um evento é um evento que já está no arquivo: o número que
    /// a spec não tem e o número do próprio evento são recusados, e nada é
    /// gravado.
    #[test]
    fn an_origin_that_is_not_in_the_file_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = write(root, "message", r#"{"author":"user","text":"o pedido"}"#)["id"].as_u64().unwrap();
        let file = store::spec_file(root, "teste").unwrap();
        let before = std::fs::read(&file).unwrap();
        for origin in [99, said + 1] {
            let rule = json!({"text": "t", "keys": ["k"], "example": "e", "origin": origin});
            let out = write(root, "rule", &rule.to_string());
            assert_eq!(out["reason"], json!("unknown-target"), "origin {origin}: {out}");
        }
        assert_eq!(std::fs::read(&file).unwrap(), before, "the refusals wrote nothing");
        let rule = json!({"text": "t", "keys": ["k"], "example": "e", "origin": said});
        assert_eq!(write(root, "rule", &rule.to_string())["ok"], json!(true), "a real origin passes");
    }

    /// O mesmo ponto gravado duas vezes é recusado na segunda, nomeando o que
    /// já está aberto: dois pontos abertos com a mesma lacuna fariam a mesma
    /// pergunta duas vezes. Outra lacuna entra, e a versão revista do ponto
    /// aberto também.
    #[test]
    fn the_same_point_twice_is_refused_naming_the_one_already_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = write(root, "message", r#"{"author":"user","text":"o pedido"}"#)["id"].as_u64().unwrap();
        let facts = json!([{"text": "não há teto", "source": format!("mensagem {said}")}]);
        let point = |gap: &str| {
            json!({"block": "limits", "gap": gap, "from": "gap", "status": "open", "origin": said,
                "facts": facts})
            .to_string()
        };
        let first = write(root, "point", &point("tamanho"));
        assert_eq!(first["ok"], json!(true), "{first}");

        let again = write(root, "point", &point("tamanho"));
        assert_eq!(again["reason"], json!("point-already-open"), "{again}");
        let hint = again["hint"].as_str().unwrap();
        assert!(hint.contains("MSTD-POINT-0001") && hint.contains("limits"), "{hint}");

        assert_eq!(write(root, "point", &point("prazo"))["ok"], json!(true), "another gap is another point");
        let revised = json!({"block": "limits", "gap": "tamanho", "from": "gap", "status": "open",
            "replaces": first["id"], "origin": said, "facts": facts});
        assert_eq!(write(root, "point", &revised.to_string())["ok"], json!(true), "a revision is the same point");
    }

    /// A lição vai para o banco de lições, com a spec do `--spec` dizendo
    /// onde ela nasceu; o arquivo de eventos, a página, o `.md` e o índice
    /// ficam como estavam. Sem `--spec`, a lição diz sozinha onde nasceu.
    #[test]
    fn writing_a_lesson_goes_to_the_bank_and_leaves_the_spec_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"um"}"#);
        let specs = root.join(".claude").join("spec");
        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("a página do fim do passo");
        let files = [specs.join("teste").join("spec.ndjson"), specs.join("teste").join("spec.md"), specs.join("teste").join("spec.html"), specs.join("index.ndjson")];
        let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();

        let lesson = r#"{"class":"defect","text":"Um rm -rf na pasta errada perde trabalho.","keys":["apagar","rm"],"applies_to":{"subproject":"apps/rt"}}"#;
        assert_eq!(write(root, "lesson", lesson), json!({"ok": true, "id": 1, "type": "lesson", "class": "defect"}));
        let after: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
        assert!(before == after, "the spec's files did not move");
        let bank = std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap();
        assert!(bank.contains(r#""found_in":{"spec":"teste"}"#) && bank.contains(r#""type":"defect""#), "{bank}");

        let everywhere = r#"{"class":"user_preference","text":"Resposta curta.","keys":["resposta"],"applies_to":{"files":["**"]},"found_in":{"source":"CLAUDE.md"}}"#;
        let second = write_to(root, None, "lesson", everywhere);
        assert_eq!(second["id"], json!(2), "{second}");
        let no_origin = r#"{"class":"defect","text":"t","keys":["k"],"applies_to":{"skill":"s"}}"#;
        let refused = write_to(root, None, "lesson", no_origin);
        assert_eq!(refused["reason"], json!("lesson-origin-missing"), "{refused}");
        assert_eq!(std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap().lines().count(), 2);
    }

    /// O `search` é gravado no arquivo de eventos e nunca aparece na página
    /// nem no `.md`: os dois mostram só o texto original.
    #[test]
    fn the_page_and_the_md_never_show_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine"}"#);
        let rule = r#"{"text":"Apagando a pasta, a trava barra o comando.","keys":["apagar","trava"],"example":"rm -rf pasta","origin":1}"#;
        assert_eq!(write(root, "rule", rule)["ok"], json!(true));
        let spec = root.join(".claude").join("spec").join("teste");
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        let line = events.lines().find(|l| l.contains("\"type\":\"rule\"")).unwrap();
        let search = serde_json::from_str::<Value>(line).unwrap()["search"].as_str().unwrap().to_string();
        assert!(search.contains(' '), "{search}");
        super::super::pages::refresh(root, "teste", Locale::PtBr).expect("a página do fim do passo");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(shown.contains("Apagando a pasta, a trava barra o comando."), "{page}");
            assert!(!shown.contains(&search) && !shown.contains("\"search\":"), "{page} shows the search field");
        }
    }

    /// Uma spec nascida em plano, com uma mensagem, um critério e as ondas
    /// `1..=waves`; devolve o número da mensagem e o do critério.
    fn planned_with_waves(root: &std::path::Path, waves: u64) -> (u64, u64) {
        born(root);
        let msg = write(root, "message", r#"{"author":"user","text":"o plano"}"#)["id"].as_u64().unwrap();
        let criterion = json!({"when": "w", "then": "t", "proof": "cargo test", "origin": msg});
        let criterion = write(root, "criterion", &criterion.to_string())["id"].as_u64().unwrap();
        for n in 1..=waves {
            assert_eq!(write_wave(root, n, msg, criterion, None)["ok"], json!(true));
        }
        (msg, criterion)
    }

    fn write_wave(root: &std::path::Path, n: u64, origin: u64, criterion: u64, replaces: Option<u64>) -> Value {
        let mut wave = json!({"n": n, "text": format!("Onda {n}."), "criteria": [criterion], "done_when": "d", "origin": origin});
        if let Some(old) = replaces {
            wave["replaces"] = json!(old);
        }
        write(root, "wave", &wave.to_string())
    }

    /// Um pedido do usuário depois da aprovação entra na mesma spec, na mesma
    /// branch: nenhuma pasta de spec nova, nenhuma branch nova, e a aprovação
    /// continua valendo. O relatório diz o passo seguinte pelo efeito.
    #[test]
    fn a_request_after_approval_keeps_the_spec_and_the_branch() {
        use mustard_core::platform::i18n::Locale;
        let dir = tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "feature/teste");
        born(root);
        witness_approves(root);
        let branches = || {
            git::run(root, &["branch", "--list"]).stdout
        };
        let before = branches();

        let msg = write(root, "message", r#"{"author":"user","text":"inclua o Windows"}"#)["id"].as_u64().unwrap();
        for (effect, key) in [("new_waves", "request.new_waves"), ("adjust_waves", "request.adjust_waves")] {
            let request = json!({"text": "Incluir o Windows.", "keys": ["windows"], "effect": effect, "origin": msg});
            let out = write(root, "request", &request.to_string());
            assert_eq!(out["ok"], json!(true), "{out}");
            assert_eq!(out["spec"], json!("teste"), "{out}");
            assert_eq!(out["next"], json!(translate(key, Locale::PtBr)), "{out}");
        }
        let note = write(root, "note", &json!({"text": "t", "keys": ["k"], "origin": msg}).to_string());
        assert!(note.get("next").is_none(), "only a request says the next step: {note}");

        let state = DiskSpecState::new(root).state("teste").unwrap();
        assert!(state.approved, "the user's request needs no new approval");
        assert_eq!(state.branch.as_deref(), Some("feature/teste"));
        assert_eq!(branches(), before, "no branch was created");
        let specs: Vec<String> = std::fs::read_dir(root.join(".claude").join("spec"))
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(specs, ["teste"], "no spec was opened");
    }

    /// Quatro ondas aprovadas e duas novas: cada onda nova é gravada, sai
    /// sem recusa e avisa a conta; a segunda diz "tinha 4, agora tem 6". A
    /// versão nova de uma onda não avisa.
    #[test]
    fn new_waves_after_approval_warn_the_growth_and_are_written() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (msg, criterion) = planned_with_waves(root, 4);
        let before = write_wave(root, 5, msg, criterion, None);
        assert!(before.get("warnings").is_none(), "no approval yet, no growth: {before}");
        let removed = write(root, "remove", &json!({"targets": [before["id"]], "reason": "cedo"}).to_string());
        assert_eq!(removed["ok"], json!(true), "{removed}");
        witness_approves(root);

        let fifth = write_wave(root, 5, msg, criterion, None);
        assert_eq!(fifth["ok"], json!(true), "{fifth}");
        assert_eq!(fifth["warnings"], json!(["A spec tinha 4 ondas aprovadas, agora tem 5."]), "{fifth}");
        let sixth = write_wave(root, 6, msg, criterion, None);
        assert_eq!(sixth["ok"], json!(true), "{sixth}");
        assert_eq!(sixth["warnings"], json!(["A spec tinha 4 ondas aprovadas, agora tem 6."]), "{sixth}");

        let revised = write_wave(root, 6, msg, criterion, sixth["id"].as_u64());
        assert_eq!(revised["ok"], json!(true), "{revised}");
        assert!(revised.get("warnings").is_none(), "a new version of a wave is not growth: {revised}");

        let log = DiskSpecState::new(root).log("teste").unwrap();
        assert_eq!(mustard_core::domain::spec_state::waves_now(&log), 6, "the new waves are in the file");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// Acrescenta na lista de pendências do projeto em `root` uma pendência
    /// aberta com o título `title` e devolve o número dela.
    fn add_pending(root: &std::path::Path, title: &str) -> String {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        let out = pending_at(&PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some(title.into()),
            detail: Some("combinado na spec".into()),
            ..PendingOpts::default()
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_str().unwrap().to_string()
    }

    /// Um projeto em pasta temporária, com o `mustard.json` que prende a
    /// lista de pendências nele, e uma mensagem do usuário na spec `teste`.
    fn project_with_message() -> (tempfile::TempDir, u64) {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let msg = write(dir.path(), "message", r#"{"author":"user","text":"e o antivírus?"}"#)["id"].as_u64().unwrap();
        (dir, msg)
    }

    fn deferred(root: &std::path::Path, pending: Value, origin: u64) -> Value {
        let draft = json!({"text": "Medir o antivírus do Windows.", "keys": ["windows"], "pending": pending, "origin": origin});
        write(root, "deferred", &draft.to_string())
    }

    /// O pedido de outro assunto vira uma pendência com número e, na spec, só
    /// o pedido adiado que aponta para ela, nunca uma onda. A cobrança da
    /// entrega acha a pendência pelo pedido adiado.
    #[test]
    fn a_request_on_another_subject_becomes_a_numbered_pending_and_a_deferred() {
        use crate::commands::event::pending::{born_in, open_born_in};
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let id = add_pending(root, "Medir o antivírus do Windows");
        assert_eq!(id, "P-1");

        let out = deferred(root, json!(id), msg);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["code"], json!("MSTD-DEFER-0001"), "{out}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let event = log.visible().into_iter().find(|event| event.event_type == "deferred").unwrap();
        assert_eq!(event.int("pending"), Some(1));
        assert_eq!(born_in(&log), ["P-1"]);
        let open: Vec<String> = open_born_in(root, &log).into_iter().map(|item| item.id).collect();
        assert_eq!(open, ["P-1"], "the delivery asks about it");
        assert_eq!(mustard_core::domain::spec_state::waves_now(&log), 0, "never a wave");
    }

    /// O número da pendência pode vir como `P-n`, como `p-n` ou como o número
    /// puro: no arquivo, fica sempre o número.
    #[test]
    fn a_pending_number_written_as_p_n_is_recorded_as_the_number() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        add_pending(root, "um");
        add_pending(root, "dois");
        for (pending, number) in [(json!("P-2"), 2), (json!(" p-1 "), 1), (json!(2), 2), (json!("1"), 1)] {
            let out = deferred(root, pending.clone(), msg);
            assert_eq!(out["ok"], json!(true), "{pending}: {out}");
            let log = DiskSpecState::new(root).log("teste").unwrap();
            let written = log.get(out["id"].as_u64().unwrap()).unwrap();
            assert_eq!(written.fields.get("pending"), Some(&json!(number)), "{pending}");
        }
        let odd = deferred(root, json!("P-dois"), msg);
        assert_eq!(odd["reason"], json!("invalid-value"), "{odd}");
    }

    /// Um número de pendência que não é inteiro positivo (negativo, zero ou
    /// com fração) é recusado como pendência que a lista não tem, e nada é
    /// gravado.
    #[test]
    fn a_deferred_request_with_a_number_that_is_not_a_positive_integer_is_refused() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        add_pending(root, "a única");
        let before = lines(root);
        for (pending, shown) in [(json!(-12), "-12"), (json!(0), "0"), (json!(1.5), "1.5")] {
            let out = deferred(root, pending.clone(), msg);
            assert_eq!(out["reason"], json!("deferred-unknown-pending"), "{pending}: {out}");
            assert!(out["hint"].as_str().unwrap().contains(shown), "{pending}: {out}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// Um pedido adiado para uma pendência que a lista não tem é recusado,
    /// com o comando que cria a pendência, e nada é gravado; sem lista
    /// nenhuma, também.
    #[test]
    fn a_deferred_request_pointing_to_a_missing_pending_is_refused() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let before = lines(root);
        let no_list = deferred(root, json!(1), msg);
        assert_eq!(no_list["reason"], json!("deferred-unknown-pending"), "{no_list}");
        add_pending(root, "a única");
        for pending in [json!(7), json!("P-7")] {
            let out = deferred(root, pending, msg);
            assert_eq!(out["reason"], json!("deferred-unknown-pending"), "{out}");
            let hint = out["hint"].as_str().unwrap();
            assert!(hint.contains("P-7") && hint.contains("mustard-rt run pending --add"), "{hint}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// Um pedido adiado para uma pendência fechada ou descartada é recusado:
    /// a cobrança da entrega nunca veria esse pedido.
    #[test]
    fn a_deferred_request_pointing_to_a_closed_pending_is_refused() {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let closed = add_pending(root, "fechada");
        let dropped = add_pending(root, "descartada");
        let settle = |close: Option<String>, drop: Option<String>, confirm: Option<String>| {
            let out = pending_at(&PendingOpts {
                root: root.to_path_buf(),
                close,
                drop,
                confirm,
                reason: Some("resolvida".into()),
                ..PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "{out}");
            out
        };
        settle(Some(closed.clone()), None, None);
        // A remoção sai em duas chamadas: a prévia devolve o código, e a
        // segunda, com ele, descarta.
        let preview = settle(None, Some(dropped.clone()), None);
        let token = preview["token"].as_str().map(str::to_string);
        assert_eq!(settle(None, Some(dropped.clone()), token)["removed"], json!([dropped]));
        let before = lines(root);
        for id in [closed, dropped] {
            let out = deferred(root, json!(id), msg);
            assert_eq!(out["reason"], json!("deferred-closed-pending"), "{out}");
            assert!(out["hint"].as_str().unwrap().contains(&id), "{out}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// De um worktree ligado, o pedido adiado confere a lista do checkout
    /// principal, e vai para a spec de lá.
    #[test]
    fn a_deferred_request_from_a_linked_worktree_checks_the_list_of_the_main_checkout() {
        let tmp = tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        repo_on(&main, "dev");
        let id = add_pending(&main, "Medir o antivírus do Windows");
        let wt = tmp.path().join("wt");
        assert!(
            git::run(&main, &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feature/teste"]).ok,
            "git worktree add failed",
        );

        let msg = write(&wt, "message", r#"{"author":"user","text":"e o antivírus?"}"#)["id"].as_u64().unwrap();
        let out = deferred(&wt, json!(id), msg);
        assert_eq!(out["ok"], json!(true), "{out}");
        let missing = deferred(&wt, json!("P-2"), msg);
        assert_eq!(missing["reason"], json!("deferred-unknown-pending"), "{missing}");
        let events = std::fs::read_to_string(main.join(".claude/spec/teste/spec.ndjson")).unwrap();
        assert!(events.contains("\"type\":\"deferred\""), "the request lives in the main checkout: {events}");
        assert!(!wt.join(".claude").exists(), "nothing of the Mustard inside the worktree");
    }

    /// Uma spec em levantamento, nascida pela porta do `open`, sem o git.
    fn surveyed(root: &std::path::Path) {
        assert_eq!(record_open(root, "teste", "feature/teste", "dev"), Ok(true));
    }

    fn message(root: &std::path::Path, author: &str, text: &str) -> u64 {
        write(root, "message", &json!({ "author": author, "text": text }).to_string())["id"].as_u64().unwrap()
    }

    fn context(root: &std::path::Path, text: &str, origin: u64) -> Value {
        write(root, "context", &json!({ "text": text, "origin": origin }).to_string())
    }

    /// O primeiro `context` de uma spec em levantamento é a resposta do
    /// usuário palavra por palavra: outro texto, ou a mensagem que não é do
    /// usuário, é recusado, e nada é gravado; a resposta igual entra, e o
    /// `context` seguinte já não é o objetivo.
    #[test]
    fn the_first_context_of_a_survey_is_the_users_answer_word_for_word() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let answer = "Travar o merge com pendência aberta.";
        let said = message(root, "user", answer);
        let reply = message(root, "assistant", answer);
        let before = lines(root);
        let reworded = context(root, "Travar o merge.", said);
        assert_eq!(reworded["reason"], json!("goal-not-verbatim"), "{reworded}");
        assert!(reworded["hint"].as_str().unwrap().contains(&said.to_string()), "{reworded}");
        let from_reply = context(root, answer, reply);
        assert_eq!(from_reply["reason"], json!("goal-not-verbatim"), "{from_reply}");
        assert_eq!(lines(root), before, "a refusal writes nothing");
        assert_eq!(context(root, answer, said)["ok"], json!(true));
        assert_eq!(context(root, "Outro contexto, livre.", said)["ok"], json!(true));
    }

    /// A resposta do assistente, gravada pelo binário como o despachante a
    /// grava no fim do turno, ligada à mensagem `reply_to`.
    fn response(root: &std::path::Path, text: &str, reply_to: u64) -> u64 {
        let reply = json!({ "author": "assistant", "text": text, "reply_to": reply_to });
        record(root, "teste", "response", reply.as_object().cloned().unwrap(), PhaseWriter::Binary)
            .expect("the binary records the response")
            .written
            .id
    }

    /// Uma chamada do Claude Code a um gancho, na sessão `s1`: o evento e os
    /// campos que ele traz.
    fn hook_call(root: &std::path::Path, event: &str, raw: Value) -> HookInput {
        HookInput {
            hook_event_name: Some(event.to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        }
    }

    /// O caminho de verdade, do jeito que a sessão o percorre: a spec nasce,
    /// o assistente fecha o turno sugerindo o objetivo, e o usuário responde
    /// "pode usar essa". A resposta do turno em que a spec nasceu entra pelo
    /// gancho do fim da resposta, sem mensagem a que responder; o sim entra
    /// pelo gancho da entrada da mensagem; e o `run write` do objetivo grava a
    /// frase sugerida, com `origin` na mensagem do usuário, que o índice
    /// mostra. A frase só em parte, com a maiúscula trocada, cortada no fim
    /// ou no começo de uma palavra, e a de uma resposta que veio depois do
    /// sim, são recusadas, e nada é gravado.
    #[test]
    fn a_yes_to_the_suggested_goal_records_the_suggestion_word_for_word() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, "teste");
        surveyed(root);
        let suggestion = "Um sim aprova o objetivo sugerido.";
        let stop = |text: &str| hook_call(root, "Stop", json!({ "last_assistant_message": text }));
        let opening = run_event(Some(Trigger::Stop), &stop(&format!("A spec nasceu. Sugiro: \"{suggestion}\" Serve?")));
        assert!(!opening.is_blocking(), "{opening:?}");
        let yes_prompt = hook_call(root, "UserPromptSubmit", json!({ "prompt": "pode usar essa" }));
        assert!(!run_event(Some(Trigger::UserPromptSubmit), &yes_prompt).is_blocking());
        let later = run_event(Some(Trigger::Stop), &stop("Gravo: \"Travar tudo, sempre.\""));
        assert!(!later.is_blocking(), "{later:?}");

        let log = DiskSpecState::new(root).log("teste").unwrap();
        let talk: Vec<(&str, Option<&str>, Option<u64>)> = log
            .visible()
            .into_iter()
            .filter(|e| matches!(e.event_type.as_str(), "message" | "response"))
            .map(|e| (e.event_type.as_str(), e.str_field("author"), e.int("reply_to")))
            .collect();
        let yes = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        assert_eq!(
            talk,
            [("response", Some("assistant"), None), ("message", Some("user"), None), ("response", Some("assistant"), Some(yes))],
            "the opening answer is recorded before the yes, without a message to reply to"
        );

        let before = lines(root);
        for goal in [
            "Um sim aprova o objetivo",
            "o objetivo sugerido.",
            "um sim aprova o objetivo sugerido.",
            "Um sim aprova o objetivo suger",
            "m sim aprova o objetivo sugerido.",
            "Travar tudo, sempre.",
        ] {
            let refused = context(root, goal, yes);
            assert_eq!(refused["reason"], json!("goal-not-verbatim"), "{goal}: {refused}");
            assert!(refused["hint"].as_str().unwrap().contains("a sugestão que ele aprovou"), "{refused}");
        }
        assert_eq!(lines(root), before, "a refusal writes nothing");

        let written = context(root, suggestion, yes);
        assert_eq!(written["ok"], json!(true), "{written}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let goal = mustard_core::domain::survey::goal(&log).expect("the goal was recorded");
        assert_eq!((goal.str_field("text"), goal.int("origin")), (Some(suggestion), Some(yes)));
        assert_eq!(index_goal(root).as_deref(), Some(suggestion));
    }

    /// Com a conversa já andando, vale só a frase que está inteira, palavra
    /// por palavra, na última resposta antes do sim: a frase com palavras a
    /// mais ou trocadas, a sugestão de uma resposta mais antiga, a de uma
    /// resposta que veio depois da mensagem e a que aponta em `origin` uma
    /// resposta do assistente, e não a mensagem do usuário, são recusadas, e
    /// nada é gravado; a sugestão da resposta respondida passa.
    #[test]
    fn only_the_answered_response_lends_its_suggestion() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let asked = message(root, "user", "Quero que o sim baste.");
        response(root, "Sugiro: \"Travar o envio com pendência aberta.\" Serve?", asked);
        let other = message(root, "user", "Não, outra.");
        let suggestion = "Um sim aprova o objetivo sugerido.";
        response(root, &format!("Então sugiro: \"{suggestion}\" Pode ser?"), other);
        let yes = message(root, "user", "pode usar essa");
        let later = response(root, "Gravo: \"Travar tudo, sempre.\"", yes);
        let before = lines(root);
        for (goal, origin) in [
            ("Um sim aprova o objetivo sugerido e a barra fica limpa.", yes),
            ("Um sim aprova o sugerido objetivo.", yes),
            ("m sim aprova o objetivo sugerido.", yes),
            ("Travar o envio com pendência aberta.", yes),
            ("Travar tudo, sempre.", yes),
            (suggestion, later),
        ] {
            let refused = context(root, goal, origin);
            assert_eq!(refused["reason"], json!("goal-not-verbatim"), "{goal}: {refused}");
            assert!(refused["hint"].as_str().unwrap().contains("a sugestão que ele aprovou"), "{refused}");
        }
        assert_eq!(lines(root), before, "a refusal writes nothing");

        let written = context(root, suggestion, yes);
        assert_eq!(written["ok"], json!(true), "{written}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let goal = mustard_core::domain::survey::goal(&log).expect("the goal was recorded");
        assert_eq!((goal.str_field("text"), goal.int("origin")), (Some(suggestion), Some(yes)));
        assert_eq!(index_goal(root).as_deref(), Some(suggestion));
    }

    /// O objetivo errado sai com `remove`, e a próxima resposta do usuário
    /// vira o objetivo, também palavra por palavra.
    #[test]
    fn a_removed_goal_lets_the_next_answer_become_the_goal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let first = message(root, "user", "Quero isso.");
        let goal = context(root, "Quero isso.", first)["id"].as_u64().unwrap();
        let removal = write(
            root,
            "remove",
            &json!({ "targets": [goal], "reason": "o usuário respondeu outra coisa" }).to_string(),
        );
        assert_eq!(removal["ok"], json!(true), "{removal}");
        let second = message(root, "user", "Travar o merge com pendência aberta.");
        assert_eq!(context(root, "Qualquer coisa.", second)["reason"], json!("goal-not-verbatim"));
        assert_eq!(context(root, "Travar o merge com pendência aberta.", second)["ok"], json!(true));
    }

    /// O objetivo que a linha da spec no índice mostra.
    fn index_goal(root: &std::path::Path) -> Option<String> {
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        index
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|line| line["name"] == json!("teste"))
            .and_then(|line| line["goal"].as_str().map(str::to_string))
    }

    fn revise(root: &std::path::Path, id: u64, text: &str, origin: u64) -> Value {
        write(root, "context", &json!({ "text": text, "origin": origin, "replaces": id }).to_string())
    }

    fn remove(root: &std::path::Path, id: u64) -> Value {
        write(root, "remove", &json!({ "targets": [id], "reason": "não vale" }).to_string())
    }

    /// Revisar o objetivo com outras palavras é recusado, e nada é gravado; a
    /// revisão que repete uma resposta nova do usuário passa.
    #[test]
    fn revising_the_goal_takes_a_new_answer_word_for_word() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", "Travar o merge.");
        let goal = context(root, "Travar o merge.", said)["id"].as_u64().unwrap();
        let before = lines(root);
        let reworded = revise(root, goal, "Travar tudo, sempre.", said);
        assert_eq!(reworded["reason"], json!("goal-not-verbatim"), "{reworded}");
        assert_eq!(lines(root), before);
        let again = message(root, "user", "Travar o merge e o envio.");
        assert_eq!(revise(root, goal, "Travar o merge e o envio.", again)["ok"], json!(true));
        assert_eq!(index_goal(root).as_deref(), Some("Travar o merge e o envio."));
    }

    /// Tirar o objetivo não promove um `context` que o usuário não escreveu:
    /// a remoção que passaria o lugar para ele é recusada. Tirado o outro
    /// antes, o objetivo sai, e a vaga fica aberta.
    #[test]
    fn removing_the_goal_never_promotes_a_context_the_user_did_not_write() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", "Travar o merge.");
        let goal = context(root, "Travar o merge.", said)["id"].as_u64().unwrap();
        let free = context(root, "Uma nota do assistente.", said)["id"].as_u64().unwrap();
        let promoted = remove(root, goal);
        assert_eq!(promoted["reason"], json!("goal-not-verbatim"), "{promoted}");
        assert_eq!(index_goal(root).as_deref(), Some("Travar o merge."));
        assert_eq!(remove(root, free)["ok"], json!(true));
        assert_eq!(remove(root, goal)["ok"], json!(true));
        assert_eq!(index_goal(root), None);
    }

    /// O índice e a regra do objetivo leem o mesmo objetivo: em cada caminho
    /// que troca o objetivo, aceito ou recusado, o que a linha do índice
    /// mostra é o objetivo que a regra conferiu, e ele é sempre uma mensagem
    /// do usuário palavra por palavra.
    #[test]
    fn the_index_and_the_goal_rule_see_the_same_goal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let seen = |root: &std::path::Path| -> Option<String> {
            let log = DiskSpecState::new(root).log("teste").unwrap();
            let shown = index_goal(root);
            assert_eq!(shown, spec_index::goal_of(&log), "the index file and its reading");
            match mustard_core::domain::survey::goal(&log) {
                Some(goal) => {
                    let said = goal.int("origin").and_then(|id| log.get(id)).expect("the goal has its origin");
                    assert_eq!(said.str_field("author"), Some("user"));
                    assert_eq!(said.str_field("text"), goal.str_field("text"));
                    assert_eq!(shown.as_deref(), goal.str_field("text"));
                }
                None => assert_eq!(shown, None),
            }
            shown
        };
        let first = message(root, "user", "Travar o merge.");
        assert_eq!(context(root, "Outra coisa.", first)["reason"], json!("goal-not-verbatim"));
        assert_eq!(seen(root), None);
        let goal = context(root, "Travar o merge.", first)["id"].as_u64().unwrap();
        assert_eq!(seen(root).as_deref(), Some("Travar o merge."));
        assert_eq!(revise(root, goal, "Outra coisa.", first)["reason"], json!("goal-not-verbatim"));
        let other = context(root, "Uma nota.", first)["id"].as_u64().unwrap();
        assert_eq!(remove(root, goal)["reason"], json!("goal-not-verbatim"));
        assert_eq!(seen(root).as_deref(), Some("Travar o merge."));
        let second = message(root, "user", "Travar o envio.");
        assert_eq!(revise(root, other, "Travar o envio.", second)["ok"], json!(true));
        assert_eq!(seen(root).as_deref(), Some("Travar o merge."));
        assert_eq!(remove(root, goal)["ok"], json!(true));
        assert_eq!(seen(root).as_deref(), Some("Travar o envio."));
    }

    /// Uma spec que nasce em plano, como as do `spec-draft`, não tem a vaga do
    /// objetivo: o primeiro `context` dela é livre, e a porta do `open` nunca
    /// a leva de volta ao levantamento.
    #[test]
    fn a_spec_born_in_plan_keeps_a_free_first_context_and_never_returns_to_the_survey() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        let said = message(root, "user", "oi");
        assert_eq!(context(root, "Um contexto qualquer.", said)["ok"], json!(true));
        let before = lines(root);
        assert_eq!(record_open(root, "teste", "feature/teste", "dev"), Ok(false));
        assert_eq!(lines(root), before);
        assert_eq!(DiskSpecState::new(root).state("teste").unwrap().phase, Some("plan"));
    }

    /// O tipo de trabalho sai só pelo `grill`: o `run write work_type` é
    /// recusado nos dois idiomas, o modelo também não o tira, e nada é
    /// gravado.
    #[test]
    fn the_work_type_is_written_only_by_grill() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", "Travar o merge.");
        let draft = json!({ "kinds": ["feature"], "origin": said }).to_string();
        let before = lines(root);
        let refused = write(root, "work_type", &draft);
        assert_eq!(refused["reason"], json!("work-type-by-grill"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("mustard-rt run grill"), "{refused}");
        assert_eq!(lines(root), before);

        let mut by_grill = Map::new();
        by_grill.insert("kinds".to_string(), json!(["feature"]));
        by_grill.insert("origin".to_string(), json!(said));
        let recorded = record(root, "teste", "work_type", by_grill, PhaseWriter::Binary).unwrap().written.id;
        let before = lines(root);
        let removal = write(root, "remove", &json!({ "targets": [recorded], "reason": "outro tipo" }).to_string());
        assert_eq!(removal["reason"], json!("work-type-by-grill"), "{removal}");
        assert_eq!(lines(root), before);

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let english = write(root, "work_type", &draft);
        assert!(english["hint"].as_str().unwrap().contains("is written by `mustard-rt run grill`"), "{english}");
    }

    const GOAL: &str = "Travar o merge com pendência aberta.";

    /// Um ponto do levantamento gravado aberto.
    struct Opened {
        id: u64,
        code: String,
        block: String,
        gap: String,
    }

    /// Uma spec em levantamento com o objetivo, o tipo de trabalho `kinds`
    /// gravado pela porta do binário, como o `grill` grava, e um ponto aberto
    /// por lacuna, na ordem da lista; no condensado, todos num bloco só.
    /// Enquanto a lista está sendo gravada, cada gravação pede os pontos das
    /// lacunas que faltam; a gravação do último ponto devolve o primeiro.
    /// Devolve a mensagem do objetivo e os pontos.
    fn listed(root: &std::path::Path, kinds: &[&str], condensed: bool) -> (u64, Vec<Opened>) {
        surveyed(root);
        let said = message(root, "user", GOAL);
        assert_eq!(context(root, GOAL, said)["ok"], json!(true));
        let mut work_type = Map::new();
        work_type.insert("kinds".to_string(), json!(kinds));
        work_type.insert("origin".to_string(), json!(said));
        assert!(record(root, "teste", "work_type", work_type, PhaseWriter::Binary).is_ok());
        let gaps = survey::gaps(kinds);
        let mut points = Vec::new();
        for (i, key) in gaps.iter().enumerate() {
            let block = if condensed { survey::CONDENSED } else { key.block() };
            let gap = key.label(Locale::PtBr);
            let point = json!({"block": block, "gap": gap, "from": "gap", "status": "open", "origin": said,
                "facts": [{"text": GOAL, "source": format!("mensagem {said}")}]});
            let report = write(root, "point", &point.to_string());
            assert_eq!(report["ok"], json!(true), "{report}");
            points.push(Opened {
                id: report["id"].as_u64().unwrap(),
                code: report["code"].as_str().unwrap().to_string(),
                block: block.to_string(),
                gap: gap.to_string(),
            });
            if i + 1 < gaps.len() {
                let ask = translate("survey.record_points", Locale::PtBr).replace("{spec}", "teste");
                assert_eq!(report["next"], json!(ask), "{report}");
                let left: Vec<&str> =
                    report["points"].as_array().unwrap().iter().filter_map(|item| item["gap"].as_str()).collect();
                let expected: Vec<&str> = gaps[i + 1..].iter().map(|key| key.label(Locale::PtBr)).collect();
                assert_eq!(left, expected, "each write asks for the gaps still without a point: {report}");
                assert!(report.get("point").is_none(), "no point before the list is recorded: {report}");
            } else if condensed {
                assert_eq!(report["points"].as_array().map(Vec::len), Some(gaps.len()), "{report}");
            } else {
                assert_eq!(report["point"]["id"], json!(points[0].id), "{report}");
            }
        }
        (said, points)
    }

    /// Uma resposta: uma decisão com a mensagem de origem.
    fn answer(root: &std::path::Path, origin: u64) -> Value {
        write(root, "decision", &json!({"text": "Resposta.", "keys": ["k"], "why": "w", "origin": origin}).to_string())
    }

    /// Um ponto que fecha `closes` com a resposta `record`.
    fn close(root: &std::path::Path, point: &Opened, closes: Value, record: u64, origin: u64) -> Value {
        let closing = json!({"block": point.block, "gap": point.gap, "from": "gap", "status": "closed",
            "closes": closes, "result": [record], "origin": origin});
        write(root, "point", &closing.to_string())
    }

    /// Responde e fecha o ponto; devolve o relatório do fechamento.
    fn settle(root: &std::path::Path, point: &Opened, origin: u64) -> Value {
        let decided = answer(root, origin)["id"].as_u64().unwrap();
        let closed = close(root, point, json!(point.id), decided, origin);
        assert_eq!(closed["ok"], json!(true), "{closed}");
        closed
    }

    fn unrouted_ids(report: &Value) -> Vec<u64> {
        report["unrouted"]
            .as_array()
            .unwrap_or_else(|| panic!("no unrouted: {report}"))
            .iter()
            .map(|m| m["id"].as_u64().unwrap())
            .collect()
    }

    /// A gravação da fase de plano pela porta do binário.
    fn to_plan(root: &std::path::Path) -> Result<Recorded, Refusal> {
        let mut draft = Map::new();
        draft.insert("phase".to_string(), json!("plan"));
        draft.insert("author".to_string(), json!("binary"));
        record(root, "teste", "state", draft, PhaseWriter::Binary)
    }

    /// Cada resposta gravada devolve o próximo ponto aberto: o mesmo, enquanto
    /// ele não fecha, com o código e o número para o fechamento; o
    /// fechamento devolve o seguinte.
    #[test]
    fn each_answer_returns_the_next_open_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let answered = answer(root, said);
        assert_eq!(answered["point"]["id"], json!(points[0].id), "{answered}");
        let next = answered["next"].as_str().unwrap();
        assert!(next.contains(&points[0].code) && next.contains(&points[0].id.to_string()), "{next}");
        assert!(answered.get("review").is_none(), "{answered}");
        assert_eq!(answer(root, said)["point"]["id"], json!(points[0].id), "the same point until it closes");
        assert_eq!(settle(root, &points[0], said)["point"]["id"], json!(points[1].id));
    }

    /// Fechar o último ponto de um bloco devolve a revisão dele: a pergunta,
    /// "Seguir" por último, os pontos do bloco e as respostas que os
    /// fecharam; o próximo ponto vem junto, para depois da revisão.
    #[test]
    fn closing_the_last_point_of_a_block_returns_the_block_review_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        for point in &points[..3] {
            let report = settle(root, point, said);
            assert!(report.get("review").is_none(), "{report}");
        }
        let report = settle(root, &points[3], said);
        let review = &report["review"];
        assert_eq!(review["block"], json!("defect"), "{report}");
        assert_eq!(review["question"], json!("Quer ver mais algum ponto ou aprofundar algum?"));
        assert_eq!(review["options"], json!(["Seguir"]));
        let codes: Vec<&str> = points[..4].iter().map(|p| p.code.as_str()).collect();
        assert_eq!(review["points"], json!(codes));
        assert_eq!(review["records"], json!(["MSTD-DEC-0001", "MSTD-DEC-0002", "MSTD-DEC-0003", "MSTD-DEC-0004"]));
        assert_eq!(report["point"]["id"], json!(points[4].id), "{report}");
        let next = report["next"].as_str().unwrap();
        assert!(next.starts_with("O bloco defect fechou.") && next.contains(&points[4].code), "{next}");
    }

    /// No último bloco, a revisão oferece o revisor de fora, antes de
    /// "Seguir", nos dois idiomas, e o fim do levantamento vem junto.
    #[test]
    fn the_last_block_review_offers_the_outside_reviewer() {
        for (config, outside, go_on, done) in [
            (None, "Quer que um revisor de fora confira o levantamento inteiro?", "Seguir", "O levantamento não tem ponto aberto."),
            (
                Some(r#"{"language":{"text":"en-US"}}"#),
                "Would you like an outside reviewer to check the whole survey?",
                "Continue",
                "The survey has no open point.",
            ),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let (said, points) = listed(root, &["fix"], false);
            if let Some(config) = config {
                std::fs::write(root.join("mustard.json"), config).unwrap();
            }
            let mut last = Value::Null;
            for point in &points {
                last = settle(root, point, said);
            }
            assert_eq!(last["review"]["block"], json!("proof"), "{last}");
            assert_eq!(last["review"]["options"], json!([outside, go_on]), "{last}");
            assert!(last.get("point").is_none(), "{last}");
            assert!(last["next"].as_str().unwrap().contains(done), "{last}");
            assert!(last["unrouted"].is_array(), "{last}");
        }
    }

    /// O revisor de fora é oferecido uma vez: fechado o último ponto que veio
    /// dele, a revisão do bloco dele fica só com "Seguir", e o fim vem junto.
    #[test]
    fn closing_the_outside_reviewers_last_point_does_not_offer_the_reviewer_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let mut last = Value::Null;
        for point in &points {
            last = settle(root, point, said);
        }
        let outside = "Quer que um revisor de fora confira o levantamento inteiro?";
        assert_eq!(last["review"]["options"], json!([outside, "Seguir"]), "{last}");
        let gap = "O merge pela interface web";
        let found = json!({"block": "outside_review", "gap": gap, "from": "outside_review", "status": "open",
            "origin": said, "facts": [{"text": "O revisor achou.", "source": format!("mensagem {said}")}]});
        let added = write(root, "point", &found.to_string());
        assert_eq!(added["point"]["id"], added["id"], "the reviewer's point is the next one: {added}");
        let reviewer = Opened {
            id: added["id"].as_u64().unwrap(),
            code: added["code"].as_str().unwrap().to_string(),
            block: "outside_review".to_string(),
            gap: gap.to_string(),
        };
        let closed = settle(root, &reviewer, said);
        assert_eq!(closed["review"]["block"], json!("outside_review"), "{closed}");
        assert_eq!(closed["review"]["options"], json!(["Seguir"]), "offered once: {closed}");
        assert!(closed["unrouted"].is_array(), "{closed}");
    }

    /// O fim do levantamento lista as mensagens do usuário que nenhum
    /// registro aponta: as duas soltas entram; a que virou decisão, a do
    /// objetivo e a do assistente ficam fora.
    #[test]
    fn the_end_of_the_survey_lists_the_user_messages_no_event_points_to() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let loose = [message(root, "user", "E o painel?"), message(root, "user", "E o aviso por e-mail?")];
        let routed = message(root, "user", "O merge trava sempre.");
        message(root, "assistant", "Anotado.");
        for point in &points[..4] {
            settle(root, point, said);
        }
        let decided = answer(root, routed)["id"].as_u64().unwrap();
        let end = close(root, &points[4], json!(points[4].id), decided, said);
        assert_eq!(unrouted_ids(&end), loose, "{end}");
    }

    /// A mensagem que só recebeu resposta continua sem destino.
    #[test]
    fn a_message_only_replied_to_is_still_without_destination() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let asked = message(root, "user", "Isso vale para o dev?");
        let reply = json!({"author": "assistant", "text": "Vale.", "reply_to": asked});
        assert!(record(root, "teste", "response", reply.as_object().cloned().unwrap(), PhaseWriter::Binary).is_ok());
        let mut end = Value::Null;
        for point in &points {
            end = settle(root, point, said);
        }
        assert_eq!(unrouted_ids(&end), [asked]);
    }

    /// Tirada a única decisão que apontava a mensagem, ela volta a ficar sem
    /// destino, e a remoção devolve o fim com ela.
    #[test]
    fn a_message_whose_only_record_was_removed_is_without_destination_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let asked = message(root, "user", "Trave também o envio.");
        let decision = answer(root, asked)["id"].as_u64().unwrap();
        let mut end = Value::Null;
        for point in &points {
            end = settle(root, point, said);
        }
        assert!(unrouted_ids(&end).is_empty(), "{end}");
        let removal = remove(root, decision);
        assert_eq!(removal["ok"], json!(true), "{removal}");
        assert_eq!(unrouted_ids(&removal), [asked], "{removal}");
        let unrelated = write(root, "note", &json!({"text": "Uma nota.", "keys": ["n"], "origin": said}).to_string());
        assert!(unrelated.get("next").is_none(), "a write that changes nothing brings no step: {unrelated}");
    }

    /// No levantamento condensado, fechar os pontos não pede revisão de
    /// bloco: cada fechamento devolve os que faltam, de uma vez, e o último
    /// devolve o fim.
    #[test]
    fn a_condensed_survey_skips_the_block_review_and_goes_to_the_end() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], true);
        for (i, point) in points.iter().enumerate() {
            let report = settle(root, point, said);
            assert!(report.get("review").is_none(), "{report}");
            let left = points.len() - i - 1;
            if left > 0 {
                assert_eq!(report["points"].as_array().map(Vec::len), Some(left), "{report}");
                assert_eq!(report["next"], json!(translate("survey.present_all", Locale::PtBr)));
            } else {
                assert!(report["unrouted"].is_array(), "{report}");
                assert_eq!(report["next"], json!(translate("survey.done", Locale::PtBr)));
            }
        }
    }

    /// Um ponto novo no bloco já revisto reabre o bloco: ele é o próximo
    /// ponto, e o fechamento dele pede a revisão de novo.
    #[test]
    fn a_point_added_after_the_review_reopens_the_block_and_its_closing_asks_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let mut report = Value::Null;
        for point in &points[..4] {
            report = settle(root, point, said);
        }
        assert_eq!(report["review"]["block"], json!("defect"), "{report}");
        let deeper = json!({"block": "defect", "gap": "O sintoma no Windows", "from": "gap", "status": "open",
            "origin": said, "facts": [{"text": "Pedido na revisão.", "source": format!("mensagem {said}")}]});
        let added = write(root, "point", &deeper.to_string());
        assert_eq!(added["point"]["id"], added["id"], "the new point is the next one: {added}");
        let added = Opened {
            id: added["id"].as_u64().unwrap(),
            code: added["code"].as_str().unwrap().to_string(),
            block: "defect".to_string(),
            gap: "O sintoma no Windows".to_string(),
        };
        let again = settle(root, &added, said);
        assert_eq!(again["review"]["block"], json!("defect"), "{again}");
        assert_eq!(again["review"]["points"].as_array().map(Vec::len), Some(5), "{again}");
        assert_eq!(again["point"]["id"], json!(points[4].id), "{again}");
    }

    /// Depois da aprovação, nenhuma gravação traz passo do levantamento.
    #[test]
    fn after_approval_a_write_returns_no_survey_step() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        for point in &points {
            settle(root, point, said);
        }
        assert!(to_plan(root).is_ok(), "every point is closed");
        witness_approves(root);
        let extra = json!({"block": "limits", "gap": "Mais um limite", "from": "gap", "status": "open", "origin": said,
            "facts": [{"text": "f", "source": format!("mensagem {said}")}]});
        for report in [
            write(root, "message", r#"{"author":"user","text":"Mais uma coisa."}"#),
            write(root, "point", &extra.to_string()),
        ] {
            assert_eq!(report["ok"], json!(true), "{report}");
            for field in ["next", "point", "points", "review", "unrouted"] {
                assert!(report.get(field).is_none(), "{field}: {report}");
            }
        }
    }

    /// De um worktree, a gravação vai para o arquivo do checkout principal e
    /// devolve o mesmo próximo ponto que a gravação feita de lá.
    #[test]
    fn a_write_from_a_linked_worktree_returns_the_same_next_point() {
        let tmp = tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        repo_on(&main, "dev");
        let (said, points) = listed(&main, &["fix"], false);
        let wt = tmp.path().join("wt");
        assert!(
            git::run(&main, &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feature/teste"]).ok,
            "git worktree add failed",
        );
        let decision = json!({"text": "Resposta.", "keys": ["k"], "why": "w", "origin": said}).to_string();
        let from_main = write(&main, "decision", &decision);
        let from_wt = write_to(&wt, Some("teste"), "decision", &decision);
        assert_eq!(from_wt["point"], from_main["point"], "{from_wt}");
        assert_eq!(from_wt["point"]["id"], json!(points[0].id));
        let closing = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap", "status": "closed",
            "closes": points[0].id, "result": [from_wt["id"]], "origin": said});
        let closed = write_to(&wt, Some("teste"), "point", &closing.to_string());
        assert_eq!(closed["point"]["id"], json!(points[1].id), "{closed}");
        assert!(!wt.join(".claude").exists(), "nothing of the Mustard inside the worktree");
    }

    /// Com um ponto aberto, a passagem para o plano é recusada com a lista
    /// dos abertos, com o código, o número e a lacuna, nos dois idiomas, e
    /// nada é gravado.
    #[test]
    fn leaving_the_survey_with_an_open_point_is_refused_with_the_list() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        settle(root, &points[0], said);
        let before = lines(root);
        let refusal = to_plan(root).err().expect("an open point holds the survey");
        assert_eq!(refusal.reason(), "survey-open");
        let shown = refusal.message(Locale::PtBr);
        assert!(shown.contains("pontos abertos no levantamento (4)"), "{shown}");
        for point in &points[1..] {
            let listed = format!("{} ({}): {}", point.code, point.id, point.gap);
            assert!(shown.contains(&listed), "{shown}");
        }
        assert!(!shown.contains(&points[0].code), "the closed point is not listed: {shown}");
        assert!(refusal.message(Locale::EnUs).contains("open survey points (4)"));
        assert_eq!(lines(root), before, "nothing was written");
        assert_eq!(DiskSpecState::new(root).state("teste").unwrap().phase, Some("survey"));
    }

    /// Sem o tipo de trabalho, o levantamento não começou: a passagem para o
    /// plano é recusada e manda rodar o `grill`.
    #[test]
    fn leaving_the_survey_before_grill_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", GOAL);
        assert_eq!(context(root, GOAL, said)["ok"], json!(true));
        let refusal = to_plan(root).err().expect("no survey yet");
        assert_eq!(refusal.reason(), "survey-not-started");
        assert!(refusal.message(Locale::PtBr).contains("mustard-rt run grill"), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::EnUs).contains("has had no survey yet"));
    }

    /// Uma lacuna do tipo de trabalho sem ponto segura a passagem, mesmo sem
    /// nenhum ponto aberto: é o ponto que o assistente esqueceu de gravar.
    #[test]
    fn leaving_the_survey_with_a_gap_without_a_point_is_refused_with_the_gap() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", GOAL);
        assert_eq!(context(root, GOAL, said)["ok"], json!(true));
        let mut work_type = Map::new();
        work_type.insert("kinds".to_string(), json!(["fix"]));
        work_type.insert("origin".to_string(), json!(said));
        assert!(record(root, "teste", "work_type", work_type, PhaseWriter::Binary).is_ok());
        let refusal = to_plan(root).err().expect("no point was recorded");
        assert_eq!(refusal.reason(), "survey-gaps-unrecorded");
        let shown = refusal.message(Locale::PtBr);
        assert!(shown.contains("5 lacunas") && shown.contains("Como provar que ficou pronto"), "{shown}");
        assert!(refusal.message(Locale::EnUs).contains("How to prove it is done"));
    }

    /// Uma lacuna que ficou sem ponto é pedida pela gravação seguinte, com o
    /// item a copiar, no lugar do próximo ponto; fechar os outros pontos não
    /// traz o fim nem oferece o revisor de fora. Gravado o ponto que faltava,
    /// ele é o próximo.
    #[test]
    fn a_gap_left_without_a_point_is_asked_again_by_the_next_write() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", GOAL);
        assert_eq!(context(root, GOAL, said)["ok"], json!(true));
        let mut work_type = Map::new();
        work_type.insert("kinds".to_string(), json!(["fix"]));
        work_type.insert("origin".to_string(), json!(said));
        assert!(record(root, "teste", "work_type", work_type, PhaseWriter::Binary).is_ok());
        let forgotten = survey::GapKey::ExpectedVsActual;
        let point_of = |key: survey::GapKey| {
            json!({"block": key.block(), "gap": key.label(Locale::PtBr), "from": "gap", "status": "open",
                "origin": said, "facts": [{"text": GOAL, "source": format!("mensagem {said}")}]})
        };
        let mut points = Vec::new();
        for key in survey::gaps(&["fix"]).into_iter().filter(|key| *key != forgotten) {
            let report = write(root, "point", &point_of(key).to_string());
            points.push(Opened {
                id: report["id"].as_u64().unwrap_or_else(|| panic!("{report}")),
                code: report["code"].as_str().unwrap().to_string(),
                block: key.block().to_string(),
                gap: key.label(Locale::PtBr).to_string(),
            });
        }
        let ask = translate("survey.record_points", Locale::PtBr).replace("{spec}", "teste");
        let asks_again = |report: &Value| {
            assert!(report["next"].as_str().is_some_and(|next| next.ends_with(&ask)), "{report}");
            let item = json!({"block": "defect", "gap": forgotten.label(Locale::PtBr), "from": "gap", "origin": said});
            assert_eq!(report["points"], json!([item]), "{report}");
            assert!(report.get("point").is_none() && report.get("unrouted").is_none(), "{report}");
        };
        asks_again(&answer(root, said));
        let mut last = Value::Null;
        for point in &points {
            last = settle(root, point, said);
            asks_again(&last);
        }
        assert_eq!(last["review"]["block"], json!("proof"), "{last}");
        assert_eq!(last["review"]["options"], json!(["Seguir"]), "the survey is not over: {last}");
        let recorded = write(root, "point", &point_of(forgotten).to_string());
        assert_eq!(recorded["point"]["id"], recorded["id"], "the forgotten point is the next one: {recorded}");
    }

    /// Com todos os pontos fechados, a passagem para o plano passa.
    #[test]
    fn leaving_the_survey_with_every_point_closed_passes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        for point in &points {
            settle(root, point, said);
        }
        assert!(to_plan(root).is_ok());
        assert_eq!(DiskSpecState::new(root).state("teste").unwrap().phase, Some("plan"));
    }

    /// Fechar o que não é um ponto aberto é recusado, com o código, o número
    /// e a lacuna dos pontos abertos, e nada é gravado.
    #[test]
    fn closing_a_point_that_is_not_open_is_refused_with_the_open_list() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let before = lines(root);
        for target in [json!(decided), json!(999)] {
            let refused = close(root, &points[0], target, decided, said);
            assert_eq!(refused["reason"], json!("point-not-open"), "{refused}");
            let hint = refused["hint"].as_str().unwrap();
            for point in &points {
                assert!(hint.contains(&format!("{} ({})", point.code, point.id)), "{hint}");
            }
        }
        assert_eq!(lines(root), before);
    }

    /// Um ponto que fecha outro não fica aberto.
    #[test]
    fn a_point_that_closes_another_cannot_stay_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let before = lines(root);
        let open_closing = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap", "status": "open",
            "closes": points[0].id, "origin": said, "facts": [{"text": "f", "source": format!("mensagem {said}")}]});
        let refused = write(root, "point", &open_closing.to_string());
        assert_eq!(refused["reason"], json!("closing-point-open"), "{refused}");
        assert_eq!(lines(root), before);
    }

    /// "Não se aplica" leva o motivo; com ele, o ponto fecha.
    #[test]
    fn not_applicable_needs_a_reason() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let mut closing = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap",
            "status": "not_applicable", "closes": points[0].id, "result": [decided], "origin": said});
        let refused = write(root, "point", &closing.to_string());
        assert_eq!(refused["reason"], json!("not-applicable-needs-reason"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("`reason`"), "{refused}");
        closing["reason"] = json!("O defeito não aparece fora do Linux.");
        let closed = write(root, "point", &closing.to_string());
        assert_eq!(closed["point"]["id"], json!(points[1].id), "{closed}");
    }

    /// Uma resposta que aponta um evento que não existe é recusada.
    #[test]
    fn a_result_citing_a_missing_event_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let before = lines(root);
        let refused = close(root, &points[0], json!(points[0].id), 999, said);
        assert_eq!(refused["reason"], json!("unknown-target"), "{refused}");
        assert_eq!(lines(root), before);
    }

    /// O mesmo ponto não fecha duas vezes: o segundo fechamento é recusado,
    /// e a lista dos abertos já não o traz.
    #[test]
    fn closing_the_same_point_twice_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        settle(root, &points[0], said);
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let twice = close(root, &points[0], json!(points[0].id), decided, said);
        assert_eq!(twice["reason"], json!("point-not-open"), "{twice}");
        let hint = twice["hint"].as_str().unwrap();
        let (_, open) = hint.split_once("Abertos agora:").unwrap();
        assert!(!open.contains(&points[0].code) && open.contains(&points[1].code), "{hint}");
    }

    /// O ponto fecha pelo código que a página mostra, e a linha gravada
    /// guarda o número.
    #[test]
    fn a_point_closed_by_its_code_is_closed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let closed = close(root, &points[0], json!(points[0].code), decided, said);
        assert_eq!(closed["point"]["id"], json!(points[1].id), "{closed}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        assert_eq!(log.get(closed["id"].as_u64().unwrap()).unwrap().int("closes"), Some(points[0].id));
        let unknown = close(root, &points[1], json!("MSTD-POINT-0099"), decided, said);
        assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
    }

    /// Um ponto revisto fecha pelo número de qualquer versão: a primeira ou a
    /// nova.
    #[test]
    fn a_revised_open_point_is_closed_by_either_number() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let revise = |point: &Opened| -> u64 {
            let revision = json!({"block": point.block, "gap": point.gap, "from": "gap", "status": "open",
                "replaces": point.id, "origin": said, "facts": [{"text": "Revisto.", "source": format!("mensagem {said}")}]});
            let report = write(root, "point", &revision.to_string());
            assert_eq!(report["code"], json!(point.code), "the revision keeps the code: {report}");
            report["id"].as_u64().unwrap()
        };
        revise(&points[0]);
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let by_first = close(root, &points[0], json!(points[0].id), decided, said);
        assert_eq!(by_first["point"]["id"], json!(points[1].id), "closed by its first number: {by_first}");
        let newer = revise(&points[1]);
        let by_newer = close(root, &points[1], json!(newer), decided, said);
        assert_eq!(by_newer["point"]["id"], json!(points[2].id), "closed by its new number: {by_newer}");
    }

    /// A versão nova de um fechamento continua fechando o mesmo ponto.
    #[test]
    fn a_revised_closing_keeps_closing_its_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let closing = settle(root, &points[0], said)["id"].as_u64().unwrap();
        let revision = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap", "status": "closed",
            "closes": points[0].id, "reason": "Resposta revista.", "replaces": closing, "origin": said});
        let revised = write(root, "point", &revision.to_string());
        assert_eq!(revised["point"]["id"], json!(points[1].id), "{revised}");
    }

    /// Um ponto aberto não sai com `remove`, nem pelo número nem pelo código:
    /// ele só fecha. Nada é gravado.
    #[test]
    fn an_open_point_cannot_be_removed_only_closed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let before = lines(root);
        for target in [json!(points[0].id), json!(points[0].code)] {
            let removal = write(root, "remove", &json!({"targets": [target], "reason": "não vale"}).to_string());
            assert_eq!(removal["reason"], json!("open-point-removed"), "{removal}");
            assert!(removal["hint"].as_str().unwrap().contains(&points[0].code), "{removal}");
        }
        assert_eq!(lines(root), before);
        assert_eq!(settle(root, &points[0], said)["point"]["id"], json!(points[1].id));
    }

    /// Um ponto aberto é expurgado só no trecho, pelo número ou pelo código:
    /// o fato dele troca o segredo por "…", o ponto segue aberto e segue sendo
    /// o próximo a apresentar, e a situação e o bloco não mudam. O pedido cujo
    /// trecho não aparece no ponto é recusado, nos dois idiomas, e o arquivo
    /// não muda.
    #[test]
    fn an_open_point_is_purged_only_in_the_excerpt_and_stays_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let secret = ["DB_PASSWORD=", "S3nh4F0rte", "2024"].concat();
        let revised = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap", "status": "open",
            "replaces": points[0].id, "origin": said,
            "facts": [{"text": format!("o banco usa {secret}"), "source": format!("mensagem {said}")}]});
        assert_eq!(write(root, "point", &revised.to_string())["ok"], json!(true));
        let file = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let before = std::fs::read(&file).unwrap();
        let code = &points[0].code;
        let missing = write(root, "purge", &json!({"targets": [code], "reason": "secret", "excerpt": "outra"}).to_string());
        assert_eq!(missing["reason"], json!("purge-excerpt-not-found"), "{missing}");
        let expected = format!(
            "O item {code} não traz o trecho a expurgar: nem o que o pedido indica em `excerpt`, nem texto com \
             cara de segredo. Diga o trecho exato em `excerpt`. Nada foi gravado."
        );
        assert_eq!(missing["hint"], json!(expected), "{missing}");
        let english = Refusal::PurgeExcerptNotFound { code: code.clone() }.message(Locale::EnUs);
        assert!(english.starts_with(&format!("Item {code} does not carry the excerpt to purge")), "{english}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "the file stays the same");

        let purged = write(root, "purge", &json!({"targets": [code], "reason": "secret"}).to_string());
        assert_eq!(purged["ok"], json!(true), "{purged}");
        assert_eq!(purged["point"]["code"], json!(code), "the point is still the next one: {purged}");
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(!raw.contains("S3nh4F0rte"), "the excerpt left the file");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let point = survey::points(&log).into_iter().find(|p| p.first() == points[0].id).expect("the point stays");
        assert!(point.is_open());
        let shown = point.shown();
        assert_eq!(shown.fields["facts"][0]["text"], json!("o banco usa DB_PASSWORD=…"));
        assert_eq!(shown.str_field("status"), Some("open"));
        assert_eq!(shown.str_field("block"), Some(points[0].block.as_str()));
    }

    /// A lacuna de um ponto também é expurgada, só no trecho. Com o segredo
    /// só na lacuna, o expurgo pela procura de segredo e o com o trecho
    /// indicado passam; o trecho sai do arquivo também no fechamento, que
    /// copiou a lacuna do original, e no original que já saiu da leitura,
    /// pelo expurgo do fechamento; e cada ponto segue como estava, o fechado
    /// com a mesma lacuna nos dois lados e o aberto ainda aberto.
    #[test]
    fn a_secret_only_in_the_gap_leaves_the_point_and_its_closing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let block = points[0].block.as_str();
        let secret = ["DB_PASSWORD=", "S3nh4F0rte", "2024"].concat();
        let client = "Loja Exemplo 42";
        let open = |gap: String| {
            let point = json!({"block": block, "gap": gap, "from": "outside_review", "status": "open",
                "origin": said, "facts": [{"text": "O revisor de fora apontou.", "source": format!("mensagem {said}")}]});
            let report = write(root, "point", &point.to_string());
            assert_eq!(report["ok"], json!(true), "{report}");
            (report["id"].as_u64().unwrap(), report["code"].as_str().unwrap().to_string())
        };
        let (closed, closed_code) = open(format!("o revisor colou {secret} no pedido"));
        let (still, still_code) = open(format!("o revisor citou a {client}"));
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let closing = json!({"block": block, "gap": "outra", "from": "outside_review", "status": "closed",
            "closes": closed, "result": [decided], "origin": said});
        let closing = write(root, "point", &closing.to_string());
        assert_eq!(closing["ok"], json!(true), "{closing}");
        let closing = closing["id"].as_u64().unwrap();
        let file = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let raw = std::fs::read_to_string(&file).unwrap();
        let line_of = |raw: &str, id: u64| raw.lines().find(|l| l.contains(&format!("\"id\":{id},"))).unwrap().to_string();
        assert!(line_of(&raw, closing).contains("S3nh4F0rte"), "the closing copied the gap");

        let found = write(root, "purge", &json!({"targets": [closed_code], "reason": "secret"}).to_string());
        assert_eq!(found["ok"], json!(true), "{found}");
        let asked =
            write(root, "purge", &json!({"targets": [still_code], "reason": "client_data", "excerpt": client}).to_string());
        assert_eq!(asked["ok"], json!(true), "{asked}");
        // O original que já saiu da leitura é alcançado pelo expurgo do
        // fechamento dele.
        let (gone, _) = open(format!("o revisor colou {secret} de novo"));
        let last = json!({"block": block, "gap": "outra", "from": "outside_review", "status": "closed",
            "closes": gone, "result": [decided], "origin": said});
        let last = write(root, "point", &last.to_string());
        assert_eq!(last["ok"], json!(true), "{last}");
        let left = write(root, "remove", &json!({"targets": [gone], "reason": "O texto tinha um segredo."}).to_string());
        assert_eq!(left["ok"], json!(true), "{left}");
        let reached = write(root, "purge", &json!({"targets": [last["code"]], "reason": "secret"}).to_string());
        assert_eq!(reached["ok"], json!(true), "{reached}");

        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(!raw.contains("S3nh4F0rte"), "the excerpt left each point and its closing, the removed one too");
        assert!(!raw.contains(client), "the asked excerpt left the gap");
        let gap = "o revisor colou DB_PASSWORD=… no pedido";
        for id in [closed, closing] {
            assert!(line_of(&raw, id).contains(gap), "{}", line_of(&raw, id));
        }
        let again = "o revisor colou DB_PASSWORD=… de novo";
        for id in [gone, last["id"].as_u64().unwrap()] {
            assert!(line_of(&raw, id).contains(again), "{}", line_of(&raw, id));
        }
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let pairs = survey::points(&log);
        let pair = pairs.iter().find(|p| p.first() == closed).expect("the closed point stays");
        assert!(!pair.is_open(), "the point stays closed");
        assert_eq!(pair.closing().map(|c| c.id), Some(closing));
        assert_eq!(pair.gap(), Some(gap));
        let open_one = pairs.iter().find(|p| p.first() == still).expect("the open point stays");
        assert!(open_one.is_open(), "the point stays open");
        assert_eq!(open_one.gap(), Some("o revisor citou a …"));
    }

    /// O veredito, que só o binário grava, também é expurgado só no trecho:
    /// ele continua na leitura, com o resto do texto, e a mesma procura de
    /// segredo que serve à página acha o trecho.
    #[test]
    fn a_verdict_is_purged_only_in_the_excerpt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        let secret = ["ghp_", &"a1".repeat(18)].concat();
        let draft = json!({"wave": 1, "result": "rejected", "author": "review",
            "criteria": [{"criterion": 1, "tests_rule": true}],
            "text": format!("O teste imprime o token {secret} no log.")});
        let verdict = record(root, "teste", "verdict", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
        let code = verdict.written.code.clone().unwrap();
        let purged = write(root, "purge", &json!({"targets": [code], "reason": "secret"}).to_string());
        assert_eq!(purged["ok"], json!(true), "{purged}");
        assert_eq!(purged["purged"], json!([verdict.written.id]), "{purged}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let shown = log.visible().into_iter().find(|e| e.event_type == "verdict").expect("the verdict stays");
        assert_eq!(shown.str_field("text"), Some("O teste imprime o token … no log."));
        assert_eq!(shown.str_field("result"), Some("rejected"));
        let raw = std::fs::read_to_string(root.join(".claude/spec/teste/spec.ndjson")).unwrap();
        assert!(!raw.contains(&secret), "the excerpt left the file, and the purge did not write it");
    }

    /// Enquanto o original existe, tirar o fechamento só reabre o ponto: com
    /// `remove`, ele volta a ser o próximo ponto aberto.
    #[test]
    fn taking_the_closing_out_while_the_original_stands_reopens_the_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let closing = settle(root, &points[0], said)["id"].as_u64().unwrap();
        let out = write(root, "remove", &json!({"targets": [closing], "reason": "Fechei o ponto errado."}).to_string());
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["point"]["id"], json!(points[0].id), "the point is open again: {out}");
    }

    /// Com o original fora, o ponto que o fechou é o único registro dele: o
    /// `remove` dele é recusado, pelo número e pelo código, com o texto nos
    /// dois idiomas, e o arquivo não muda; tirar o original e o fechamento na
    /// mesma gravação também é recusado. O expurgo do fechamento, que só
    /// oculta o trecho, passa, e o ponto segue fechado.
    #[test]
    fn the_closing_of_a_point_whose_original_left_does_not_leave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let not_applicable = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap",
            "status": "not_applicable", "closes": points[0].id, "origin": said,
            "reason": "O fato citava a senha S3nh4F0rte do banco."});
        let closed = write(root, "point", &not_applicable.to_string());
        assert_eq!(closed["ok"], json!(true), "{closed}");
        let closing = closed["id"].as_u64().unwrap();
        let code = closed["code"].as_str().unwrap().to_string();
        let left = write(root, "remove", &json!({"targets": [points[0].id], "reason": "O texto tinha um segredo."}).to_string());
        assert_eq!(left["ok"], json!(true), "{left}");
        let file = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let before = std::fs::read(&file).unwrap();
        let expected = format!(
            "O ponto {code} fecha um ponto cujo texto original já saiu, e é o único registro dele: não sai com \
             `remove`. Para tirar um dado sensível dele, use `purge`, que só oculta o trecho. Nada foi gravado."
        );
        for target in [json!(closing), json!(code)] {
            let out = write(root, "remove", &json!({"targets": [target], "reason": "Não vale mais."}).to_string());
            assert_eq!(out["reason"], json!("closing-point-last-record"), "{out}");
            assert_eq!(out["hint"], json!(expected), "{out}");
        }
        assert_eq!(std::fs::read(&file).unwrap(), before, "the file stays the same");

        let together = settle(root, &points[1], said)["id"].as_u64().unwrap();
        let both = write(root, "remove", &json!({"targets": [points[1].id, together], "reason": "Os dois."}).to_string());
        assert_eq!(both["reason"], json!("closing-point-last-record"), "{both}");
        let english = Refusal::ClosingPointLastRecord { code: "MSTD-POINT-0007".to_string() }.message(Locale::EnUs);
        assert_eq!(
            english,
            "Point MSTD-POINT-0007 closes a point whose original text is already gone, and it is the only record of \
             it: it does not leave with `remove`. To take sensitive data out of it, use `purge`, which only hides \
             the excerpt. Nothing was written."
        );

        let purged = write(root, "purge", &json!({"targets": [code], "reason": "secret", "excerpt": "S3nh4F0rte"}).to_string());
        assert_eq!(purged["ok"], json!(true), "{purged}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let point = survey::points(&log).into_iter().find(|p| p.first() == points[0].id).expect("the point stays");
        assert!(!point.is_open(), "the point stays closed");
        assert_eq!(log.get(closing).unwrap().str_field("reason"), Some("O fato citava a senha … do banco."));
    }

    /// A versão nova de um fechamento que vem sem `closes` e aberta recebe o
    /// ponto que a antiga fechava e é recusada como todo ponto aberto que
    /// fecha outro, pelo número e pelo código da versão antiga. O arquivo não
    /// muda, e o ponto segue fechado.
    #[test]
    fn a_new_version_of_a_closing_cannot_reopen_the_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let closed = settle(root, &points[0], said);
        let file = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let before = std::fs::read(&file).unwrap();
        for old in [closed["id"].clone(), closed["code"].clone()] {
            let reopened = json!({"block": points[0].block, "gap": points[0].gap, "from": "gap", "status": "open",
                "replaces": old, "origin": said, "facts": [{"text": "Reaberto.", "source": format!("mensagem {said}")}]});
            let refused = write(root, "point", &reopened.to_string());
            assert_eq!(refused["reason"], json!("closing-point-open"), "{refused}");
        }
        assert_eq!(std::fs::read(&file).unwrap(), before, "the file stays the same");
        let asked = answer(root, said);
        assert_eq!(asked["point"]["id"], json!(points[1].id), "the point stays closed: {asked}");
    }

    /// A versão nova de um fechamento que aponta outro ponto em `closes` é
    /// gravada com o `closes` da antiga e com a lacuna do ponto que ela
    /// fecha: o mesmo ponto segue fechado, e o outro segue aberto.
    #[test]
    fn a_new_version_of_a_closing_keeps_the_point_the_old_one_closed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, points) = listed(root, &["fix"], false);
        let closing = settle(root, &points[0], said)["id"].as_u64().unwrap();
        let decided = answer(root, said)["id"].as_u64().unwrap();
        let moved = json!({"block": points[1].block, "gap": points[1].gap, "from": "gap", "status": "closed",
            "closes": points[1].id, "result": [decided], "replaces": closing, "origin": said});
        let revised = write(root, "point", &moved.to_string());
        assert_eq!(revised["ok"], json!(true), "{revised}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let written = log.get(revised["id"].as_u64().unwrap()).unwrap();
        assert_eq!(written.int("closes"), Some(points[0].id));
        assert_eq!(written.str_field("gap"), Some(points[0].gap.as_str()));
        assert_eq!(revised["point"]["id"], json!(points[1].id), "the other point stays open: {revised}");
    }

    /// A porta que fica arma a cobrança no fechamento e na entrega, com a
    /// sessão de quem fechou e sem ela, e a entrada na execução não arma nada.
    /// É a porta que o `close` e o `pr-merge` novos vão chamar.
    #[test]
    fn the_binary_door_still_arms_the_charge_on_closed_and_delivered() {
        use crate::commands::event::pending::armed_charges;
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        for spec in ["com-sessao", "sem-sessao"] {
            let folder = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&folder).unwrap();
            let state = json!({"v": 1, "id": 1, "type": "state", "phase": "approved", "author": "binary",
                "at": "2026-09-14T10:00:00Z"});
            std::fs::write(folder.join("spec.ndjson"), format!("{state}\n")).unwrap();
        }

        assert!(record_phase(root, "com-sessao", "running", Some("s-1")), "the spec enters execution");
        assert!(armed_charges(root).is_empty(), "entering execution charges nothing");

        assert!(record_phase(root, "com-sessao", "closed", Some("s-1")), "the close is recorded");
        assert!(record_phase(root, "sem-sessao", "delivered", None), "the merge is recorded");
        let armed: Vec<(String, Option<String>)> =
            armed_charges(root).into_iter().map(|charge| (charge.spec, charge.session)).collect();
        assert!(armed.contains(&("com-sessao".to_string(), Some("s-1".to_string()))), "{armed:?}");
        assert!(armed.contains(&("sem-sessao".to_string(), None)), "{armed:?}");
    }

    /// A porta do pull request aberto grava a fase com o número e o endereço
    /// só numa spec fechada, não arma cobrança nenhuma e não grava de novo; uma
    /// spec em execução fica como está, para ainda poder fechar.
    #[test]
    fn the_open_pull_request_is_recorded_only_on_a_closed_spec_and_charges_nothing() {
        use crate::commands::event::pending::armed_charges;
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        for (spec, phase) in [("em-execucao", "running"), ("fechada", "closed")] {
            let folder = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&folder).unwrap();
            let state = json!({"v": 1, "id": 1, "type": "state", "phase": phase, "author": "binary",
                "at": "2026-09-17T10:00:00Z"});
            std::fs::write(folder.join("spec.ndjson"), format!("{state}\n")).unwrap();
        }
        let phase_of = |spec: &str| DiskSpecState::new(root).state(spec).and_then(|s| s.phase);

        assert!(!record_pr_open(root, "em-execucao", 7, Some("https://exemplo/pull/7")));
        assert_eq!(phase_of("em-execucao"), Some("running"), "a running spec can still close");

        assert!(record_pr_open(root, "fechada", 7, Some("https://exemplo/pull/7")));
        assert_eq!(phase_of("fechada"), Some("pr_open"));
        let log = DiskSpecState::new(root).log("fechada").unwrap();
        let recorded = log.get(log.max_id()).unwrap();
        assert_eq!(recorded.fields["pr"], json!({"number": 7, "url": "https://exemplo/pull/7"}));
        assert_eq!(recorded.str_field("author"), Some("binary"));
        assert!(armed_charges(root).is_empty(), "opening the pull request charges nothing");
        assert!(!record_pr_open(root, "fechada", 7, None), "a second opening records nothing");
    }

    /// Dois fechamentos ao mesmo tempo armam os dois: o arquivo dos
    /// contadores da cobrança é lido e gravado com a trava presa, e nenhum
    /// fechamento se perde.
    #[test]
    fn two_closings_at_once_arm_both() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let names: Vec<Vec<String>> =
            (0..10).map(|round| (0..2).map(|w| format!("spec-{round}-{w}")).collect()).collect();
        for spec in names.iter().flatten() {
            let folder = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&folder).unwrap();
            let state = json!({"v": 1, "id": 1, "type": "state", "phase": "running", "author": "binary",
                "at": "2026-09-13T10:00:00Z"});
            std::fs::write(folder.join("spec.ndjson"), format!("{state}\n")).unwrap();
        }
        for pair in &names {
            std::thread::scope(|scope| {
                for spec in pair {
                    scope.spawn(move || assert!(record_phase(root, spec, "closed", None), "{spec}"));
                }
            });
        }
        let mut armed: Vec<String> =
            crate::commands::event::pending::armed_charges(root).into_iter().map(|charge| charge.spec).collect();
        armed.sort();
        let mut expected: Vec<String> = names.into_iter().flatten().collect();
        expected.sort();
        assert_eq!(armed, expected, "no closing armed at the same time was lost");
    }

    /// Uma spec sem nenhum ponto, como as do `spec-draft`, grava e aprova
    /// como antes: sem passo do levantamento e sem recusa nova.
    #[test]
    fn an_old_spec_without_points_writes_and_approves_as_before() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        let said = message(root, "user", "Travar o merge.");
        let rule = write(root, "rule", &json!({"text": "Regra.", "keys": ["k"], "example": "e", "origin": said}).to_string());
        assert_eq!(rule["ok"], json!(true), "{rule}");
        for field in ["next", "point", "review", "unrouted"] {
            assert!(rule.get(field).is_none(), "{field}: {rule}");
        }
        witness_approves(root);
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }
}
