//! A fila da rodada e as ondas em andamento: quais ondas saem agora, a vaga
//! fixa de cada uma, quais estão em andamento, quais já estão entregues e
//! aprovadas, e o estado de cada uma que a página mostra. A rodada não pede
//! revisão de onda nenhuma: quem confere o trabalho, uma vez por obra, é o
//! agente de teste dedicado que o fechamento pede.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SEND_REQUEST_FIELDS, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, last_change, review_send};
use mustard_core::domain::wave_prompt::{Choice, dispatch_items};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::stops::waves_replanned;
use super::vacancy::Vacancies;
use crate::commands::wave::wave_overlap_check::{WaveGraph, wave_graph};
use crate::commands::spec_events::task_dependents::TaskRefs;
use crate::commands::spec_events::write::record;
use crate::shared::dag::touches_whole_tree;

/// Quantas ondas saem juntas quando o projeto não diz outra coisa: duas. Cada
/// onda compila dentro da própria vaga e trabalha na própria cópia, e cada
/// cópia leva a sua compilação para o disco: duas ondas com o mesmo arquivo
/// saem juntas, e a rodada junta as duas no commit.
const DEFAULT_PARALLEL: usize = 2;

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
pub(crate) fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo as vagas livres da conta
/// única ([`Vacancies::free`]). Duas ondas cujos arquivos se cruzam saem
/// juntas, e a que divide arquivo com uma onda em andamento também sai: cada
/// onda trabalha na própria cópia, e a rodada junta no commit; o trecho que
/// conflitar segura só a volta dele. A onda com o curinga da árvore inteira (`**`) nunca sai ao lado
/// de outra: nem com outra em andamento, nem com ela em andamento. A onda órfã — em andamento sem o
/// processo que a mandou — não conta como vaga ocupada para as outras
/// entrarem, porque ela não está compilando nada; a cópia dela é limpa à
/// parte, em [`open_copies`]. A onda parada pelo limite de consertos
/// (`stuck`) não sai, nem a que depende dela, direta ou por outra onda. A
/// onda que leva a avaliação de uma skill e espera tarefas abertas nos
/// arquivos que a skill cita (`evaluating`,
/// [`waves_waiting`](super::skill_hold::waves_waiting)) também não sai, nem
/// toma vaga.
///
/// A onda de lote nasce sem dependência entre ondas, então uma dependência em
/// círculo não tem de onde vir pelo backlog, e o plano não a procura. O ciclo
/// sai da leitura do grafo ([`wave_graph`]) que esta função reaproveita —
/// nunca uma conta própria à parte —, e, se uma onda gravada antes do backlog
/// trouxer um, a rodada devolve os números do ciclo em vez de escolher uma
/// ordem às cegas.
pub(crate) fn next_waves(log: &SpecLog, vacancies: &Vacancies, (stuck, evaluating): Holding<'_, '_>) -> Result<Vec<u64>, Vec<u64>> {
    outgoing(log, vacancies, (stuck, evaluating), &BTreeSet::new()).map(|out| out.go)
}

/// O que segura uma onda pronta na fila: a parada pelo limite de consertos,
/// com os eventos que a pararam, e a que espera, pela avaliação de skill que
/// leva, estas tarefas abertas.
pub(crate) type Holding<'a, 'b> = (&'a BTreeMap<u64, Vec<&'b SpecEvent>>, &'a BTreeMap<u64, Vec<u64>>);

/// As ondas que saem nesta rodada ([`next_waves`]) e o que sobra para a
/// montagem do backlog: quantas vagas seguem livres depois delas e se há
/// espaço para uma onda que cruza todas.
pub(super) struct Outgoing {
    /// As ondas que saem, na ordem em que saem.
    pub go: Vec<u64>,
    /// Quantas ondas novas ainda cabem: as vagas do projeto menos as ondas em
    /// andamento, as que seguram a vaga e as que saem agora. Zero quando a que
    /// sai ou a que está em andamento tem o curinga da árvore inteira.
    pub free: usize,
    /// Nada em andamento e nada saindo: só então uma onda com o curinga da
    /// árvore inteira pode sair, e sozinha.
    pub alone: bool,
    /// As ondas prontas, com vaga, que a avaliação de skill que levam segura:
    /// sem ela, sairiam agora.
    pub evaluating: Vec<u64>,
}

/// [`next_waves`] com mais um dado: as ondas de `skip` são tratadas como se
/// já tivessem saído, e não competem pelas vagas. É como a montagem do
/// backlog conta as vagas livres sem as ondas de lote que ela mesma desfaz.
pub(super) fn outgoing(
    log: &SpecLog,
    vacancies: &Vacancies,
    (stuck, evaluating): Holding<'_, '_>,
    skip: &BTreeSet<u64>,
) -> Result<Outgoing, Vec<u64>> {
    let running = &vacancies.open;
    let graph = wave_graph(log);
    // A onda reprovada volta para a fila: sem isso o ciclo de conserto não
    // fecha, porque o fechamento recusa e diz qual refazer e a rodada nunca a
    // despacharia de novo.
    let to_redo = waves_to_redo(log);
    // O pedido gravado descreve o plano daquele momento: a onda que ganhou
    // versão nova depois dele, e ainda não entregou, sai de novo com o pedido
    // do plano atual.
    let replanned = waves_replanned(log);
    // O que já saiu da fila: a onda com pedido e também a onda que já
    // entregou. Só o pedido não basta, porque a onda entregue antes de a
    // rodada existir não tem pedido nenhum e apareceria como pronta para
    // sair — e sairia de novo um trabalho já feito.
    // Quais ondas já entregaram é pergunta do núcleo, e é ele que responde:
    // recalcular o filtro aqui deixava duas camadas decidindo a mesma coisa,
    // concordando hoje e livres para divergir amanhã.
    let delivered = log.delivered_waves();
    let already_out: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .filter(|n| !replanned.contains(n))
        .chain(delivered.iter().copied())
        .filter(|n| !to_redo.contains(n))
        // A onda cuja volta espera a rodada já saiu, mesmo replanejada depois
        // do envio: despachá-la de novo deixaria a volta sem ser assumida, e
        // a cópia dela, com o código entregue, seria a de uma onda nova.
        .chain(log.waves_returned())
        .chain(skip.iter().copied())
        .collect();
    // Só a onda do plano é candidata: a de lote que ficou sem tarefa saiu
    // dele, e despachá-la seria um pedido sem nada dentro — o backlog já
    // reempacota numa onda nova a tarefa que ela perdeu.
    let planned = log.planned_waves();
    let blocked: BTreeSet<u64> = log.visible().into_iter()
        .filter(|event| event.event_type == "task" && event.str_field("blocked_by").is_some())
        .filter_map(SpecEvent::wave).collect();
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = wave.wave().filter(|n| planned.contains(n)) {
            depends.insert(n, wave.ints("depends_on"));
        }
    }
    let done = waves_done(log, running);
    // Quantas ondas novas saem é a conta única das vagas ([`Vacancies::free`]):
    // só a onda com agente trabalhando ocupa vaga, e a que voltou guarda a
    // cópia sem ocupá-la. A órfã não compila nada, e a cópia dela segue presa
    // para o reenvio. A onda replanejada que ainda tem o pedido aberto segue
    // dona da vaga que o envio dela gravou: ao sair de novo volta a ela, sem
    // pedir vaga nenhuma. Por isso sai antes das outras e fora da conta do
    // teto delas.
    let holding = &vacancies.holding;
    let slots = vacancies.free();
    // A onda com o curinga da árvore inteira cruza com todas, até com a que
    // não declara arquivo: com ela em andamento nada mais sai, e ela só sai
    // sem nenhuma outra em andamento nem saindo junto.
    let mut busy = !running.is_empty();
    let mut whole_tree_out = running.keys().any(|n| graph.files.get(n).is_some_and(touches_whole_tree));
    let mut go = Vec::new();
    let mut held = Vec::new();
    let mut fresh = 0;
    let (own, rest): (Vec<u64>, Vec<u64>) = ready_in_order(&depends, &graph, &already_out, &delivered, &done)?.into_iter().partition(|n| holding.contains(n));
    for n in own.into_iter().chain(rest) {
        if whole_tree_out || (!holding.contains(&n) && fresh >= slots) {
            break;
        }
        if stuck.contains_key(&n) || blocked.contains(&n)
            || dependencies_of(n, &depends).iter().any(|d| stuck.contains_key(d) || blocked.contains(d)) {
            continue;
        }
        if evaluating.contains_key(&n) {
            held.push(n);
            continue;
        }
        let whole_tree = touches_whole_tree(graph.files.get(&n).into_iter().flatten());
        if whole_tree && busy {
            continue;
        }
        busy = true;
        whole_tree_out = whole_tree;
        fresh += usize::from(!holding.contains(&n));
        go.push(n);
    }
    let free = if whole_tree_out { 0 } else { slots.saturating_sub(fresh) };
    Ok(Outgoing { go, free, alone: !busy, evaluating: held })
}

/// As ondas com pedido aberto, cada uma com o número do pedido dela: a onda
/// tem pedido e nenhuma entrega depois dele. O pedido mais antigo que a
/// versão mais nova da onda ou de uma tarefa dela descreve um plano que já
/// mudou, e não conta. Uma onda que já entregou só volta a sair por uma
/// reprovação: o pedido que veio depois de uma entrega, sem reprovação entre
/// as duas, não é trabalho em curso, nem o pedido de uma onda que saiu do
/// plano. Não olha se o Claude Code que mandou o pedido segue aberto — é
/// [`waves_in_progress`] e [`orphaned_waves`] que decidem isso, cada uma para
/// o seu lado.
pub(crate) fn open_sends(log: &SpecLog) -> BTreeMap<u64, u64> {
    let replanned = waves_replanned(log);
    log.unanswered_sends().into_iter().filter(|(n, _)| !replanned.contains(n)).collect()
}

/// Grava, uma vez por onda, a nota do binário dizendo que a onda saiu do ar
/// por ficar sem tarefa ([`SpecLog::waves_left_without_tasks`]), no idioma `lang`, e
/// devolve o que gravou. A nota leva o código da onda nas chaves: a onda que
/// já tem a sua não ganha outra.
pub(super) fn record_waves_left_without_tasks(start: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Result<Vec<Value>, Refusal> {
    let left = log.waves_left_without_tasks();
    if left.is_empty() {
        return Ok(Vec::new());
    }
    let codes = log.codes();
    let keys = |e: &SpecEvent| -> Vec<String> {
        e.fields.get("keys").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
    };
    let noted: BTreeSet<String> = log
        .block(BlockQuery::Block(Block::Notes))
        .into_iter()
        .filter(|e| e.event_type == "note" && e.str_field("author") == Some("binary"))
        .map(keys)
        .filter(|keys| keys.iter().any(|key| key == WAVE_LEFT_KEY))
        .flatten()
        .collect();
    let waves = log.block(BlockQuery::Block(Block::Waves));
    let returned = log.waves_returned();
    let mut recorded = Vec::new();
    for (wave, sent) in left {
        let Some(event) = waves.iter().rfind(|e| e.event_type == "wave" && e.wave() == Some(wave)) else {
            continue;
        };
        let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
        if noted.contains(&code) {
            continue;
        }
        let send = codes.get(&sent).cloned().unwrap_or_else(|| sent.to_string());
        // O agente que já gravou a volta antes da remoção não ficou sem volta.
        let key = if returned.contains(&wave) { "round.wave_left_after_return" } else { "round.wave_left_without_tasks" };
        let text = translate(key, lang).replace("{wave}", &wave.to_string()).replace("{send}", &send);
        let mut draft = Map::new();
        draft.insert("text".into(), json!(text));
        draft.insert("keys".into(), json!([code, WAVE_LEFT_KEY]));
        draft.insert("label".into(), json!(code));
        draft.insert("author".into(), json!("binary"));
        let written = record(start, spec, "note", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "note", "id": written.written.id, "text": text }));
    }
    Ok(recorded)
}

/// A chave que marca a nota de onda que saiu do ar por ficar sem tarefa.
const WAVE_LEFT_KEY: &str = "wave-left-without-tasks";

/// O pedido de revisão aberto, pelo número da versão que o gravou: o pedido
/// mais novo da revisão final, que o fechamento grava ([`review_send`]), sem
/// veredito oficial depois dele. A volta que o revisor gravou e ninguém
/// assumiu ainda não fecha o pedido: só o veredito que a rodada ou o
/// fechamento grava ao assumi-la. O veredito do revisor de onda de uma spec
/// antiga nunca o fecha.
pub(crate) fn open_review(log: &SpecLog) -> Option<u64> {
    let asked = review_send(log, None)?.asked;
    (!log.visible().iter().any(|e| e.event_type == "verdict" && !super::is_wave_verdict(e) && e.id > asked)).then_some(asked)
}

/// O pedido de revisão final aberto ([`open_review`]) cujo veredito ainda
/// pode aprovar a obra: o mais novo que a última mudança dela
/// ([`last_change`]), pelo número da versão que gravou o pedido. O pedido
/// aberto mais velho que ela não espera veredito: o revisor não recebeu o que
/// mudou depois dele, e o fechamento abre um pedido novo — também quando o
/// lançamento do revisor gravou uma versão do envio depois da mudança.
pub(crate) fn awaited_review(log: &SpecLog) -> Option<u64> {
    open_review(log).filter(|asked| *asked > last_change(log))
}

/// A versão nova do envio mais recente da onda `wave`: os campos dele, tirando
/// `v`, `id`, `code`, `at`, `type`, `search` e o pedido
/// ([`SEND_REQUEST_FIELDS`], que quem lê acha na versão que o gravou, por
/// [`SpecLog::send_field`]), com `replaces` apontando para ele e os campos de
/// `extra` somados por cima. Usado quando a volta de uma onda traz o consumo —
/// o modelo usado de verdade, os passos e os tokens —, que só se sabe depois
/// do envio já gravado. `None` sem envio para a onda.
pub(crate) fn send_revision(log: &SpecLog, wave: u64, extra: Map<String, Value>) -> Option<Map<String, Value>> {
    revise_send(log, *log.last_by_wave("send").get(&wave)?, extra)
}

/// A versão nova do envio `id`, na forma de [`send_revision`]: o caminho do
/// envio que não é de onda nenhuma, como o da revisão final. `None` sem esse
/// envio.
pub(crate) fn revise_send(log: &SpecLog, id: u64, extra: Map<String, Value>) -> Option<Map<String, Value>> {
    let event = log.get(id).filter(|event| event.event_type == "send")?;
    let mut draft: Map<String, Value> = event
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search"].contains(&key.as_str()) && !SEND_REQUEST_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("replaces".into(), json!(id));
    for (key, value) in extra {
        draft.insert(key, value);
    }
    Some(draft)
}

/// A subida de modelo do agente novo da onda `wave` de `log`, no projeto em
/// `root`: os campos da versão do envio com o modelo um degrau acima do dele
/// e de onde ele subiu, pela volta pendente que a rodada recusou ou quem
/// conduz reprovou ([`mustard_core::domain::agent_model::escalate`]). Vazio
/// com o modelo pela dificuldade desligado, no envio de outro hospedeiro, sem
/// volta pendente, quando o envio já subiu por essa mesma volta e no Opus,
/// que fica.
pub(crate) fn new_agent_escalation(root: &Path, log: &SpecLog, wave: u64) -> Map<String, Value> {
    use mustard_core::domain::agent_model::{ESCALATED_FIELD, Escalation, escalate};
    let raised = || -> Option<Map<String, Value>> {
        if !mustard_core::ProjectConfig::load(root).by_difficulty() {
            return None;
        }
        let sent = *log.last_by_wave("send").get(&wave)?;
        let send = log.get(sent)?;
        if send.str_field("host").unwrap_or("claude") != "claude" {
            return None;
        }
        let cause = super::rejection::pending_return(log, wave)?;
        if Escalation::of(send).is_some_and(|before| before.cause == cause) {
            return None;
        }
        let previous = log.send_field(sent, "model").and_then(Value::as_str)?;
        let chosen = escalate(previous, previous, cause);
        let from = chosen.escalated_from?;
        Some(Map::from_iter([("model".to_string(), json!(chosen.model)), (ESCALATED_FIELD.to_string(), from.to_value())]))
    };
    raised().unwrap_or_default()
}

/// O campo do envio gravado quando nenhum hospedeiro foi reconhecido por
/// trás da rodada: é ele que separa o envio de agora, sem o par, do envio de
/// versão antiga, também sem o par.
const NO_HOST_PROCESS: &str = "no_host_process";

/// O campo do envio gravado junto do par quando o par é do hospedeiro
/// reconhecido: é ele que separa o par de agora do par do envio de versão
/// antiga feito no Codex, que guardava o processo da própria rodada.
const HOST_PAIR: &str = "host_pair";

/// Os campos que dizem, no envio, qual hospedeiro o mandou: o par
/// (`claude_pid`, `claude_started`) do hospedeiro reconhecido
/// ([`crate::commands::flow::stuck::sender_process`]), com a marca
/// [`HOST_PAIR`], ou, sem nenhum, a marca [`NO_HOST_PROCESS`]. O despacho do
/// agente novo de uma onda que voltou grava o hospedeiro dele por aqui
/// também. [`host_still_here`] lê de volta o que sai daqui.
pub(crate) fn sender_fields(sender: Option<(u32, u64)>) -> Map<String, Value> {
    let mut fields = Map::new();
    if let Some((pid, started)) = sender {
        fields.insert("claude_pid".into(), json!(pid));
        fields.insert("claude_started".into(), json!(started));
        fields.insert(HOST_PAIR.into(), json!(true));
    } else {
        fields.insert(NO_HOST_PROCESS.into(), json!(true));
    }
    fields
}

/// `true` quando o hospedeiro — o Claude Code ou o Codex — que mandou o envio
/// `sent` ainda está aberto: o par gravado (`claude_pid`, `claude_started`)
/// segue vivo. O envio sem o par nunca conta como fechado: sem hospedeiro
/// reconhecido, a rodada não tem como saber, e quem decide é a pausa que o
/// orquestrador manda. A exceção é o envio de versão antiga sem o par, que só
/// o Claude Code mandava: com o campo `host` dizendo `claude` e sem a marca
/// [`NO_HOST_PROCESS`], ele segue a regra de antes — fechado no Linux, aberto
/// fora dele. O envio de versão antiga feito no Codex, com o par e sem a
/// marca [`HOST_PAIR`], gravou o processo da própria rodada, que morre logo
/// depois: esse par não diz nada do hospedeiro, e o envio conta como aberto,
/// como o envio sem hospedeiro reconhecido.
fn host_still_here(log: &SpecLog, sent: u64) -> bool {
    let Some(event) = log.get(sent) else {
        return false;
    };
    match (event.int("claude_pid"), event.int("claude_started")) {
        #[allow(clippy::cast_possible_truncation)]
        (Some(pid), Some(started)) => {
            let round_pair = event.str_field("host") == Some("codex") && event.fields.get(HOST_PAIR).is_none();
            round_pair || crate::commands::flow::stuck::process_alive(pid as u32, started)
        }
        _ => {
            let older_claude_send = event.str_field("host") == Some("claude") && event.fields.get(NO_HOST_PROCESS).is_none();
            !older_claude_send || cfg!(not(target_os = "linux"))
        }
    }
}

/// As ondas em andamento, cada uma com o número do pedido dela: as com pedido
/// aberto ([`open_sends`]) cujo Claude Code ainda está aberto e que ainda não
/// voltaram ([`SpecLog::waves_returned`]). A onda que voltou e espera — o clique do
/// usuário na mudança de plano, ou a volta regravada que uma conferência
/// dela pediu — já tem o agente terminado: o envio aberto segue guardando a
/// vaga, os arquivos e a cópia dela, mas ela não está em andamento para
/// ninguém. A resposta da rodada, o bloco de retomada, a página e o pedido
/// de cada onda leem o andamento daqui, e só daqui.
pub(crate) fn waves_in_progress(log: &SpecLog) -> BTreeMap<u64, u64> {
    let returned = log.waves_returned();
    open_sends(log).into_iter().filter(|(n, sent)| !returned.contains(n) && host_still_here(log, *sent)).collect()
}

/// O campo da versão do envio que diz que o agente da onda começou: a
/// observação do hospedeiro o grava quando vê o agente ligado ao envio pela
/// primeira vez ([`record_launch`]). A versão nova do envio leva os campos da
/// anterior, menos o pedido ([`send_revision`]), e o envio novo da rodada (o
/// despacho e o reenvio) começa sem ele.
const AGENT_LAUNCHED: &str = "agent_launched";

/// Os campos da versão do envio que marca o agente da onda lançado
/// ([`AGENT_LAUNCHED`]); [`agent_launched`] lê de volta o que sai daqui.
pub(crate) fn launch_fields() -> Map<String, Value> {
    Map::from_iter([(AGENT_LAUNCHED.to_string(), json!(true))])
}

/// `true` quando o envio `sent` já tem o agente da onda lançado depois do
/// despacho ([`launch_fields`]).
pub(crate) fn agent_launched(log: &SpecLog, sent: u64) -> bool {
    log.get(sent).is_some_and(|event| event.fields.get(AGENT_LAUNCHED) == Some(&json!(true)))
}

/// `true` quando o envio `sent` da onda `wave` já tem agente: o lançamento
/// gravado nele ([`agent_launched`]), ou, quando essa gravação falhou, o
/// agente que a observação do hospedeiro ligou ao despacho dele. A onda com
/// agente não volta ao despacho nem aceita um segundo agente.
pub(crate) fn agent_known(root: &Path, spec: &str, log: &SpecLog, wave: u64, sent: u64) -> bool {
    agent_launched(log, sent) || crate::shared::host_observation::dispatch_has_agent(root, spec, wave, log.dispatch_position(sent))
}

/// Grava que o agente da onda `wave` da spec `spec`, de `root`, começou: o
/// envio dela que despachou a onda na posição `dispatch` ganha uma versão com
/// [`launch_fields`]. É o envio aberto ou, na onda que quem conduz a obra
/// reprovou, que já voltou e não tem pedido aberto, o que guarda a reprovação
/// ([`rejected_send`]), onde o despacho do agente novo gravou o Claude Code
/// dele. Só a prova de que o agente existe chama isto — a primeira chamada
/// dele ligada ao envio, ou o despacho nativo que terminou —, nunca o pedido
/// de despacho, que a permissão ainda pode negar. Ler, conferir e gravar
/// correm com a trava do arquivo de eventos presa, a mesma que a rodada pega
/// para soltar a onda do agente que não começou
/// ([`super::rejection::release_unstarted`]): o lançamento e a soltura não se
/// perdem um ao outro. O envio que já marca o lançamento, o de outro despacho
/// e o que um reenvio substituiu não gravam nada. `false` só quando a
/// gravação falha.
pub(crate) fn record_launch(root: &Path, spec: &str, wave: u64, dispatch: u64) -> bool {
    let Ok(path) = mustard_core::io::spec_events::spec_file(root, spec) else { return false };
    let written = mustard_core::io::spec_events::with_locked_writer(&path, |locked| {
        // A onda reprovada já voltou e não tem pedido aberto: o agente novo
        // dela marca o lançamento no envio que guarda a reprovação.
        let Some(sent) = open_sends(locked.log()).get(&wave).copied().or_else(|| rejected_send(locked.log(), wave)) else { return Some(()) };
        if locked.log().dispatch_position(sent) != dispatch || agent_launched(locked.log(), sent) {
            return Some(());
        }
        let draft = send_revision(locked.log(), wave, launch_fields())?;
        crate::commands::spec_events::write::record_locked(locked, root, spec, "send", draft, PhaseWriter::Binary).ok()?;
        Some(())
    });
    matches!(written, Ok(Some(Some(()))))
}

/// Os minutos sem sinal de vida a partir dos quais a onda conta como calada:
/// o mesmo prazo do aviso `wave-silent` da rodada.
pub(crate) const SILENT_AFTER_MINUTES: i64 = 40;

/// As ondas com agente trabalhando, cada uma com o número do pedido dela: as
/// em andamento ([`waves_in_progress`]) cujo agente foi lançado depois do
/// envio ([`agent_launched`]) e deu sinal de vida há menos de
/// [`SILENT_AFTER_MINUTES`] ([`silent_minutes`]). A onda que a rodada soltou e
/// ninguém lançou segura a vaga, e a rodada seguinte a devolve ao despacho,
/// mas o processo vivo do condutor não diz que um agente a trabalha; a calada
/// além do prazo também não. A reorganização da lista lê daqui quais tarefas
/// não pode regravar.
pub(crate) fn waves_with_live_agent(root: &Path, spec: &str, log: &SpecLog) -> BTreeMap<u64, u64> {
    waves_in_progress(log)
        .into_iter()
        .filter(|(wave, sent)| {
            agent_launched(log, *sent) && silent_minutes(root, spec, *wave, log, *sent).is_some_and(|minutes| minutes < SILENT_AFTER_MINUTES)
        })
        .collect()
}

/// As ondas órfãs, cada uma com o número do pedido dela: as com pedido aberto
/// ([`open_sends`]) cujo Claude Code já fechou. A rodada as reenvia com o
/// mesmo pedido de antes, na cópia que volta limpa ao commit atual. A onda
/// que já voltou ([`SpecLog::waves_returned`]) nunca é órfã: o agente dela terminou, e
/// a cópia guarda o que ele entregou até a rodada o assumir.
pub(crate) fn orphaned_waves(log: &SpecLog) -> BTreeMap<u64, u64> {
    let returned = log.waves_returned();
    open_sends(log).into_iter().filter(|(n, sent)| !returned.contains(n) && !host_still_here(log, *sent)).collect()
}

/// As ondas que esperam um agente novo, cada uma com o arquivo do trecho de
/// conserto dela: a rodada recusou a volta e gravou o trecho em disco
/// ([`super::fixes::fix_file`]), e o Claude Code que mandou a onda já fechou,
/// levando o agente dela. Só nesse caso a onda aceita outro agente, que
/// recebe o pedido e o trecho e trabalha na mesma cópia; com o Claude Code do
/// envio aberto, o conserto vai ao agente que fez a onda. A volta que quem
/// conduz a obra reprovou também espera um agente novo, com o Claude Code do
/// envio aberto ou fechado, até o despacho dele ([`rejected_unclaimed`]). O
/// despacho do agente novo grava uma versão do envio com o Claude Code dele,
/// e a onda sai daqui enquanto esse Claude Code está aberto. O agente novo
/// que nunca começou não a segura para sempre: a rodada tira o Claude Code
/// dele do envio ([`super::rejection::release_unstarted`]) quando nenhuma
/// chamada do agente gravou o lançamento ([`record_launch`]), e a onda volta
/// a esperar um agente. A entrega nova muda o nome do trecho, e a onda sai
/// daqui sozinha. O gancho do despacho e a recusa da rodada leem daqui, e só
/// daqui.
pub(crate) fn waves_awaiting_new_agent(root: &Path, spec: &str, log: &SpecLog) -> BTreeMap<u64, PathBuf> {
    let sends = log.last_by_wave("send");
    log.waves_returned()
        .into_iter()
        .filter(|n| sends.get(n).is_some_and(|sent| rejected_unclaimed(log, *n, *sent) || !host_still_here(log, *sent)))
        .filter_map(|n| super::fixes::fix_file(root, spec, log, n).filter(|file| file.is_file()).map(|file| (n, file)))
        .collect()
}

/// A volta pendente da onda `wave` é a que quem conduz a obra reprovou, e o
/// envio mais novo dela (`sent`) é o que gravou a reprovação, sem Claude Code
/// nenhum: a reprovação tirou a onda do agente que a fez, e nenhum agente novo
/// a tomou ainda. Quem reprova diz que o agente terminou, então o Claude Code
/// que mandou a onda, aberto ou fechado, não segura nada aqui; o despacho do
/// agente novo grava o Claude Code dele, e a regra de sempre volta a valer.
fn rejected_unclaimed(log: &SpecLog, wave: u64, sent: u64) -> bool {
    let unclaimed = log.get(sent).is_some_and(|event| event.int("claude_pid").is_none()) && !agent_launched(log, sent);
    unclaimed && rejected_send(log, wave) == Some(sent)
}

/// O envio mais novo da onda `wave` quando ele guarda a reprovação da volta
/// pendente dela: a onda espera um agente novo, ou o agente novo que a tomou.
pub(crate) fn rejected_send(log: &SpecLog, wave: u64) -> Option<u64> {
    let sent = *log.last_by_wave("send").get(&wave)?;
    super::rejection::rejection_of(log, wave).filter(|(back, _)| super::rejection::pending_return(log, wave) == Some(*back)).map(|_| sent)
}

/// Tira do rascunho de uma versão do envio o agente que ele tinha: o Claude
/// Code que o mandou e o lançamento ([`AGENT_LAUNCHED`]). A onda volta a
/// esperar um agente novo, que só a toma quando começa de fato.
pub(crate) fn without_agent(draft: &mut Map<String, Value>) {
    for field in ["claude_pid", "claude_started", AGENT_LAUNCHED] {
        draft.remove(field);
    }
}

/// As ondas que voltaram ([`SpecLog::waves_returned`]) sem pedir mudança de plano: o
/// trabalho delas está entregue e só espera a rodada assumi-lo. A que pede
/// mudança espera a resposta do usuário, e as tarefas dela seguem por fazer.
pub(crate) fn waves_returned_without_replan(log: &SpecLog) -> BTreeSet<u64> {
    log.returns_after_dispatch()
        .into_iter()
        .filter(|e| e.str_field("replan").is_none_or(|change| change.trim().is_empty()))
        .filter_map(SpecEvent::wave)
        .collect()
}

/// A onda `n` é de lote — o binário a formou a partir do backlog, com autor
/// binário — e não a combinação à mão do plano: só a de lote devolve tarefa
/// ao backlog quando cortada, porque só ela é o que o backlog empacota de novo.
pub(crate) fn backlog_wave(log: &SpecLog, n: u64) -> bool {
    log.visible().into_iter().any(|event| event.event_type == "wave" && event.wave() == Some(n) && event.str_field("author") == Some("binary"))
}

/// Minutos desde a última ação da onda `wave`: a mais nova entre a hora do
/// envio `sent` e a do arquivo de sinal de vida que
/// [`crate::hooks::observe::wave_alive_observer`] grava para a vaga que o
/// envio gravou. A vaga passa de uma onda para a seguinte, e o arquivo dela
/// pode guardar a hora da onda anterior: a hora do envio vale enquanto a onda
/// nova ainda não agiu. `None` sem nenhuma hora legível — a rodada não avisa
/// sem saber.
pub(crate) fn silent_minutes(root: &Path, spec: &str, wave: u64, log: &SpecLog, sent: u64) -> Option<i64> {
    let parse = |raw: &str| chrono::DateTime::parse_from_rfc3339(raw.trim()).ok();
    let send = log.get(sent);
    let copy = send
        .and_then(|event| event.str_field("copy").map(str::to_string))
        .or_else(|| mustard_core::io::wave_prompt::recorded_copy(log, wave).map(|copy| copy.path));
    let from_file = copy
        .filter(|copy| mustard_core::io::wave_prompt::is_slot_of(root, spec, copy))
        .and_then(|copy| Path::new(&copy).file_name().map(|name| name.to_string_lossy().into_owned()))
        .and_then(|slot| std::fs::read_to_string(crate::hooks::observe::wave_alive_observer::alive_path(root, spec, &slot)).ok())
        .and_then(|raw| parse(&raw));
    let from_send = send.and_then(|event| parse(event.at()));
    let at = match (from_file, from_send) {
        (Some(file), Some(send)) => file.max(send),
        (file, send) => file.or(send)?,
    };
    let now = chrono::Local::now().with_timezone(at.offset());
    Some((now - at).num_minutes())
}

/// As ondas do plano com o conserto pendente: a última revisão delas
/// reprovou, e nenhuma entrega chegou depois dessa reprovação. Só a entrega
/// resolve o conserto, com veredito novo ou sem ele: a aprovação final do
/// agente de teste dedicado responde pelo combinado inteiro e, quando não
/// aponta onda, é gravada sem onda nenhuma — não entra na revisão de onda
/// alguma e, por isso, não solta a onda reprovada. É a leitura única de
/// "onda com conserto pendente", que
/// `finished` (o fechamento), `waves_done` (a fila) e `wave_states` (o
/// estado da página) compartilham, para as três não discordarem de quando o
/// ciclo de conserto termina.
pub(crate) fn waves_pending_fix(log: &SpecLog) -> BTreeMap<u64, u64> {
    let delivered = log.last_by_wave("delivered");
    log.last_rejected().into_iter().filter(|(n, id)| delivered.get(n).is_none_or(|fix| fix < id)).collect()
}

/// As ondas entregues e aprovadas: têm entrega, não estão em andamento, e não
/// têm conserto pendente ([`waves_pending_fix`]). A onda entregue antes de a
/// rodada existir, sem pedido e sem veredito, está provada pelo código que
/// entrou. A rodada não pede revisão de onda nenhuma: a entrega já basta, e
/// só uma reprovação sem conserto ainda entregue devolve a onda para a fila.
pub(super) fn waves_done(log: &SpecLog, running: &BTreeMap<u64, u64>) -> BTreeSet<u64> {
    let pending = waves_pending_fix(log);
    log.delivered_waves().into_iter().filter(|n| !running.contains_key(n) && !pending.contains_key(n)).collect()
}

/// A primeira onda planejada que ainda não está entregue e aprovada, com as
/// em andamento em `running`. `None` quando todas estão.
pub(super) fn first_unfinished(log: &SpecLog, running: &BTreeMap<u64, u64>) -> Option<u64> {
    let done = waves_done(log, running);
    log.planned_waves().into_iter().find(|n| !done.contains(n))
}

/// A onda `n` depende de todas as outras que ainda não terminaram: as
/// dependências dela, diretas ou por outra onda, alcançam cada onda planejada
/// que não está entregue e aprovada. É a última onda da obra.
fn depends_on_all(n: u64, depends: &BTreeMap<u64, Vec<u64>>, done: &BTreeSet<u64>) -> bool {
    let reached = dependencies_of(n, depends);
    depends.keys().filter(|w| **w != n && !done.contains(w)).all(|w| reached.contains(w))
}

/// As ondas de que `n` depende, diretas ou por outra onda.
fn dependencies_of(n: u64, depends: &BTreeMap<u64, Vec<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = depends.get(&n).cloned().unwrap_or_default();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(depends.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// As ondas que voltam para a fila: têm o conserto pendente
/// ([`waves_pending_fix`]) e ainda não foram despachadas de novo desde a
/// reprovação — o pedido mais novo da onda é anterior a essa reprovação.
/// Depois que o conserto sai, a onda espera a entrega dele, e não é
/// despachada de novo pela mesma reprovação — mas continua com o conserto
/// pendente para quem lê [`waves_pending_fix`], como o fechamento, até a
/// entrega chegar. O pedido do conserto que o plano da onda deixou para
/// trás, por uma versão nova da onda ou de uma tarefa dela, conta como se
/// não existisse: a onda volta para a fila e sai com o pedido do plano
/// atual. A reprovação do fechamento que não cita arquivo da onda já saiu
/// antes desta leitura, na soltura que a rodada chama
/// ([`crate::commands::flow::common::stale_marks::release`]), e não traz a
/// onda de volta.
pub(crate) fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_dispatch_by_wave();
    let replanned = waves_replanned(log);
    waves_pending_fix(log).into_iter().filter(|(n, id)| replanned.contains(n) || last_send.get(n).is_none_or(|sent| sent < id)).map(|(n, _)| n).collect()
}

/// As ondas prontas para sair, na ordem do grafo de dependências que
/// [`wave_graph`] já leu (`graph`) — por nível ([`WaveGraph::level`]) e,
/// entre as do mesmo nível, por quantas ondas cada uma destrava, direta ou
/// por outra, da maior para a menor, e por último pelo número. `already_out`
/// são as ondas que já saíram da fila e `delivered` as que já entregaram,
/// que é o que solta as ondas dependentes delas. A onda que depende de todas
/// as outras só sai com as dependências entregues e aprovadas (`done`).
///
/// Uma dependência em círculo é recusada com os números do ciclo
/// (`graph.cycle`) — a mesma leitura que já trava a aprovação do plano
/// ([`crate::commands::flow::plan`]), não uma segunda conta à parte que
/// pudesse discordar dela.
///
/// Só as ondas de `depends` são candidatas: quem chama o monta com as ondas
/// do plano ([`SpecLog::planned_waves`]), sem a de lote que ficou sem tarefa.
fn ready_in_order(
    depends: &BTreeMap<u64, Vec<u64>>,
    graph: &WaveGraph,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    done: &BTreeSet<u64>,
) -> Result<Vec<u64>, Vec<u64>> {
    if !graph.cycle.is_empty() {
        return Err(graph.cycle.clone());
    }
    let candidates: BTreeSet<u64> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(n, on)| {
            let released = if depends_on_all(**n, depends, done) { done } else { delivered };
            on.iter().all(|d| released.contains(d))
        })
        .map(|(n, _)| *n)
        .collect();
    let deps: BTreeMap<u64, BTreeSet<u64>> = depends.iter().map(|(n, on)| (*n, on.iter().copied().collect())).collect();
    let unlocks = task_unlocks(&deps);
    let mut order: Vec<(u32, std::cmp::Reverse<usize>, u64)> =
        depends.keys().map(|&n| (graph.level.get(&n).copied().unwrap_or(0), std::cmp::Reverse(unlocks.get(&n).copied().unwrap_or(0)), n)).collect();
    order.sort_unstable();
    Ok(order.into_iter().map(|(_, _, n)| n).filter(|n| candidates.contains(n)).collect())
}

/// Os itens que o pedido de uma onda leva: os números de tudo que entrou
/// nele, com a escolha dos itens antes do envio (`choice`).
pub(super) fn sent_items(log: &SpecLog, wave: u64, choice: Option<&Choice>) -> Vec<u64> {
    dispatch_items(log, wave, choice).into_iter().map(|e| e.id).collect()
}

// ---------------------------------------------------------------------------
// O desempate de [`ready_in_order`] entre ondas do mesmo nível: quantas
// outras cada uma destrava
// ---------------------------------------------------------------------------

/// Quantas ondas cada uma destrava: o tamanho do fecho transitivo de quem
/// depende dela, direta ou por outra onda do backlog.
fn task_unlocks(deps: &BTreeMap<u64, BTreeSet<u64>>) -> BTreeMap<u64, usize> {
    let mut unlocks: BTreeMap<u64, usize> = deps.keys().map(|&n| (n, 0)).collect();
    for &m in deps.keys() {
        for on in task_transitive_deps(m, deps) {
            *unlocks.entry(on).or_insert(0) += 1;
        }
    }
    unlocks
}

/// As ondas de que `n` depende, diretas ou por outra, dentro de `deps`.
fn task_transitive_deps(n: u64, deps: &BTreeMap<u64, BTreeSet<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = deps.get(&n).cloned().unwrap_or_default().into_iter().collect();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(deps.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// Os arquivos que uma tarefa declara: cada item de `files` é o caminho, seja
/// como texto solto ou como `{"path": ...}`.
pub(crate) fn task_files(task: &SpecEvent) -> BTreeSet<String> {
    files_in(&task.fields)
}

/// Os arquivos que os campos `fields` de uma tarefa — a gravada ou a que ainda
/// vai ser — declaram em `files`, como [`task_files`] os lê.
pub(crate) fn files_in(fields: &Map<String, Value>) -> BTreeSet<String> {
    fields
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        .map(|path| path.trim().replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect()
}

/// A versão nova da tarefa `id`: os campos dela, tirando `v`, `id`, `code`,
/// `at`, `type` e `search`, com `replaces` apontando para ela, o autor
/// `binary` (a versão é da rodada, seja quem for que escreveu a anterior) e os
/// campos de `extra` somados por cima — a mesma forma de [`send_revision`],
/// para a tarefa em vez do envio. Quem passa `author` em `extra` vence.
///
/// Na tarefa, `replaces` aponta a versão mais nova do mesmo código, ainda que
/// a leitura mostre outra: numa spec antiga, a remoção de só a versão com a
/// onda devolvia à leitura a versão sem onda, e a versão nova sobre ela
/// traria de volta a tarefa removida. Apontada a mais nova, a gravação recusa
/// a versão nova da tarefa que saiu. O número é o desta leitura, e não o
/// código resolvido na hora de gravar: a versão que outra gravação fizer
/// depois desta leitura continua recusando esta como substituída, e o texto
/// dela não se perde. O evento sem código, e o que não é tarefa, vão pelo
/// número, como antes.
pub(crate) fn task_revision(log: &SpecLog, id: u64, extra: Map<String, Value>) -> Option<Map<String, Value>> {
    let event = log.get(id)?;
    let mut draft: Map<String, Value> = event
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let newest = (event.event_type == "task")
        .then(|| log.codes())
        .and_then(|codes| {
            let code = codes.get(&id)?;
            log.events.iter().filter(|e| codes.get(&e.id) == Some(code)).map(|e| e.id).max()
        })
        .unwrap_or(id);
    draft.insert("replaces".into(), json!(newest));
    draft.insert("author".into(), json!("binary"));
    for (key, value) in extra {
        draft.insert(key, value);
    }
    Some(draft)
}

/// As tarefas vigentes da spec como o motor do backlog as lê: cada uma com as
/// dependências resolvidas para a versão vigente, os arquivos que declara,
/// se já está feita — a onda dela está entre as entregues e aprovadas
/// (`done_waves`) —, se está presa (`blocked_by` ou dependência quebrada) e se
/// traz a marca de prioridade.
pub(crate) fn backlog_population(log: &SpecLog, done_waves: &BTreeSet<u64>) -> Vec<crate::shared::dag::BacklogTask<u64>> {
    let codes = log.codes();
    let refs = TaskRefs::new(log, &codes);
    // A tarefa com dependência que não aponta tarefa nenhuma da spec não sai:
    // a rodada a segura com o motivo, e as que dependem dela esperam junto.
    let broken = crate::commands::spec_events::task_dependents::broken_dependencies(log);
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .map(|task| {
            let depends_on: BTreeSet<u64> = refs.depends_on(task).collect();
            let done = task.wave().is_some_and(|w| done_waves.contains(&w));
            // A marca de prioridade é lida do evento, como o usuário a deu: o
            // Jev não a julga.
            let priority = task.str_field("priority").is_some_and(|reason| !reason.trim().is_empty());
            let blocked = task.str_field("blocked_by").is_some() || broken.contains_key(&task.id);
            crate::shared::dag::BacklogTask { id: task.id, depends_on, files: task_files(task), done, blocked, priority }
        })
        .collect()
}

/// O que está no backlog: a tarefa sem onda nenhuma, e também a que carrega
/// um número de onda que nunca virou evento de onda — a spec antiga,
/// numerada à mão — desde que essa onda não tenha entregue. A tarefa de
/// onda com evento próprio, gravada pelo plano ou por um lote anterior,
/// nunca está aqui: quem a despacha é [`next_waves`]. A onda já entregue ou
/// aprovada fica como história, e a tarefa dela nunca volta para o backlog.
///
/// É a leitura única do backlog: a formação do lote ([`dispatch_backlog`]), o
/// próximo passo da rodada e o fechamento leem daqui, para os três não
/// discordarem de quando o backlog está vazio.
pub(crate) fn backlog_left(log: &SpecLog) -> BTreeSet<u64> {
    backlog_left_without(log, &BTreeSet::new())
}

/// [`backlog_left`] como ficaria sem as ondas de lote `undone`: as tarefas
/// delas contam como soltas, porque a montagem do backlog as desfaz antes de
/// formar as ondas da rodada.
pub(super) fn backlog_left_without(log: &SpecLog, undone: &BTreeSet<u64>) -> BTreeSet<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let planned = log.planned_waves();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .filter(|t| match t.wave() {
            None => true,
            Some(w) => !done_waves.contains(&w) && (!planned.contains(&w) || undone.contains(&w)),
        })
        .map(|t| t.id)
        .collect()
}

/// As tarefas ainda por entregar: a do backlog ([`backlog_left`]) e a de onda
/// planejada que ainda não está entregue e aprovada. Não contam as das ondas
/// em `returning` — a que volta agora e as que o conserto dela fecha —,
/// porque a entrega as fecha agora.
///
/// É a leitura única de "tarefa ainda não entregue": a rodada a consulta
/// antes de criar tarefa para um item combinado que a volta não cumpriu.
pub(crate) fn tasks_not_delivered(log: &SpecLog, returning: &BTreeSet<u64>) -> BTreeSet<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let planned = log.planned_waves();
    let in_open_wave = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "task")
        .filter(|t| t.wave().is_some_and(|w| planned.contains(&w) && !done_waves.contains(&w)))
        .map(|t| t.id);
    backlog_left(log).into_iter().chain(in_open_wave).filter(|id| log.get(*id).and_then(SpecEvent::wave).is_none_or(|w| !returning.contains(&w))).collect()
}

/// Uma tarefa ainda por entregar, como a leitura do backlog a mostra: o
/// número da versão vigente, os arquivos que ela declara e as tarefas ainda
/// por entregar de que ela depende, cada uma pela versão vigente.
pub(crate) struct LeftTask {
    pub id: u64,
    pub files: BTreeSet<String>,
    pub depends_on: BTreeSet<u64>,
}

/// As tarefas ainda por entregar ([`tasks_not_delivered`]), em ordem de
/// número, cada uma com os arquivos dela e as dependências que também ainda
/// faltam: a dependência já entregue não prende mais nada e fica de fora.
pub(crate) fn tasks_left(log: &SpecLog) -> Vec<LeftTask> {
    let left = tasks_not_delivered(log, &BTreeSet::new());
    let codes = log.codes();
    let refs = TaskRefs::new(log, &codes);
    left.iter()
        .filter_map(|id| log.get(*id))
        .map(|task| LeftTask {
            id: task.id,
            files: task_files(task),
            depends_on: refs.depends_on(task).filter(|dependency| left.contains(dependency)).collect(),
        })
        .collect()
}

/// `true` quando a tarefa `task` não cobre item nenhum: `covers` ausente ou
/// vazio. A onda leva como critérios os itens que as tarefas dela cobrem, e a
/// gravação da onda recusa a que ficaria sem nenhum; por isso a tarefa que
/// não cobre nada não forma onda nem pega carona na de outra.
pub(super) fn covers_nothing(task: &SpecEvent) -> bool {
    task.ints("covers").is_empty()
}

/// As tarefas do backlog ([`backlog_left`]) que não cobrem item nenhum
/// ([`covers_nothing`]): ficam no backlog sem onda, e a rodada as nomeia
/// num aviso até uma versão delas trazer o `covers`.
pub(crate) fn backlog_uncovered(log: &SpecLog) -> BTreeSet<u64> {
    backlog_left(log).into_iter().filter(|id| log.get(*id).is_some_and(covers_nothing)).collect()
}

/// As tarefas do backlog ([`backlog_left`]) que já estão prontas — todas as
/// dependências entregues ou aprovadas —, na ordem em que o motor do backlog
/// as empacota. Vazia quando o backlog está vazio ou quando toda tarefa dele
/// ainda espera uma dependência. A tarefa que não cobre nada
/// ([`covers_nothing`]) nunca está pronta: ela segura as que dependem dela, e
/// a rodada que só tem ela no backlog a nomeia entre as presas, em vez de
/// mandar rodar de novo.
pub(crate) fn backlog_ready(log: &SpecLog) -> Vec<u64> {
    backlog_ready_without(log, &BTreeSet::new())
}

/// `true` quando a obra `spec`, como está gravada agora em `root`, tem onda a
/// despachar: a órfã, que sai de novo, ou, com vaga livre, a onda pronta do
/// plano ou a tarefa pronta do backlog. A spec que não se lê, e o plano com
/// dependência em círculo, contam como tendo: na dúvida, quem pergunta segue
/// como se a onda fosse sair.
pub(crate) fn dispatch_waiting(root: &Path, spec: &str) -> bool {
    use mustard_core::io::spec_events as store;
    let Some(log) = store::spec_file(root, spec).ok().and_then(|path| store::read(&path).ok().flatten()) else {
        return true;
    };
    if !orphaned_waves(&log).is_empty() {
        return true;
    }
    let vacancies = super::vacancy::vacancies(root, spec, &log);
    if vacancies.free() == 0 {
        return false;
    }
    if !backlog_ready(&log).is_empty() {
        return true;
    }
    let (stuck, evaluating) = (super::stops::waves_stuck(&log), super::skill_hold::waves_waiting(root, spec, &log));
    next_waves(&log, &vacancies, (&stuck, &evaluating)).map_or(true, |going| !going.is_empty())
}

/// [`backlog_ready`] como ficaria sem as ondas de lote `undone`
/// ([`backlog_left_without`]).
pub(super) fn backlog_ready_without(log: &SpecLog, undone: &BTreeSet<u64>) -> Vec<u64> {
    let running = waves_in_progress(log);
    let done_waves = waves_done(log, &running);
    let left = backlog_left_without(log, undone);
    crate::shared::dag::ready_tasks(&backlog_population(log, &done_waves))
        .into_iter()
        .filter(|id| left.contains(id))
        .filter(|id| !log.get(*id).is_some_and(covers_nothing))
        .collect()
}

/// O estado de cada onda do plano ([`SpecLog::planned_waves`]), pela mesma
/// leitura que decide o que a rodada despacha: em andamento com o agente
/// começado, com o pedido aberto e nenhum agente nele — o agente nunca
/// começou ([`agent_launched`]) ou o Claude Code do envio fechou
/// ([`orphaned_waves`]) —, reprovada na última revisão, entregue e aprovada —
/// a rodada não pede revisão de onda nenhuma, então a entrega já vale como
/// aprovada —, com o commit dela ou sem, ou por fazer, a que o backlog formou
/// e ainda não saiu inclusive. A onda de lote que ficou sem tarefa saiu do
/// plano e não aparece, nem com a entrega gravada em nome dela. O painel, a
/// barra de status e a retomada contam as ondas por aqui, e só por aqui.
pub(crate) fn wave_states(log: &SpecLog) -> mustard_core::view::document::WaveStates {
    use mustard_core::view::document::WaveState;
    let running = waves_in_progress(log);
    let orphaned = orphaned_waves(log);
    let rejected = waves_pending_fix(log);
    let done = waves_done(log, &running);
    let committed = waves_committed(log);
    log.planned_waves()
        .into_iter()
        .map(|n| {
            let state = if let Some(sent) = running.get(&n) {
                if agent_launched(log, *sent) { WaveState::Running } else { WaveState::NoAgent }
            } else if orphaned.contains_key(&n) {
                WaveState::NoAgent
            } else if rejected.contains_key(&n) {
                WaveState::Rejected
            } else if done.contains(&n) {
                if committed.contains(&n) { WaveState::Committed } else { WaveState::Approved }
            } else {
                WaveState::Todo
            };
            (n, state)
        })
        .collect()
}

/// As ondas com commit gravado depois do envio que as despachou por último:
/// o commit de uma tentativa anterior não vale para a entrega de agora. A
/// onda entregue antes de a rodada existir, sem envio, vale por qualquer
/// commit que a nomeie.
pub(crate) fn waves_committed(log: &SpecLog) -> BTreeSet<u64> {
    let dispatches = log.last_dispatch_by_wave();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves").into_iter().map(move |n| (n, e.id)))
        .filter(|(n, id)| dispatches.get(n).is_none_or(|start| id > start))
        .map(|(n, _)| n)
        .collect()
}

/// O cenário da volta com tarefa não feita, que os testes da entrega também
/// usam.
#[cfg(test)]
pub(super) use tests::{SIX_FILES, UndoneReturn, backlog_project, backlog_task_on, return_with_an_undone_task, seed_running, spec_now, task_now, wave_order};

#[cfg(test)]
mod tests {
    use std::process::Command;

    use std::path::PathBuf;

    use mustard_core::domain::spec_state::PhaseWriter;
    use mustard_core::io::spec_events as store;
    use mustard_core::io::wave_prompt::{shown, slot_path};
    use mustard_core::platform::i18n::{Locale, translate};
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::backlog::dispatch_backlog;
    use crate::commands::flow::round::tests::*;
    use crate::commands::spec_events::write::record;

    mod rejected_wave_new_agent;
    mod removal_after_delivery;

    /// A cópia gravada no envio da onda `wave`.
    fn sent_copy(root: &Path, wave: u64) -> String {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        mustard_core::io::wave_prompt::recorded_copy(&log, wave).unwrap_or_else(|| panic!("wave {wave}")).path
    }

    /// A volta que quem conduz a obra reprovou espera um agente novo até o
    /// despacho dele, também onde o envio sem Claude Code conta como aberto:
    /// o envio que gravou a reprovação, sem Claude Code nenhum, solta a onda;
    /// o despacho do agente novo, com o Claude Code dele, a segura de novo; e
    /// a reprovação de uma volta que não é a pendente não solta nada.
    #[test]
    fn a_rejected_return_waits_for_a_new_agent_until_one_is_dispatched() {
        let send = r#""type":"send","wave":1,"role":"wave","text":"pedido","lines":1,"chars":6,"mustard":"t""#;
        let at = r#""at":"2026-01-10T21:54:28-03:00""#;
        let rejected = r#""rejected":{"delivered":2,"reason":"faltou o teste"}"#;
        let log_with = |more: &[&str]| {
            let mut lines = vec![
                format!(r#"{{"v":1,"id":1,{at},{send},"claude_pid":1,"claude_started":1}}"#),
                format!(r#"{{"v":1,"id":2,{at},"type":"delivered","wave":1,"text":"saiu","returned":true}}"#),
            ];
            lines.extend(more.iter().map(|line| (*line).to_string()));
            mustard_core::domain::spec_events::parse_log(&lines.join("\n"))
        };
        let rejection = format!(r#"{{"v":1,"id":3,{at},{send},"replaces":1,{rejected}}}"#);
        assert!(rejected_unclaimed(&log_with(&[&rejection]), 1, 3), "the rejection frees the wave");
        let taken = format!(r#"{{"v":1,"id":4,{at},{send},"replaces":3,{rejected},"claude_pid":1,"claude_started":1}}"#);
        assert!(!rejected_unclaimed(&log_with(&[&rejection, &taken]), 1, 4), "the new agent holds it");
        let other = rejection.replace(r#""delivered":2"#, r#""delivered":9"#);
        assert!(!rejected_unclaimed(&log_with(&[&other]), 1, 3), "the rejection of another return frees nothing");
    }

    /// Duas ondas sem dependência e sem arquivo em comum saem juntas, cada
    /// uma na sua vaga — a pasta fixa `a`, `b`… sob a pasta da spec —,
    /// posta no commit atual. O pedido de cada uma traz a cópia e nunca uma
    /// pasta de compilação, em projeto Node ou Rust: o que a cópia compila
    /// fica dentro dela. O teto de compilações do projeto limita quantas
    /// saem.
    #[test]
    fn two_waves_without_a_shared_file_go_out_together_each_in_its_own_slot_and_the_cap_holds() {
        for kind in ["npm", "cargo"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
            std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
            mapped(root, kind);

            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "sem arquivo em comum, as duas saem juntas: {out}");
            let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
            for (at, wave) in [1_u64, 2].iter().enumerate() {
                let copy = sent_copy(root, *wave);
                let expected = mustard_core::io::wave_prompt::slot_path(root, "x", at);
                assert_eq!(copy, mustard_core::io::wave_prompt::shown(&expected), "{kind}: {out}");
                assert!(expected.join(".git").is_file(), "the copy of wave {wave} is a linked checkout");
                assert_eq!(std::fs::read_to_string(expected.join("src/a.rs")).unwrap(), "fn one() {}\n");
                let copy_head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&expected).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&copy_head.stdout).trim(), head, "the copy stands on the current commit");
                let prompt = &request_at(&out, at);
                assert!(prompt.contains(&format!("`{copy}`")), "{prompt}");
                for word in ["CARGO_TARGET_DIR", "target/copias", "pasta de compilação"] {
                    assert!(!prompt.contains(word), "{kind}: no build folder in the request ({word}): {prompt}");
                }
            }
        }

        // Com o teto do projeto em 1, só uma onda sai por rodada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        let out = round(root, "x", None);
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
    }

    /// A rodada abre a cópia da onda fora da pasta do projeto, na pasta das
    /// cópias dele — o nome do projeto e um código curto que não muda —, sob
    /// a pasta da spec, e o envio grava esse caminho, que o pedido cita. O
    /// revisor final usa a vaga da última onda; nada nasce em
    /// `.claude/worktrees`. A entrega que cita o arquivo pelo caminho absoluto
    /// da cópia gravada no envio volta relativa ao repositório, mesmo quando
    /// essa cópia não é a que a pasta das cópias daria hoje, como a da onda
    /// que saiu antes de a pasta mudar.
    #[test]
    fn a_wave_copy_is_born_outside_the_project_folder() {
        use mustard_core::io::wave_prompt::{copies_dir, final_copy_path};

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");

        let project = std::fs::canonicalize(root).unwrap();
        let copies = copies_dir(root);
        assert!(!copies.starts_with(root) && !copies.starts_with(&project), "{copies:?}");
        let folder = copies.file_name().unwrap().to_string_lossy().into_owned();
        let (name, code) = folder.rsplit_once('-').unwrap();
        assert_eq!(name, project.file_name().unwrap().to_string_lossy(), "{folder}");
        assert!(code.len() == 8 && code.chars().all(|c| c.is_ascii_hexdigit()), "{folder}");
        assert_eq!(copies_dir(&project), copies, "the same project always gives the same folder");

        let copy = sent_copy(root, 1);
        assert_eq!(copy, shown(&copies.join("x").join("a")), "{out}");
        assert!(Path::new(&copy).join(".git").is_file(), "the copy is a linked checkout");
        let prompt = &request_at(&out, 0);
        assert!(prompt.contains(&format!("`{copy}`")), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(final_copy_path(root, "x", &log), copies.join("x").join("a"), "the review uses the wave's slot");
        assert!(!root.join(".claude").join("worktrees").exists(), "nothing is born inside the project");

        // Um envio mais novo da onda grava a cópia noutro lugar.
        let elsewhere = tempdir().unwrap();
        let moved = shown(&elsewhere.path().join("x").join("a"));
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.wave() == Some(1) && e.event_type == "send").unwrap();
        let draft = json!({
            "wave": 1, "role": "wave", "text": log.send_text(sent.id).unwrap_or_default(),
            "lines": log.send_field(sent.id, "lines").and_then(Value::as_u64).unwrap_or(1), "chars": log.send_field(sent.id, "chars").and_then(Value::as_u64).unwrap_or(1),
            "items": log.send_field(sent.id, "items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0", "author": "binary", "copy": moved,
        });
        store::write(&path, "send", draft.as_object().cloned().unwrap(), &[]).unwrap();

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// mudou\n").unwrap();
        let body = json!({"wave": 1, "text": "Saiu.", "files": [format!("{moved}/src/a.rs")], "commit": "a entrega 1 saiu"});
        let wrote = returned(root, body);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let log = store::read(&path).unwrap().unwrap();
        let back = log.events.iter().rfind(|e| e.event_type == "delivered").unwrap();
        assert_eq!(back.fields.get("files"), Some(&json!(["src/a.rs"])), "{wrote}");
    }

    /// Duas ondas sem dependência entre si, com o mesmo arquivo declarado,
    /// saem juntas: cada uma trabalha na própria cópia. Sem `maxCompilingWaves`
    /// no projeto, o teto é de duas; a terceira espera a vaga.
    #[test]
    fn waves_with_a_shared_file_leave_together_up_to_the_default_cap_of_two() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "o arquivo em comum não segura a onda 2, e a 3 espera a vaga: {out}");
    }

    /// A cópia da onda órfã — pedido aberto sem o processo que mandou —
    /// volta suja ao commit atual sozinha, na mesma rodada em que a rodada
    /// nota a órfã, sem que ninguém peça o reenvio primeiro: quem falhou no
    /// meio do trabalho não deixou uma retomada em curso. A limpeza leva o
    /// arquivo novo e o mudado, mas não a pasta de compilação que o git
    /// ignora: a compilação feita ali fica para quem usar a vaga depois. A
    /// órfã segura a vaga dela: a onda que sai ao lado vai para outra.
    #[cfg(target_os = "linux")]
    #[test]
    fn orphan_copy_goes_back_clean_to_the_current_commit_without_waiting_for_a_resend() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        std::fs::write(root.join(".git").join("info").join("exclude"), "target/\n").unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");

        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        std::fs::write(copy.join("src/a.rs"), "fn retomada() {}\n").unwrap();
        std::fs::write(copy.join("src/novo.rs"), "fn novo() {}\n").unwrap();
        let build = copy.join("target").join("debug").join("compilado.o");
        std::fs::create_dir_all(build.parent().unwrap()).unwrap();
        std::fs::write(&build, "compilado").unwrap();
        assert_ne!(git_text(copy, &["status", "--porcelain"]), "", "a cópia precisa estar suja antes da rodada");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.wave() == Some(1) && e.event_type == "send").unwrap();
        let mut draft = json!({
            "wave": 1, "role": "wave", "text": log.send_text(sent.id).unwrap_or_default(),
            "lines": log.send_field(sent.id, "lines").and_then(Value::as_u64).unwrap_or(1), "chars": log.send_field(sent.id, "chars").and_then(Value::as_u64).unwrap_or(1),
            "items": log.send_field(sent.id, "items").cloned().unwrap_or_else(|| json!([])),
            "mustard": "0", "author": "binary", "copy": sent.str_field("copy").unwrap_or_default(),
        });
        drop(log);

        let (dead_pid, dead_started) = closed_process();
        draft["claude_pid"] = json!(dead_pid);
        draft["claude_started"] = json!(dead_started);
        store::write_at(&path, "send", draft.as_object().cloned().unwrap(), &[], &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()).unwrap();

        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let second = round(root, "x", None);

        assert_eq!(git_text(copy, &["status", "--porcelain"]), "", "a cópia órfã volta limpa sem pedir reenvio");
        let head = git_text(root, &["rev-parse", "HEAD"]);
        assert_eq!(git_text(copy, &["rev-parse", "HEAD"]), head, "a cópia volta ao commit atual: {head}");
        assert_eq!(std::fs::read_to_string(&build).unwrap(), "compilado", "a compilação ignorada fica na vaga");
        assert_eq!(waves_in(&second, "dispatch"), vec![2, 1], "a entrega 2 sai, e a órfã é reenviada: {second}");
        let next = shown(&mustard_core::io::wave_prompt::slot_path(root, "x", 1));
        assert_eq!(sent_copy(root, 2), next, "a órfã segura a vaga a, e a onda 2 vai para a b: {second}");
        assert_eq!(sent_copy(root, 1), copy_path, "a órfã volta na mesma vaga: {second}");
    }

    /// O envio que saiu sem hospedeiro reconhecido por trás da rodada — um
    /// hospedeiro fora da lista, um script, a linha de comando — nunca dá a
    /// onda como órfã: a rodada seguinte não a reenvia, e ela segue em
    /// andamento até quem conduz pausá-la. O envio de versão antiga sem o par
    /// segue a regra de antes só quando diz que veio do Claude Code: esse é
    /// órfão e sai de novo; o do Codex, não. O projeto é em TypeScript.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_send_without_a_recognized_host_is_never_orphaned() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/app.ts"], &[]), (2, &["src/b.ts"], &[]), (3, &["src/c.ts"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2, 3], "{first}");

        let path = store::spec_file(root, "x").unwrap();
        let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        for (wave, host, without_host) in [(1, "claude", true), (2, "claude", false), (3, "codex", false)] {
            let log = store::read(&path).unwrap().unwrap();
            let fields = if without_host { sender_fields(None) } else { Map::new() };
            let mut draft = send_revision(&log, wave, fields).expect("the send of the wave");
            draft.remove("claude_pid");
            draft.remove("claude_started");
            if !without_host {
                draft.remove(NO_HOST_PROCESS);
            }
            draft.insert("host".into(), json!(host));
            store::write_at(&path, "send", draft, &[], &at).unwrap();
        }

        let second = round(root, "x", None);
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "só a órfã de versão antiga do Claude Code sai de novo: {second}");
        let running = waves_in_progress(&store::read(&path).unwrap().unwrap());
        assert!(running.contains_key(&1) && running.contains_key(&3), "{running:?}: {second}");
    }

    /// O envio de versão antiga feito no Codex guardava, no lugar do
    /// hospedeiro, o processo da própria rodada, que morre logo depois: com
    /// esse par morto e sem a marca do par do hospedeiro, a onda não é dada
    /// como órfã, não sai de novo e segue em andamento. O par morto com a
    /// marca, de um Codex reconhecido que fechou, e o par morto do envio
    /// antigo do Claude Code seguem órfãos e saem de novo. O projeto é em
    /// TypeScript.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_older_codex_send_with_a_closed_pair_is_never_orphaned() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/app.ts"], &[]), (2, &["src/b.ts"], &[]), (3, &["src/c.ts"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2, 3], "{first}");

        let path = store::spec_file(root, "x").unwrap();
        let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        for (wave, host, marked) in [(1, "codex", false), (2, "codex", true), (3, "claude", false)] {
            let log = store::read(&path).unwrap().unwrap();
            let mut draft = send_revision(&log, wave, sender_fields(Some(closed_process()))).expect("the send of the wave");
            draft.remove(NO_HOST_PROCESS);
            if !marked {
                draft.remove(HOST_PAIR);
            }
            draft.insert("host".into(), json!(host));
            store::write_at(&path, "send", draft, &[], &at).unwrap();
        }

        let second = round(root, "x", None);
        assert_eq!(waves_in(&second, "dispatch"), vec![2, 3], "o envio antigo do Codex com o par morto não sai de novo: {second}");
        let running = waves_in_progress(&store::read(&path).unwrap().unwrap());
        assert!(running.contains_key(&1), "{running:?}: {second}");
    }

    /// A onda que já entregou não é despachada de novo, mesmo sem pedido
    /// nenhum: a onda entregue antes de a rodada existir não tem pedido, e
    /// mandá-la sair seria mandar refazer um trabalho já feito.
    #[test]
    fn a_wave_that_already_delivered_does_not_go_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}));

        let out = round(root, "x", None);
        let waves: Vec<u64> = out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![2], "a onda 1 já tem entrega: {out}");
    }

    /// A onda cuja última revisão reprovou volta a ser despachada, e uma vez
    /// só: depois que o conserto sai, a mesma reprovação não a manda de novo.
    /// Entregue o conserto, ele para de estar em andamento e de sair de novo
    /// — falta o agente de teste dedicado, no fechamento, dizer se ficou
    /// certo, e a rodada não pede revisão nenhuma dele.
    #[test]
    fn a_rejected_wave_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o teste")));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu: {quiet}");

        let back = round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(back["dispatch"], json!([]), "o conserto não sai de novo: {back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");
    }

    /// A onda reprovada cujo conserto já saiu e que ganha uma tarefa nova
    /// depois desse envio volta para a fila, como se o envio não existisse: a
    /// rodada seguinte a despacha de novo, com a tarefa nova no pedido, e ela
    /// fica em andamento com o envio novo; a rodada depois dessa não a solta
    /// outra vez.
    #[test]
    fn a_rejected_wave_that_gets_a_new_task_after_its_fix_went_out_goes_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let fix = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o teste")));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "{fix}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let task = write(
            root,
            "x",
            "task",
            json!({"wave": 1, "text": "Tarefa nova da onda 1.",
            "files": [{"path": "src/b.rs"}], "depends_on": [], "origin": said}),
        );
        let task_code = task["code"].as_str().unwrap_or_else(|| panic!("{task}")).to_string();

        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "the replanned fix goes out again: {again}");
        let prompt = &request_at(&again, 0);
        assert!(prompt.lines().any(|l| l.starts_with(&format!("   - {task_code} — "))), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log.visible().into_iter().rfind(|e| e.event_type == "send").map(|e| codes[&e.id].clone()).unwrap();
        let running = again["running"].as_array().cloned().unwrap_or_default();
        assert_eq!(running.len(), 1, "{again}");
        assert_eq!(running[0]["wave"], json!(1), "{again}");
        assert_eq!(running[0]["send"], json!(newest), "{again}");

        let quiet = round(root, "x", None);
        assert_eq!(waves_in(&quiet, "dispatch"), Vec::<u64>::new(), "{quiet}");
        assert_eq!(waves_in(&quiet, "running"), vec![1], "{quiet}");
    }

    /// O reenvio de uma onda replanejada usa a cópia que já existe, de um
    /// envio anterior: limpa, ela vai para o commit atual do checkout
    /// principal, mesmo que um commit alheio à rodada tenha avançado o
    /// checkout desde a criação dela; com mudança sem commitar, como a de uma
    /// retomada em andamento, ela fica no commit que estava.
    #[test]
    fn a_clean_copy_moves_to_the_current_commit_before_a_send() {
        for dirty in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            let first = round(root, "x", None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
            let copy_path = sent_copy(root, 1);
            let copy = Path::new(&copy_path);
            let old_head = git_text(copy, &["rev-parse", "HEAD"]);

            // Um commit alheio à rodada, de outro trabalho no checkout
            // principal, avança o HEAD sem tocar a onda 1, que segue sem
            // entrega e com a cópia dela ainda no disco.
            std::fs::write(root.join("src/b.rs"), "fn dois() {}\n").unwrap();
            git_at(root, &["add", "-A"]);
            git_at(root, &["commit", "-q", "-m", "outro trabalho"]);
            let new_head = git_text(root, &["rev-parse", "HEAD"]);
            assert_ne!(old_head, new_head, "o commit alheio precisa mudar o HEAD");

            if dirty {
                std::fs::write(copy.join("src/a.rs"), "fn retomada() {}\n").unwrap();
            }

            // A onda 1 ganha versão nova e volta para a fila com a cópia que
            // já existe, sem entrega nem reprovação nenhuma antes disso.
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let planned = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).unwrap();
            let mut fields = planned.fields.clone();
            for key in ["v", "id", "code", "at", "search", "type", "author"] {
                fields.remove(key);
            }
            let mut wave = Value::Object(fields);
            wave["done_when"] = json!("A suíte passa e o teste novo também.");
            wave["replaces"] = json!(planned.id);
            write(root, "x", "wave", wave);

            let again = round(root, "x", None);
            assert_eq!(waves_in(&again, "dispatch"), vec![1], "a onda replanejada volta a sair: {again}");
            assert_eq!(sent_copy(root, 1), copy_path, "a replanejada volta à vaga que gravou: {again}");
            let copy_head = git_text(copy, &["rev-parse", "HEAD"]);
            if dirty {
                assert_eq!(copy_head, old_head, "a cópia com mudança fica como está: {copy_head}");
                assert_eq!(std::fs::read_to_string(copy.join("src/a.rs")).unwrap(), "fn retomada() {}\n");
            } else {
                assert_eq!(copy_head, new_head, "a cópia limpa vai para o commit atual: {copy_head}");
            }
        }
    }

    /// A data de mudança do arquivo `path`.
    fn modified(path: &Path) -> std::time::SystemTime {
        std::fs::metadata(path).and_then(|meta| meta.modified()).unwrap_or_else(|err| panic!("{path:?}: {err}"))
    }

    /// Põe no arquivo `path` uma data antiga e redonda, que o teste reconhece
    /// depois.
    fn age(path: &Path) -> std::time::SystemTime {
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(old).unwrap();
        old
    }

    /// A vaga passa de uma onda para a seguinte sem nascer de novo: a onda 2
    /// sai na vaga que a 1 deixou, posta no commit novo, e só o arquivo que
    /// o commit da onda 1 mudou ganha data nova — o outro guarda a dele, e a
    /// compilação que o git ignora fica. O pedido da onda 2 diz que a vaga é
    /// reaproveitada e lista o arquivo que mudou desde o último uso dela.
    #[test]
    fn two_preparations_of_the_same_slot_keep_the_date_of_an_unchanged_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        std::fs::write(root.join(".git").join("info").join("exclude"), "target/\n").unwrap();
        std::fs::write(root.join("mustard.json"), br#"{"prepareCommand":"git --version"}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let slot = slot_path(root, "x", 0);
        assert_eq!(sent_copy(root, 1), shown(&slot), "{first}");
        let fresh = translate("prompt.execution.prepared_native", Locale::PtBr);
        assert!(request_at(&first, 0).contains(fresh), "native preparation is already complete");
        let untouched = age(&slot.join("src/b.rs"));
        age(&slot.join("src/a.rs"));
        let build = slot.join("target").join("compilado.o");
        std::fs::create_dir_all(build.parent().unwrap()).unwrap();
        std::fs::write(&build, "compilado").unwrap();

        let second = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "{second}");
        assert_eq!(sent_copy(root, 2), shown(&slot), "a onda 2 usa a vaga que a 1 deixou: {second}");
        assert_eq!(git_text(&slot, &["rev-parse", "HEAD"]), git_text(root, &["rev-parse", "HEAD"]));
        assert!(std::fs::read_to_string(slot.join("src/a.rs")).unwrap().contains("A soma saiu."), "{second}");
        assert_ne!(modified(&slot.join("src/a.rs")), untouched, "o arquivo mudado ganha data nova");
        assert_eq!(modified(&slot.join("src/b.rs")), untouched, "o arquivo que não mudou guarda a data");
        assert_eq!(std::fs::read_to_string(&build).unwrap(), "compilado", "a compilação ignorada fica na vaga");
        let prompt = &request_at(&second, 0);
        assert!(prompt.contains(fresh) && prompt.contains("`src/a.rs`"), "{prompt}");
        assert!(!prompt.contains("`src/b.rs`."), "só o que mudou entra na lista: {prompt}");
    }

    /// A onda replanejada, cujo último envio ficou sem entrega, volta à vaga
    /// que esse envio gravou, mesmo com outra vaga livre antes dela na
    /// ordem.
    #[test]
    fn a_replanned_wave_goes_back_to_the_slot_it_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let second_slot = shown(&slot_path(root, "x", 1));
        assert_eq!(sent_copy(root, 2), second_slot, "{first}");

        round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        assert_eq!(sent_copy(root, 2), second_slot, "a vaga a está livre, e a onda 2 volta à b: {again}");
    }

    /// A vaga cuja pasta existe sem ser cópia do git — sobra de um processo
    /// que caiu no meio — nasce de novo, sem o que havia nela; a vaga cuja
    /// pasta sumiu com o registro do git ainda de pé também, sem que o
    /// registro velho trave a cópia nova.
    #[test]
    fn a_slot_folder_that_is_not_a_git_copy_or_a_stale_registration_is_rebuilt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let slot = slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("lixo")).unwrap();
        std::fs::write(slot.join("lixo").join("velho.txt"), "sobra").unwrap();

        let first = round(root, "x", None);
        assert_eq!(sent_copy(root, 1), shown(&slot), "{first}");
        assert!(slot.join(".git").is_file() && !slot.join("lixo").exists(), "a pasta solta vira cópia: {first}");

        std::fs::remove_dir_all(&slot).unwrap();
        assert!(git_text(root, &["worktree", "list", "--porcelain"]).contains(&shown(&slot)), "o registro ficou");
        let second = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&second, "dispatch"), vec![2], "o registro velho não trava a cópia: {second}");
        assert_eq!(sent_copy(root, 2), shown(&slot), "{second}");
        assert!(slot.join(".git").is_file(), "{second}");
        assert_eq!(git_text(&slot, &["rev-parse", "HEAD"]), git_text(root, &["rev-parse", "HEAD"]));
    }

    /// As refs que guardam código de cópia, no repositório principal.
    fn folder_refs(root: &Path) -> Vec<String> {
        git_text(root, &["for-each-ref", "--format=%(refname)", "refs/mustard/kept"]).lines().map(str::to_string).collect()
    }

    /// Uma pasta de vaga que o git já não conhece: sem cópia ligada, com o
    /// trabalho de quem a usou — um arquivo mudado (`src/a.rs`), um arquivo
    /// igual ao do commit (`src/b.rs`) e dois arquivos novos — e um `.git`
    /// que aponta para um registro que sumiu.
    fn broken_slot_with_code(slot: &Path) {
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::create_dir_all(slot.join("lixo")).unwrap();
        std::fs::write(slot.join("src/a.rs"), "fn one() {}\n// o meio do trabalho\n").unwrap();
        std::fs::write(slot.join("src/b.rs"), "fn one() {}\n").unwrap();
        std::fs::write(slot.join("src/novo.rs"), "fn novo() {}\n").unwrap();
        std::fs::write(slot.join("lixo/velho.txt"), "sobra\n").unwrap();
        std::fs::write(slot.join(".git"), "gitdir: /sumiu/.git/worktrees/a\n").unwrap();
    }

    /// A pasta da vaga que o git esqueceu e tem código dentro não é apagada
    /// sem antes ir para uma ref do repositório principal: a ref traz cada
    /// arquivo novo ou mudado com o conteúdo de antes, o aviso lista só esses
    /// e diz como trazê-los de volta, o arquivo igual ao do commit não conta,
    /// o índice do principal não muda, e a onda sai numa cópia nova e limpa.
    #[test]
    fn a_broken_slot_folder_keeps_its_code_under_a_ref_before_it_is_rebuilt() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[])]);
        let slot = slot_path(root, "x", 0);
        broken_slot_with_code(&slot);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(sent_copy(root, 1), shown(&slot), "{out}");
        let refs = folder_refs(root);
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert!(refs[0].starts_with("refs/mustard/kept/x/a-"), "a ref diz a vaga: {refs:?}");
        let show = |file: &str| git_text(root, &["show", &format!("{}:{file}", refs[0])]);
        assert_eq!(show("src/novo.rs"), "fn novo() {}");
        assert_eq!(show("lixo/velho.txt"), "sobra");
        assert_eq!(show("src/a.rs"), "fn one() {}\n// o meio do trabalho");
        let kept = warning_of(&out, "code-kept");
        assert_eq!(kept["ref"], json!(refs[0]), "{out}");
        assert_eq!(kept["files"], json!(["lixo/velho.txt", "src/a.rs", "src/novo.rs"]), "{out}");
        let hint = translate("round.code_kept_slot", Locale::PtBr).replace("{copy}", &shown(&slot)).replace("{ref}", &refs[0]);
        assert_eq!(kept["hint"], json!(hint), "{kept}");
        assert!(slot.join(".git").is_file() && !slot.join("src/novo.rs").exists(), "a cópia nova sai limpa: {out}");
        assert_eq!(git_text(&slot, &["status", "--porcelain"]), "", "{out}");
        assert_eq!(git_text(root, &["ls-files", "src/novo.rs", "lixo"]), "", "o índice do principal não muda");
        let stray: Vec<String> = std::fs::read_dir(root.join(".git"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("mustard-kept-index"))
            .collect();
        assert!(stray.is_empty(), "o índice temporário não fica: {stray:?}");
    }

    /// A pasta da vaga que o git esqueceu e não tem arquivo nenhum — só
    /// pastas vazias e o `.git` solto — é apagada como sempre: nada é
    /// guardado e nenhum aviso de código sai.
    #[test]
    fn a_broken_slot_folder_with_no_files_is_rebuilt_without_keeping_anything() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let slot = slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("lixo/fundo")).unwrap();
        std::fs::write(slot.join(".git"), "gitdir: /sumiu/.git/worktrees/a\n").unwrap();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(folder_refs(root), Vec::<String>::new(), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warned.iter().all(|w| w["reason"] != json!("code-kept")), "{out}");
        assert!(slot.join(".git").is_file() && !slot.join("lixo").exists(), "a pasta virou cópia: {out}");
    }

    /// A pasta da vaga que o git esqueceu e cujo código não pôde ser
    /// guardado não é apagada: o arquivo fica onde estava, a onda não sai, e o
    /// aviso diz que a cópia não foi criada.
    #[test]
    fn a_broken_slot_folder_whose_code_could_not_be_kept_stays_and_the_wave_does_not_go_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let slot = slot_path(root, "x", 0);
        broken_slot_with_code(&slot);
        // Uma ref no lugar da pasta das refs da spec: o git não cria as de dentro.
        let head = git_text(root, &["rev-parse", "HEAD"]);
        git_at(root, &["update-ref", "refs/mustard/kept/x", &head]);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "{out}");
        let failed = warning_of(&out, "copy-not-created");
        assert_eq!(failed["wave"], json!(1), "{out}");
        assert_eq!(std::fs::read_to_string(slot.join("src/novo.rs")).unwrap(), "fn novo() {}\n", "{out}");
        assert_eq!(std::fs::read_to_string(slot.join("lixo/velho.txt")).unwrap(), "sobra\n", "{out}");
        assert_eq!(folder_refs(root), vec!["refs/mustard/kept/x".to_string()], "nada foi guardado: {out}");
    }

    /// A vaga que uma onda usou e o git esqueceu guarda o código no nome
    /// da onda: o dono que quem prepara a cópia passa vai para a ref e para
    /// o que foi guardado.
    #[test]
    fn a_broken_folder_is_kept_under_the_owner_the_caller_names() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[])]);
        let slot = slot_path(root, "x", 0);
        broken_slot_with_code(&slot);
        let head = git_text(root, &["rev-parse", "HEAD"]);

        let owner = super::super::keep::Keeping { label: "x/1-7".to_string(), wave: Some(1) };
        let prepared = crate::commands::flow::round::ensure_copy(root, &slot, &head, &owner).unwrap();
        assert_eq!(prepared.kept.len(), 1);
        let kept = &prepared.kept[0];
        assert_eq!(kept.wave, Some(1));
        assert!(kept.refname.starts_with("refs/mustard/kept/x/1-7-"), "{}", kept.refname);
        assert_eq!(kept.files, ["lixo/velho.txt", "src/a.rs", "src/novo.rs"]);
        assert_eq!(kept.copy, shown(&slot));
        assert!(slot.join(".git").is_file() && !slot.join("src/novo.rs").exists());
    }

    /// Quem tira as cópias da obra (fechamento, descarte, limpeza) também
    /// guarda antes o código da pasta que o git esqueceu: a ref tem o
    /// arquivo, e só então a pasta sai. Com a guarda falhando, a pasta fica e
    /// o motivo vai em `unkept`.
    #[test]
    fn removing_a_broken_slot_folder_keeps_its_code_first_and_leaves_it_when_it_cannot() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[])]);
        let slot = slot_path(root, "x", 0);
        broken_slot_with_code(&slot);
        let head = git_text(root, &["rev-parse", "HEAD"]);
        git_at(root, &["update-ref", "refs/mustard/kept/x", &head]);

        let blocked = crate::commands::flow::round::remove_spec_copies(root, "x", None);
        assert_eq!(blocked.unkept.len(), 1, "{blocked:?}");
        assert!(slot.join("src/novo.rs").exists(), "a pasta fica quando não pôde guardar");

        git_at(root, &["update-ref", "-d", "refs/mustard/kept/x"]);
        let removal = crate::commands::flow::round::remove_spec_copies(root, "x", None);
        assert!(removal.unkept.is_empty() && removal.left.is_empty(), "{removal:?}");
        assert_eq!(removal.kept.len(), 1, "{removal:?}");
        let refs = folder_refs(root);
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert!(refs[0].starts_with("refs/mustard/kept/x/a-"), "{refs:?}");
        assert_eq!(git_text(root, &["show", &format!("{}:src/novo.rs", refs[0])]), "fn novo() {}");
        assert!(!slot.exists(), "só depois de guardar a pasta sai");
    }

    /// Os avisos de arquivo local que não chegou à cópia, na resposta da
    /// rodada: o item da lista de cada um.
    fn local_files_missing(out: &Value) -> Vec<String> {
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        warnings.iter().filter(|w| w["reason"] == json!("local-file-missing")).map(|w| w["file"].as_str().unwrap_or_default().to_string()).collect()
    }

    /// A cópia nova da onda recebe cada arquivo da lista de arquivos locais
    /// do projeto (`localFiles`) no mesmo caminho, como arquivo comum e com o
    /// mesmo conteúdo, nunca como link: mudar o da cópia não muda o do
    /// repositório principal. O arquivo que falta no principal, o caminho com
    /// `..` e o absoluto não chegam, viram aviso cada um, e a cópia sai do
    /// mesmo jeito — sem escrever nada fora dela. A cópia limpa reaproveitada,
    /// depois da limpeza que apaga o que o git ignora, recebe os arquivos de
    /// novo, com o conteúdo de agora. Com a lista vazia ou ausente, nada é
    /// copiado e nada é avisado.
    #[test]
    fn a_new_copy_gets_the_local_files_as_copies_never_links() {
        use mustard_core::io::wave_prompt::copies_dir;

        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\nconfig/local.json\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/local.json"), "{\"porta\":1}\n").unwrap();
        let outside = dir.path().join("fora.env");
        std::fs::write(&outside, "FORA=1\n").unwrap();
        let absolute = shown(&dir.path().join("absoluto.env"));
        std::fs::write(&absolute, "ABSOLUTO=1\n").unwrap();
        let listed = json!([".env", "config/local.json", "falta.env", "../fora.env", absolute]);
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": listed }).to_string()).unwrap();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a cópia sai mesmo com o arquivo que falta: {out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        for (file, content) in [(".env", "SEGREDO=1\n"), ("config/local.json", "{\"porta\":1}\n")] {
            let kind = std::fs::symlink_metadata(copy.join(file)).unwrap().file_type();
            assert!(kind.is_file() && !kind.is_symlink(), "{file} chega como arquivo comum");
            assert_eq!(std::fs::read_to_string(copy.join(file)).unwrap(), content, "{file}");
        }
        std::fs::write(copy.join(".env"), "MUDOU=1\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), "SEGREDO=1\n", "a cópia não escreve no principal");
        assert!(!copies_dir(root).join("fora.env").exists(), "o item com .. não escreve fora da cópia");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "FORA=1\n");
        assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "ABSOLUTO=1\n", "o item absoluto não toca o arquivo");
        assert_eq!(local_files_missing(&out), ["falta.env", "../fora.env", absolute.as_str()], "{out}");
        let hints = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = hints.iter().find(|w| w["file"] == json!("falta.env")).unwrap()["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`falta.env`") && hint.contains(&copy_path), "{hint}");
        assert_eq!(git_text(copy, &["status", "--porcelain", "--untracked-files=all"]), "", "a cópia segue limpa");

        // A cópia limpa, como a limpeza da cópia órfã a deixa, perde o que o
        // git ignora; a onda sai de novo com a mesma cópia, que recebe os
        // arquivos com o conteúdo de agora.
        git_at(copy, &["clean", "-fdx"]);
        assert!(!copy.join(".env").exists());
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        replan(root, 1);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert_eq!(sent_copy(root, 1), copy_path, "a mesma cópia: {again}");
        let kind = std::fs::symlink_metadata(copy.join(".env")).unwrap().file_type();
        assert!(kind.is_file() && !kind.is_symlink(), "{again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
        assert_eq!(std::fs::read_to_string(copy.join("config/local.json")).unwrap(), "{\"porta\":1}\n");

        // Lista vazia ou ausente: a cópia sai sem nenhum arquivo local.
        for config in [json!({ "localFiles": [] }), json!({})] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root).unwrap();
            std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1], "{config}: {out}");
            let copy = sent_copy(root, 1);
            assert!(!Path::new(&copy).join(".env").exists(), "{config}: nada é copiado");
            assert!(local_files_missing(&out).is_empty(), "{config}: {out}");
        }
    }

    /// A cópia reaproveitada com um link no lugar do arquivo local recebe um
    /// arquivo comum, com o conteúdo de agora da pasta principal: o link sai
    /// antes da cópia, e o alvo dele fica como estava.
    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_local_file_gives_way_to_a_plain_copy() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": [".env"] }).to_string()).unwrap();
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);

        let target = dir.path().join("alvo.env");
        std::fs::write(&target, "ALVO=1\n").unwrap();
        std::fs::remove_file(copy.join(".env")).unwrap();
        std::os::unix::fs::symlink(&target, copy.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();
        replan(root, 1);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert_eq!(sent_copy(root, 1), copy_path, "a mesma cópia: {again}");
        let kind = std::fs::symlink_metadata(copy.join(".env")).unwrap().file_type();
        assert!(kind.is_file() && !kind.is_symlink(), "o link saiu: {again}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "{again}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ALVO=1\n", "o alvo do link fica como estava");
        assert!(local_files_missing(&again).is_empty(), "{again}");
    }

    /// O arquivo local que já está igual na vaga não é escrito de novo: a
    /// onda seguinte na mesma vaga o encontra com a data de antes. O que
    /// mudou na pasta principal chega com o conteúdo novo.
    #[test]
    fn an_identical_local_file_keeps_its_date_in_the_slot() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\nlocal.json\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("local.json"), "{}\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": [".env", "local.json"] }).to_string()).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let copy = PathBuf::from(sent_copy(root, 1));
        let kept = age(&copy.join(".env"));
        age(&copy.join("local.json"));
        std::fs::write(root.join("local.json"), "{\"porta\":2}\n").unwrap();

        let second = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(sent_copy(root, 2), shown(&copy), "{second}");
        assert_eq!(modified(&copy.join(".env")), kept, "o arquivo local igual guarda a data: {second}");
        assert_eq!(std::fs::read_to_string(copy.join("local.json")).unwrap(), "{\"porta\":2}\n");
        assert_ne!(modified(&copy.join("local.json")), kept, "o que mudou chega de novo");
    }

    /// Um arquivo versionado posto à mão na lista de arquivos locais não é
    /// copiado: a cópia fica com a versão do commit, mesmo com a pasta
    /// principal mudada, e a rodada o devolve na lista dos que não foram
    /// copiados. O arquivo da mesma lista que o git ignora chega.
    #[test]
    fn a_versioned_file_in_the_local_list_keeps_the_commit_version_in_the_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(root.join("config/app.json"), "{\"versao\":\"commit\"}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("config/app.json"), "{\"versao\":\"principal\"}\n").unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        let listed = json!({ "localFiles": [".env", "config/app.json"] });
        std::fs::write(root.join("mustard.json"), listed.to_string()).unwrap();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let copy_path = sent_copy(root, 1);
        let copy = Path::new(&copy_path);
        let app = std::fs::read_to_string(copy.join("config/app.json")).unwrap();
        assert_eq!(app, "{\"versao\":\"commit\"}\n", "a versão do commit fica intacta: {out}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=1\n", "{out}");
        assert_eq!(local_files_missing(&out), ["config/app.json"], "{out}");
        assert_eq!(git_text(copy, &["status", "--porcelain", "--untracked-files=all"]), "", "a cópia segue limpa");
    }

    /// As cópias que um teste da rodada cria moram fora da pasta temporária
    /// dele, e saem quando ele termina — também quando ele falha —, e o git
    /// do projeto deixa de listá-las.
    #[test]
    fn the_copies_a_test_makes_leave_when_it_ends_even_when_it_fails() {
        use mustard_core::io::wave_prompt::copies_dir;

        for fails in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let inside = root.clone();
            let ended = std::thread::spawn(move || {
                approved(&inside, "x", &[(1, &["src/a.rs"], &[])]);
                let out = round(&inside, "x", None);
                let copy = sent_copy(&inside, 1);
                assert!(Path::new(&copy).join(".git").is_file(), "{out}");
                assert!(!fails, "o teste falhou depois de criar a cópia");
            })
            .join();
            assert_eq!(ended.is_err(), fails);
            assert!(!copies_dir(&root).exists(), "fails={fails}: a pasta das cópias saiu");
            let listed = git_text(&root, &["worktree", "list", "--porcelain"]);
            assert_eq!(listed.matches("worktree ").count(), 1, "fails={fails}: só o checkout principal: {listed}");
        }
    }

    /// O estado de cada onda que a página mostra acompanha a rodada: por
    /// fazer antes de sair, em andamento depois do pedido, aprovada assim que
    /// entrega — a rodada não pede revisão nenhuma —, o que já solta a onda
    /// seguinte, reprovada por quem julgar (o agente de teste dedicado, no
    /// fechamento), em andamento de novo com o conserto e aprovada no fim.
    #[test]
    fn the_wave_states_follow_the_round() {
        use mustard_core::view::document::WaveState::{Committed, Rejected, Running, Todo};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let events = root.join(".claude/spec/x/spec.ndjson");
        let states = || {
            let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
            wave_states(&log).into_iter().collect::<Vec<_>>()
        };
        assert_eq!(states(), [(1, Todo), (2, Todo)], "nothing went out yet: every wave of the plan is to do");
        round(root, "x", None);
        assert_eq!(states(), [(1, Running), (2, Todo)]);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(states(), [(1, Committed), (2, Running)], "the delivery already frees the next wave");

        // A reprovação, que só o agente de teste dedicado grava, no
        // fechamento.
        let rejected = json!({"author": "review", "final": true, "wave": 1, "result": "rejected", "text": "faltou o teste"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Rejected), (2, Running)]);
        round(root, "x", None);
        assert_eq!(states(), [(1, Running), (2, Running)], "the fix went out");
        round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        let approval = json!({"author": "review", "final": true, "wave": 1, "result": "approved", "text": "pronto"});
        mustard_core::io::spec_events::write(&events, "verdict", approval.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Committed), (2, Running)]);
    }

    /// O estado das ondas lista só as do plano: a onda de lote que o backlog
    /// formou e ainda não saiu vem como por fazer, e a onda de lote que ficou
    /// sem tarefa não vem, nem com uma entrega gravada em nome dela.
    #[test]
    fn the_wave_states_list_only_the_planned_waves() {
        use mustard_core::view::document::WaveState::Todo;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let events = root.join(".claude/spec/x/spec.ndjson");
        let raw = |kind: &str, fields: Value| {
            mustard_core::io::spec_events::write(&events, kind, fields.as_object().cloned().unwrap(), &[]).unwrap();
        };
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
        let batch = |n: u64| {
            json!({"author": "binary", "n": n, "text": format!("Lote {n}."), "criteria": [crit],
                "done_when": "A suíte passa."})
        };
        // O lote 2 ficou sem tarefa, e uma entrega foi gravada em nome dele.
        raw("wave", batch(2));
        raw("delivered", json!({"author": "wave", "wave": 2, "text": "Saiu.", "files": ["src/b.rs"]}));
        // O lote 3 tem tarefa e ainda não saiu.
        raw("wave", batch(3));
        raw(
            "task",
            json!({"author": "binary", "wave": 3, "text": "Tarefa do lote.", "files": [{"path": "src/c.rs"}],
            "depends_on": []}),
        );
        let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
        assert_eq!(
            wave_states(&log).into_iter().collect::<Vec<_>>(),
            [(1, Todo), (3, Todo)],
            "the empty batch stays out, and the formed batch that did not go out is to do"
        );
    }

    /// A onda em andamento — com pedido e sem entrega depois dele — ocupa uma
    /// vaga do limite e a cópia fixa dela: a rodada não passa do limite
    /// contando as que já saíram, e a onda que sai no lugar da que voltou
    /// fica com a vaga livre, e não com a da que segue em andamento. A cópia
    /// da que voltou continua no disco depois do commit, pronta para a
    /// seguinte.
    /// O pedido anterior ao replanejamento da onda não conta como andamento.
    /// A resposta lista as ondas em andamento com o código do pedido de cada
    /// uma.
    #[test]
    fn a_wave_in_flight_holds_its_slot_and_a_send_before_the_replan_does_not_count() {
        // A onda 2 volta; a 3 sai no lugar dela, enquanto a 1 segue.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let held = sent_copy(root, 1);
        let freed = sent_copy(root, 2);
        let out = round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(Path::new(&freed).join(".git").is_file(), "a cópia fica depois do commit da onda: {out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "a vaga da 2 ficou livre: {out}");
        assert_eq!(sent_copy(root, 3), freed, "a 3 usa a vaga que a 2 deixou, e não a da 1 ({held})");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let sent = |wave: u64| log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(wave)).map(|e| codes[&e.id].clone()).unwrap();
        let running: Vec<Value> = out["running"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            running.iter().map(|r| (r["wave"].clone(), r["send"].clone())).collect::<Vec<_>>(),
            vec![(json!(1), json!(sent(1))), (json!(3), json!(sent(3)))],
            "{out}"
        );

        // Duas ondas em andamento enchem o limite de duas.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let full = round(root, "x", None);
        assert_eq!(waves_in(&full, "dispatch"), Vec::<u64>::new(), "as duas vagas estão ocupadas: {full}");
        assert_eq!(waves_in(&full, "running"), vec![1, 2], "{full}");

        // Replanejada depois do pedido, a onda 2 deixa de estar em andamento:
        // ela sai de novo, e a vaga dela não fica presa ao pedido velho.
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let running = again["running"].as_array().cloned().unwrap_or_default();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(2)).map(|e| codes[&e.id].clone()).next_back().unwrap();
        assert_eq!(running[1]["wave"], json!(2), "o pedido novo é o que conta: {again}");
        assert_eq!(running[1]["send"], json!(newest), "o pedido novo é o que conta: {again}");

        // A onda 1 entregou antes de a rodada existir, e um pedido saiu para
        // ela depois, sem reprovação no meio: ela não está em andamento, e as
        // duas vagas ficam para as outras.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "Saiu antes da rodada.", "files": ["src/a.rs"]}));
        seed_send(root, 1);
        let free = round(root, "x", None);
        assert_eq!(waves_in(&free, "dispatch"), vec![2, 3], "{free}");
        assert_eq!(waves_in(&free, "running"), vec![2, 3], "a onda 1 não está em andamento: {free}");
    }

    /// A onda que depende de todas as outras só sai depois de elas estarem
    /// aprovadas, não só entregues — a rodada não pede revisão nenhuma, então
    /// a entrega já vale como aprovação, menos para a onda que voltou
    /// reprovada: essa segura a que depende de todas até o conserto voltar. A
    /// que depende de uma parte sai com a entrega, como antes.
    #[test]
    fn the_wave_that_depends_on_all_the_others_waits_for_their_approvals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        // As duas entregam: sem revisão nenhuma da rodada, a entrega já basta
        // para soltar a 3.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let out = round(root, "x", Some(&both));
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "as duas entregas já soltam a 3: {out}");

        // A onda 2 depende só da 1, e a 3 não passa por ela: a 2 sai com a
        // entrega da 1.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 3]);
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a entrega basta para quem não depende de todas: {out}");

        // A onda 2 entrega e é reprovada antes de a 1 entregar: a 3, que
        // depende das duas, não sai enquanto a 2 não voltar aprovada, mesmo
        // com a 1 pronta.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        let events = root.join(".claude/spec/x/spec.ndjson");
        let rejected = json!({"author": "review", "final": true, "wave": 2, "result": "rejected", "text": "faltou algo"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a reprovada volta antes da que depende de todas: {out}");
        assert_eq!(waves_in(&out, "running"), vec![2], "a 3 ainda espera a 2 aprovada: {out}");
    }

    /// As linhas dos itens combinados que o pedido da onda 1 leva por padrão,
    /// sem Jev: o que a tarefa atende sai sob ela, e o resto sai em "O que
    /// obedecer", cada um em uma linha só, com o tipo, o código e o título.
    const CHOSEN_ITEMS_BY_DEFAULT: &str = "- Regra MSTD-RULE-0001 — Vale sempre: a tabela nova tem chave.\n\
        - Regra MSTD-RULE-0002 — Vale sempre: a spec vira um PR só.\n\
        - Decisão MSTD-DEC-0003 — Da onda um: a coluna é texto.\n";

    /// A linha da tarefa da onda 1 que atende uma das regras do projeto todo.
    const ATTENDED_ITEM: &str = "#### Atende: regra MSTD-RULE-0003 — Vale sempre: a tabela nova tem índice.";

    /// A spec aprovada da escolha dos itens: uma onda, com a tarefa que faz
    /// uma das regras do projeto todo, duas regras do projeto todo que ela não
    /// faz, dois itens sem dono e uma decisão da onda. Devolve o número de
    /// cada item, pelo código.
    fn with_items_to_judge(root: &Path) -> BTreeMap<String, u64> {
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let rule = |text: &str| {
                id_of(&write(
                    root,
                    "x",
                    "rule",
                    json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said}),
                ))
            };
            rule("Vale sempre: a tabela nova tem chave.");
            rule("Vale sempre: a spec vira um PR só.");
            let done = rule("Vale sempre: a tabela nova tem índice.");
            for (text, extra) in [
                ("Sem dono: a tabela nasce vazia.", json!({"keys": ["tabela"]})),
                ("Sem dono: o download não muda.", json!({"keys": ["índice"]})),
                ("Da onda um: a coluna é texto.", json!({"waves": [1]})),
            ] {
                let mut body = json!({"title": text, "text": text, "keys": ["k"], "why": "w", "origin": said});
                body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                id_of(&write(root, "x", "decision", body));
            }
            write(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": "Criar o índice da tabela.", "files": [{"path": "src/a.rs"}],
                "depends_on": [], "covers": [done], "origin": said}),
            );
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.codes().into_iter().map(|(id, code)| (code, id)).collect()
    }

    /// A rodada que vai soltar uma onda com regras do projeto todo e itens sem
    /// ligação com ela não espera ninguém: a onda sai na mesma rodada, sem
    /// relatório, sem pedir a escolha ao orquestrador e sem agente nenhum. Sem
    /// Jev, o pedido leva o padrão: o item do projeto todo vai, o item sem
    /// ligação com a onda fica fora, e o de que a onda é dona vai. O envio não
    /// grava escolha nenhuma, porque ninguém julgou, e o gancho monta o mesmo
    /// pedido.
    #[test]
    fn a_wave_with_items_to_judge_leaves_in_the_same_round_with_the_default_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        // O mapa já foi lido, como a instalação o deixa: a rodada não espera
        // a passada, e o pedido montado de novo lê o mesmo mapa.
        super::super::commit::refresh_map_if_stale(root, &super::super::answer::scan_mine);

        let out = round(root, "x", None);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("analysis").is_none(), "no choice is asked of the conductor: {out}");
        assert!(!out["next"].as_str().unwrap_or_default().contains("ANALYSIS"), "{out}");
        let prompt = request_at(&out, 0);
        let reach = request_and_blocks_at(root, "x", &out, 0);
        assert!(prompt.contains(CHOSEN_ITEMS_BY_DEFAULT), "{prompt}");
        assert!(reach.lines().any(|l| l == ATTENDED_ITEM), "{reach}");
        for code in ["MSTD-DEC-0001", "MSTD-DEC-0002"] {
            assert!(!reach.contains(code), "{code} has no link to the wave and stays out: {reach}");
        }
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave().is_some()).collect();
        assert_eq!(sent.len(), 1, "{out}");
        assert_eq!(log.send_text(sent[0].id), Some(prompt.as_str()));
        assert!(log.send_field(sent[0].id, "analysis").is_none(), "nobody judged: {:?}", sent[0].fields);
        let items: Vec<u64> = log.whole_fields(sent[0])["items"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
        for code in ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0003", "MSTD-DEC-0003"] {
            assert!(items.contains(&ids[code]), "{code}: {items:?}");
        }
        let running = waves_in_progress(&log).into_keys().collect();
        let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
        let rebuilt = mustard_core::io::wave_prompt::prompts(root, "x", &log, Locale::PtBr, &flight, &log.planned_waves());
        assert_eq!(rebuilt.iter().find(|p| p.wave == 1).map(|p| p.text.as_str()), Some(prompt.as_str()), "building it again gives the same request");
    }

    /// As chamadas `wave items` que o programa observou na spec `x`.
    fn item_calls(root: &Path) -> Vec<Map<String, Value>> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        crate::commands::spec_events::conversation::observed(root, "x", &log)
            .into_iter()
            .filter(|e| e.event_type == "call" && e.str_field("command") == Some("wave items"))
            .map(|call| call.fields)
            .collect()
    }

    /// O Jev, numa chamada por onda, tira do pedido a regra do projeto todo
    /// só com a chance abaixo de 0,2 e põe o item sem ligação só com a chance
    /// a partir de 0,85, e a onda sai na mesma rodada. A chamada leva as tarefas
    /// da onda e só os itens que o pedido não leva nem tira por conta própria:
    /// nem o que a tarefa faz, nem o de que a onda é dona. O envio grava o
    /// que entrou e o que saiu, cada um com a chance, e a chamada fica gravada
    /// com os tokens, o custo e o modelo.
    #[test]
    fn the_jev_judges_the_items_in_one_call_per_wave_and_the_send_records_what_came_in_and_went_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let spy = std::sync::Arc::clone(&seen);
        let by_id = ids.clone();
        let _jev = crate::commands::flow::round::item_choice::fake::answering(move |board| {
            spy.lock().unwrap().push(board.clone());
            let chance = |code: &str| by_id[code];
            BTreeMap::from([
                (chance("MSTD-RULE-0001"), 0.2),
                (chance("MSTD-RULE-0002"), 0.19),
                (chance("MSTD-DEC-0001"), 0.85),
                (chance("MSTD-DEC-0002"), 0.84),
                (chance("MSTD-RULE-0003"), 0.0),
            ])
        });

        let out = round(root, "x", None);

        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("analysis").is_none(), "{out}");
        let boards = seen.lock().unwrap().clone();
        assert_eq!(boards.len(), 1, "one call for the wave");
        let asked: Vec<u64> = boards[0].items.iter().map(|item| item.id).collect();
        let mut candidates = vec![ids["MSTD-RULE-0001"], ids["MSTD-RULE-0002"], ids["MSTD-DEC-0001"], ids["MSTD-DEC-0002"]];
        candidates.sort_unstable();
        assert_eq!(asked, candidates, "only what the request neither carries nor drops by itself");
        assert!(boards[0].tasks.iter().any(|task| task.text == "Criar o índice da tabela."), "the tasks of the wave go in the state");
        let prompt = request_and_blocks_at(root, "x", &out, 0);
        for code in ["MSTD-RULE-0001", "MSTD-DEC-0001", "MSTD-DEC-0003", "MSTD-RULE-0003"] {
            assert!(prompt.contains(code), "{code} goes in whatever the chance: {prompt}");
        }
        for code in ["MSTD-RULE-0002", "MSTD-DEC-0002"] {
            assert!(!prompt.contains(code), "{code}: {prompt}");
        }
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(
            log.whole_fields(sent)["analysis"],
            json!({
                "judged": candidates,
                "removed": [{"item": ids["MSTD-RULE-0002"], "why": "Jev p=0.19"}],
                "added": [{"item": ids["MSTD-DEC-0001"], "why": "Jev p=0.85"}],
                "judged_lessons": [], "removed_lessons": [],
            })
        );
        let calls = item_calls(root);
        assert_eq!(calls.len(), 1, "{calls:?}");
        let call = &calls[0];
        assert_eq!(
            (call["filter"].clone(), call["tokens"].clone(), call["cost_micro_usd"].clone(), call["model"].clone()),
            (json!("jev"), json!(1000), json!(42), json!("jev-1.13.0")),
            "{call:?}"
        );
        assert_eq!((call["candidates"].clone(), call["returned"].clone()), (json!(4), json!(2)), "{call:?}");
    }

    /// A spec aprovada do item ligado aos arquivos da onda: uma onda que mexe
    /// em `src/a.rs`, com duas regras que dizem esse arquivo, uma regra que a
    /// tarefa faz, uma decisão de que a onda é dona e uma regra de toda onda,
    /// as três últimas também com o arquivo. Devolve o número de cada item,
    /// pelo código.
    fn with_items_of_the_files_of_the_wave(root: &Path) -> BTreeMap<String, u64> {
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let files = json!({"files": ["src/a.rs"]});
            let rule = |text: &str, extra: Value| {
                let mut body = json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": files, "origin": said});
                body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                id_of(&write(root, "x", "rule", body))
            };
            rule("Dos arquivos: a tabela tem chave.", json!({}));
            rule("Dos arquivos: a coluna tem tipo.", json!({}));
            let done = rule("Dos arquivos e da tarefa: o índice é único.", json!({}));
            rule("Dos arquivos e de toda onda: a tabela tem dono.", json!({"every_wave": true}));
            write(
                root,
                "x",
                "decision",
                json!({"title": "Dos arquivos e da onda um: a coluna é texto.",
                "text": "Dos arquivos e da onda um: a coluna é texto.", "keys": ["k"], "why": "w",
                "waves": [1], "applies_to": files, "origin": said}),
            );
            write(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": "Criar o índice da tabela.",
                "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [done], "origin": said}),
            );
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.codes().into_iter().map(|(id, code)| (code, id)).collect()
    }

    /// O Jev julga o item ligado aos arquivos da onda como o do projeto todo:
    /// com a chance abaixo de 0,2 ele sai do pedido, e com a de 0,2 fica. O
    /// que a tarefa faz, o de que a onda é dona e o de toda onda vão com a
    /// chance 0 e nem entram na pergunta. O envio grava o que saiu.
    #[test]
    fn the_jev_takes_a_file_linked_item_out_below_02_and_never_asks_about_the_ones_that_always_go() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_of_the_files_of_the_wave(root);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let spy = std::sync::Arc::clone(&seen);
        let by_id = ids.clone();
        let _jev = crate::commands::flow::round::item_choice::fake::answering(move |board| {
            spy.lock().unwrap().push(board.items.iter().map(|item| item.id).collect::<Vec<_>>());
            let mut chances: BTreeMap<u64, f64> = board.items.iter().map(|item| (item.id, 0.0)).collect();
            chances.insert(by_id["MSTD-RULE-0001"], 0.19);
            chances.insert(by_id["MSTD-RULE-0002"], 0.2);
            chances
        });

        let out = round(root, "x", None);

        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![vec![ids["MSTD-RULE-0001"], ids["MSTD-RULE-0002"]]],
            "only the two file-linked items nobody else carries are asked about"
        );
        let prompt = request_and_blocks_at(root, "x", &out, 0);
        assert!(!prompt.contains("MSTD-RULE-0001"), "a chance of 0.19 takes it out: {prompt}");
        for code in ["MSTD-RULE-0002", "MSTD-RULE-0003", "MSTD-RULE-0004", "MSTD-DEC-0001"] {
            assert!(prompt.contains(code), "{code} goes: {prompt}");
        }
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(
            log.whole_fields(sent)["analysis"],
            json!({
                "judged": [ids["MSTD-RULE-0001"], ids["MSTD-RULE-0002"]],
                "removed": [{"item": ids["MSTD-RULE-0001"], "why": "Jev p=0.19"}],
                "added": [],
                "judged_lessons": [], "removed_lessons": [],
            })
        );
    }

    /// Sem Jev, o pedido leva os itens ligados aos arquivos da onda como
    /// sempre levou, e o envio não grava escolha.
    #[test]
    fn without_the_jev_the_file_linked_items_go_in_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_items_of_the_files_of_the_wave(root);

        let out = round(root, "x", None);

        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let prompt = request_and_blocks_at(root, "x", &out, 0);
        for code in ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0003", "MSTD-RULE-0004", "MSTD-DEC-0001"] {
            assert!(prompt.contains(code), "{code} goes: {prompt}");
        }
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert!(log.send_field(sent.id, "analysis").is_none(), "nobody judged: {:?}", sent.fields);
    }

    /// O item de toda onda vai no pedido ainda que o Jev dê a ele a chance
    /// mais baixa, e nem entra na pergunta; o do projeto todo sem a marca, com
    /// a mesma chance, sai.
    #[test]
    fn an_every_wave_item_goes_even_when_the_jev_would_take_it_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let rule = |text: &str, extra: Value| {
                let mut body = json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said});
                body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                write(root, "x", "rule", body);
            };
            rule("Vale sempre: a tabela nova tem chave.", json!({"every_wave": true}));
            rule("Vale sempre: a spec vira um PR só.", json!({}));
        });
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let spy = std::sync::Arc::clone(&seen);
        let _jev = crate::commands::flow::round::item_choice::fake::answering(move |board| {
            spy.lock().unwrap().extend(board.items.iter().map(|item| item.id));
            board.items.iter().map(|item| (item.id, 0.0)).collect()
        });

        let out = round(root, "x", None);

        let prompt = request_at(&out, 0);
        assert!(prompt.contains("MSTD-RULE-0001"), "the every-wave item stays: {prompt}");
        assert!(!prompt.contains("MSTD-RULE-0002"), "the project item without the mark leaves with a chance of 0: {prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let second = log.visible().into_iter().find(|e| e.str_field("text") == Some("Vale sempre: a spec vira um PR só.")).unwrap().id;
        assert_eq!(*seen.lock().unwrap(), vec![second], "the Jev is asked only about the unmarked one");
    }

    /// Com o Jev recusando a chamada, a escolha dos itens vai à reserva: a
    /// onda espera o recibo, e a chamada fica gravada com o motivo. O recibo
    /// incerto vale como dúvida: a onda sai na rodada seguinte com o pedido
    /// padrão — o projeto todo vai, o sem ligação fica fora —, o envio não
    /// grava escolha, e o Jev não é chamado de novo.
    #[test]
    fn a_refused_call_goes_to_the_reserve_and_an_unsure_receipt_leaves_the_default_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_items_to_judge(root);
        let _jev = crate::commands::flow::round::item_choice::fake::refusing();

        let waiting = round(root, "x", None);
        assert!(waves_in(&waiting, "dispatch").is_empty(), "the wave waits for the receipt: {waiting}");
        let request = waiting["reserves"][0].clone();
        let calls = item_calls(root);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0]["filter"], json!("jev:refused"), "{calls:?}");

        crate::shared::jev::reserve::answer_in_test(root, &request, |_| "yes".into(), "unsure");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let prompt = request_at(&out, 0);
        assert!(prompt.contains(CHOSEN_ITEMS_BY_DEFAULT), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert!(log.send_field(sent.id, "analysis").is_none(), "{:?}", sent.fields);
        assert_eq!(item_calls(root).len(), 1, "the receipt answers in place of the Jev");
    }

    /// O mapa do projeto acompanha o commit atual sem segurar o pedido:
    /// enquanto o commit e a listagem do git gravados no mapa batem com os do
    /// checkout, a ferramenta do scan não roda; um commit feito fora da
    /// rodada — à mão, ou um pull — muda as linhas de uma função, e a onda
    /// seguinte sai sem esperar a passada, que vem depois do envio. Até ela
    /// acabar, o pedido cita a função pelo nome, sem as linhas velhas do mapa.
    /// A onda fica retida por um teto de compilações zerado na primeira
    /// volta, só para a comparação do mapa rodar sem nenhuma onda pronta para
    /// despachar; a segunda volta libera o teto.
    #[test]
    fn the_map_follows_the_code_without_holding_the_request() {
        crate::commands::flow::round::map_after_rounds();
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn before() {}\n\nfn sum() {\n    1 + 1;\n}\n").unwrap();
        approved_with(root, "x", &[], |said| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(
                root,
                "x",
                "wave",
                json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said}),
            );
            write(
                root,
                "x",
                "task",
                json!({"wave": 1, "text": "Somar.",
                "files": [{"path": "src/a.rs"}], "depends_on": [], "must_read": ["src/a.rs#soma"], "origin": said}),
            );
        });
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":0}"#).unwrap();
        // O que a instalação grava no projeto fica fora do git: a spec que a
        // rodada escreve não muda a listagem.
        std::fs::write(root.join(".git/info/exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();

        // O mapa já foi lido no commit atual (a "semente"), com a função em
        // 3-5, e guarda a listagem do git de agora e o conteúdo que leu.
        let head_v1 = git_text(root, &["rev-parse", "HEAD"]);
        let blob_v1 = git_text(root, &["hash-object", "src/a.rs"]);
        let now = mustard_core::io::project_map::listing(root).expect("o projeto está no git");
        let listing = now.digest();
        mustard_core::io::project_map::write_text(
            root,
            &json!({
                "modules": [{"path": "src/a.rs", "blob": blob_v1,
                    "declarations": [{"kind": "function", "name": "soma", "line": 3, "end_line": 5}]}],
                "state": {"head": head_v1, "listing": listing, "base": now.base.name, "base_tip": now.base.tip},
            })
            .to_string(),
        )
        .unwrap();

        // O teto está em zero: nenhuma onda sai, mas o mapa já é conferido.
        // Nada mudou: a ferramenta do scan não roda.
        fn mine_untouched(_: &Path, _: &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport> {
            panic!("a ferramenta do scan não deveria rodar com o mapa em dia");
        }
        let first = round_with_mine(root, "x", None, &mine_untouched);
        assert_eq!(waves_in(&first, "dispatch"), Vec::<u64>::new(), "{first}");

        // Um commit feito fora da rodada, sem passar pelo commit dela, desce
        // a função dez linhas.
        let moved = format!("{}fn before() {{}}\n\nfn sum() {{\n    1 + 1;\n}}\n", "// nova\n".repeat(10));
        std::fs::write(root.join("src/a.rs"), moved).unwrap();
        git_at(root, &["commit", "-qam", "fora da rodada"]);

        let scans: std::cell::RefCell<Vec<bool>> = std::cell::RefCell::new(Vec::new());
        let mine_after_the_send = |_: &Path, out: &Path| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            scans.borrow_mut().push(log.last_by_wave("send").contains_key(&1));
            mustard_core::io::project_map::write_text_at(
                out,
                &json!({"modules": [{"path": "src/a.rs", "declarations": [{"kind": "function", "name": "soma", "line": 13, "end_line": 15}]}]})
                    .to_string(),
            )
            .unwrap();
            Ok(mustard_core::domain::scan::ScanReport::default())
        };

        // O teto libera a vaga, sem nenhum commit da própria rodada: só o
        // mapa desatualizado explica a passada a seguir.
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();

        // A onda sai na mesma rodada, antes da passada, e o pedido não cita as
        // linhas velhas da função que o commit desceu.
        let second = round_with_mine(root, "x", None, &mine_after_the_send);
        assert_eq!(waves_in(&second, "dispatch"), vec![1], "{second}");
        let prompt = &request_at(&second, 0);
        assert!(!prompt.contains("linhas 3-5"), "{prompt}");
        assert!(prompt.contains("src/a.rs"), "{prompt}");
        let scans = scans.into_inner();
        assert!(!scans.is_empty() && scans.iter().all(|after| *after), "the pass comes after the send: {scans:?}");
    }

    /// A resposta da rodada mostra o estado de cada onda em andamento, para o
    /// orquestrador responder "como estamos?" sem conferir a cópia à mão: os
    /// minutos desde o envio, os arquivos que a cópia já tem mudados, o
    /// código e o texto do último passo gravado depois do envio, e os
    /// minutos desde o último sinal de vida. A onda sem passo gravado mostra
    /// só o que tem, sem a chave `step`.
    #[test]
    fn the_round_shows_the_state_of_each_running_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");

        // A onda 1 ganha um arquivo novo, ainda não entregue, na cópia dela;
        // e um passo gravado depois do envio.
        let copy1 = sent_copy(root, 1);
        std::fs::write(Path::new(&copy1).join("rascunho.txt"), "x").unwrap();
        write(root, "x", "step", json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "A tarefa 1 ficou pronta."}));

        let second = round(root, "x", None);
        let running = second["running"].as_array().cloned().unwrap_or_default();
        let entry_of = |wave: u64| running.iter().find(|r| r["wave"] == json!(wave)).cloned().unwrap_or_else(|| panic!("wave {wave}: {running:?}"));

        let one = entry_of(1);
        assert!(one["send"].as_str().is_some(), "{one}");
        assert!(one["minutes"].as_i64().is_some(), "os minutos desde o envio: {one}");
        assert_eq!(one["files"], json!(["rascunho.txt"]), "{one}");
        assert_eq!(one["step"], json!({"item": "MSTD-TASK-0001", "text": "A tarefa 1 ficou pronta."}), "{one}");
        assert!(one["silent_minutes"].as_i64().is_some(), "os minutos desde o sinal de vida: {one}");

        // A onda 2 não ganhou passo nenhum: a chave `step` fica de fora.
        let two = entry_of(2);
        assert!(two.get("step").is_none(), "sem passo, a chave fica de fora: {two}");
        assert_eq!(two["files"], json!([]), "a cópia da onda 2 não mudou: {two}");
        assert!(two["minutes"].as_i64().is_some(), "{two}");
        assert!(two["silent_minutes"].as_i64().is_some(), "{two}");

        // Nenhum aviso novo acorda o orquestrador: sem lista de "stuck" nem
        // pergunta, e a rodada não passa do que já se pergunta hoje.
        assert!(second.get("stopped").is_none(), "{second}");
    }

    /// O silêncio de uma onda conta da ação mais nova: o sinal de vida da
    /// vaga que o envio gravou, quando é mais novo que o envio; a hora do
    /// envio, quando o sinal da vaga ficou de uma onda anterior.
    #[test]
    fn the_silence_of_a_wave_counts_from_the_newest_between_its_slot_signal_and_its_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let copy = sent_copy(root, 1);
        let slot = Path::new(&copy).file_name().unwrap().to_string_lossy().into_owned();
        let alive = crate::hooks::observe::wave_alive_observer::alive_path(root, "x", &slot);
        std::fs::create_dir_all(alive.parent().unwrap()).unwrap();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.last_by_wave("send")[&1];
        let at = |minutes: i64| (chrono::Local::now() + chrono::Duration::minutes(minutes)).to_rfc3339();

        std::fs::write(&alive, at(30)).unwrap();
        let newer = silent_minutes(root, "x", 1, &log, sent).unwrap();
        assert!(newer <= -29, "o sinal da vaga, mais novo que o envio, vale: {newer}");

        std::fs::write(&alive, at(-120)).unwrap();
        let older = silent_minutes(root, "x", 1, &log, sent).unwrap();
        assert!((0..=1).contains(&older), "o sinal de uma onda anterior não conta: {older}");
    }

    /// A rodada sem o condutor lançando agente nenhum depois dela: a resposta
    /// como ela sai, sem o lançamento que [`round`] simula.
    fn round_without_launch(root: &Path, report: Option<&str>) -> Value {
        use crate::commands::flow::round::{RoundOpts, round_for};
        crate::commands::flow::round::review::test_conductor_review(root, "x");
        round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: report.map(str::to_string) }, None)
    }

    /// As duas ondas que a rodada soltou seguem em andamento, mas nenhuma
    /// conta como onda com agente trabalhando só porque o processo do
    /// condutor vive, nem porque o gancho do despacho deixou o agente sair: a
    /// permissão ainda pode negá-lo. A onda 1 passa a contar quando o agente
    /// dela age pela primeira vez, ligado ao envio pela conversa dele, com o
    /// sinal de vida recente. A onda 2, lançada há uma hora e calada desde
    /// então, não conta.
    #[test]
    fn the_launch_is_recorded_when_the_agent_first_acts() {
        use crate::hooks::task::subagent_inject::SubagentInject;
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["web/a.ts"], &[]), (2, &["web/b.ts"], &[])]);
        let out = round_without_launch(root, None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
        let live = || waves_with_live_agent(root, "x", &spec_now(root)).into_keys().collect::<Vec<u64>>();
        assert_eq!(waves_in_progress(&spec_now(root)).into_keys().collect::<Vec<u64>>(), vec![1, 2], "both sent waves stay listed");
        assert_eq!(live(), Vec::<u64>::new(), "no agent was launched");

        let first = out["dispatch"].as_array().unwrap().iter().find(|d| d["wave"] == json!(1)).unwrap()["read"].as_str().unwrap().to_string();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let dispatched = format!("{title}\n\n{first}");
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Agent".to_string()),
            tool_input: json!({ "prompt": dispatched, "subagent_type": "mustard-wave", "description": "onda" }),
            ..HookInput::default()
        };
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        let verdict = SubagentInject.evaluate(&input, &ctx).unwrap();
        assert!(!matches!(verdict, Verdict::Deny { .. }), "{verdict:?}");
        assert_eq!(live(), Vec::<u64>::new(), "the dispatch hook alone launches nothing");

        // A primeira ação do agente: a conversa dele abre com o pedido da
        // onda 1, e o gancho observa a chamada dele.
        let transcript = root.join("session/subagents/agent-worker.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        let opening = json!({"type": "user", "timestamp": chrono::Utc::now().to_rfc3339(), "message": {"role": "user", "content": dispatched}});
        std::fs::write(&transcript, opening.to_string()).unwrap();
        let call: HookInput = serde_json::from_value(json!({"cwd": root, "hook_event_name": "PreToolUse", "session_id": "session", "agent_id": "worker",
            "transcript_path": root.join("session.jsonl"), "tool_name": "Bash", "tool_input": {"command": first}})).unwrap();
        crate::shared::host_observation::observe(root, "claude", "PreToolUse", &call);
        assert_eq!(live(), vec![1], "the wave whose agent acted is live");

        let log = spec_now(root);
        let draft = send_revision(&log, 2, launch_fields()).unwrap();
        let hour_ago = (chrono::Local::now() - chrono::Duration::minutes(60)).format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        store::write_at(&store::spec_file(root, "x").unwrap(), "send", draft, &[], &hour_ago).unwrap();
        let log = spec_now(root);
        assert!(agent_launched(&log, log.last_by_wave("send")[&2]), "wave 2 was launched");
        assert_eq!(live(), vec![1], "the launched wave silent for an hour is not live");
    }

    /// A versão nova do envio leva só o que mudou: o pedido e o que o descreve
    /// ficam na versão que os gravou.
    #[test]
    fn a_new_version_of_the_send_does_not_repeat_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round_without_launch(root, None);
        let log = spec_now(root);
        let sent = log.last_by_wave("send")[&1];
        assert!(log.get(sent).and_then(|e| e.str_field("text")).is_some_and(|text| !text.is_empty()));
        let draft = send_revision(&log, 1, launch_fields()).unwrap();
        for field in ["text", "lines", "chars", "items", "read_items", "analysis", "skills", "summary", "lessons"] {
            assert!(!draft.contains_key(field), "{field}: {draft:?}");
        }
        assert_eq!(draft.get("replaces"), Some(&json!(sent)), "{draft:?}");
        assert_eq!(draft.get("agent_launched"), Some(&json!(true)), "{draft:?}");
        // A gravação aceita a versão sem o pedido, e quem lê o acha na
        // versão que o gravou.
        let text = log.get(sent).and_then(|e| e.str_field("text")).unwrap().to_string();
        let listed = log.get(sent).and_then(|e| e.fields.get("read_items")).cloned();
        store::write(&store::spec_file(root, "x").unwrap(), "send", draft, &[]).unwrap();
        let log = spec_now(root);
        let newest = log.last_by_wave("send")[&1];
        assert_ne!(newest, sent);
        assert_eq!(log.send_text(newest), Some(text.as_str()));
        assert_eq!(log.send_field(newest, "read_items").cloned(), listed);
    }

    /// O condutor encerrou a vez sem lançar o agente da onda 1, que ficou uma
    /// hora assim. A rodada seguinte não a trata como em andamento: devolve a
    /// onda no despacho com o mesmo comando de leitura, sem envio novo e na
    /// mesma cópia, e não manda parar um agente que nunca existiu.
    #[test]
    fn an_unlaunched_send_comes_back_in_dispatch_with_the_same_read() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round_without_launch(root, None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let copy = sent_copy(root, 1);
        let log = spec_now(root);
        let hour_ago = (chrono::Local::now() - chrono::Duration::minutes(60)).format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let older = send_revision(&log, 1, Map::new()).unwrap();
        store::write_at(&store::spec_file(root, "x").unwrap(), "send", older, &[], &hour_ago).unwrap();

        let again = round_without_launch(root, None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
        assert_eq!(again["dispatch"][0]["read"], first["dispatch"][0]["read"], "{again}");
        assert_eq!(waves_in(&again, "running"), Vec::<u64>::new(), "{again}");
        let silent = again["warnings"].as_array().into_iter().flatten().any(|w| w["reason"] == json!("wave-silent"));
        assert!(!silent, "no agent to stop: {again}");
        let log = spec_now(root);
        let dispatches = log.events.iter().filter(|e| e.event_type == "send" && log.dispatch_position(e.id) == e.id).count();
        assert_eq!(dispatches, 1, "no new send: {again}");
        assert_eq!(sent_copy(root, 1), copy, "{again}");
    }

    /// A primeira chamada do agente da onda não consegue gravar o lançamento —
    /// o arquivo da spec não aceita escrita naquela hora —, e a chamada
    /// seguinte do mesmo agente tenta de novo e grava: a onda não fica sem
    /// agente para a rodada, que a ofereceria a outro.
    #[cfg(unix)]
    #[test]
    fn a_launch_that_failed_to_record_is_tried_again_on_the_next_call() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round_without_launch(root, None);
        let read = out["dispatch"][0]["read"].as_str().unwrap().to_string();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let transcript = root.join("session/subagents/agent-worker.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        let opening = json!({"type": "user", "timestamp": chrono::Utc::now().to_rfc3339(), "message": {"role": "user", "content": format!("{title}\n\n{read}")}});
        std::fs::write(&transcript, opening.to_string()).unwrap();
        let call: mustard_core::domain::model::contract::HookInput = serde_json::from_value(json!({"cwd": root, "hook_event_name": "PreToolUse",
            "session_id": "session", "agent_id": "worker", "transcript_path": root.join("session.jsonl"), "tool_name": "Bash", "tool_input": {"command": read}}))
        .unwrap();
        let launched = || {
            let log = spec_now(root);
            agent_launched(&log, log.last_by_wave("send")[&1])
        };

        let spec = store::spec_file(root, "x").unwrap();
        std::fs::set_permissions(&spec, std::fs::Permissions::from_mode(0o444)).unwrap();
        crate::shared::host_observation::observe(root, "claude", "PreToolUse", &call);
        std::fs::set_permissions(&spec, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!launched(), "the first write failed");
        crate::shared::host_observation::observe(root, "claude", "PreToolUse", &call);
        assert!(launched(), "the next call of the same agent records the launch");
    }

    /// O Codex lançou o agente da onda e a resposta do lançamento não trouxe
    /// o número dele: o lançamento que terminou já prova que o agente nasceu.
    #[test]
    fn a_finished_codex_spawn_records_the_launch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let start: mustard_core::domain::model::contract::HookInput =
            serde_json::from_value(json!({"cwd": root, "hook_event_name": "SessionStart", "session_id": "codex-wave"})).unwrap();
        crate::shared::host_observation::observe(root, "codex", "SessionStart", &start);
        use crate::commands::flow::round::{Caller, RoundOpts};
        crate::commands::flow::round::review::test_conductor_review(root, "x");
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: None };
        let out = crate::commands::flow::round::round_in(&opts, Caller { session: Some("codex-wave"), config_dir: None });
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let read = out["dispatch"][0]["read"].as_str().unwrap().to_string();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let spawned: mustard_core::domain::model::contract::HookInput = serde_json::from_value(json!({"cwd": root, "hook_event_name": "PostToolUse",
            "session_id": "codex-wave", "tool_name": "spawn_agent", "tool_input": {"message": format!("{title}\n\n{read}"), "agent_type": "mustard-wave"},
            "tool_response": "spawned"})).unwrap();
        crate::host::codex("PostToolUse", &spawned);
        let log = spec_now(root);
        assert!(agent_launched(&log, log.last_by_wave("send")[&1]), "the finished spawn proves the agent");
    }

    /// O agente do envio da onda 1 deu o último sinal de vida há 6 minutos, e
    /// a rodada reenvia a onda agora (a onda pausada sai de novo). A resposta
    /// mostra a onda em andamento pelo envio novo, calada há 0 minuto: o
    /// silêncio conta do reenvio, não do sinal que o envio substituído deixou
    /// na vaga.
    #[test]
    fn silence_counts_from_the_replacing_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let slot = Path::new(&sent_copy(root, 1)).file_name().unwrap().to_string_lossy().into_owned();
        let alive = crate::hooks::observe::wave_alive_observer::alive_path(root, "x", &slot);
        std::fs::create_dir_all(alive.parent().unwrap()).unwrap();
        std::fs::write(&alive, (chrono::Local::now() - chrono::Duration::minutes(6)).to_rfc3339()).unwrap();

        let resent = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(waves_in(&resent, "dispatch"), vec![1], "a onda pausada sai de novo: {resent}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let replacing = log.codes().get(&log.last_by_wave("send")[&1]).cloned().unwrap();
        let running = resent["running"].as_array().cloned().unwrap_or_default();
        assert_eq!(running.len(), 1, "{resent}");
        assert_eq!(running[0]["send"], json!(replacing), "a onda anda pelo envio novo: {resent}");
        assert_eq!(running[0]["silent_minutes"], json!(0), "{resent}");
    }

    /// Um arquivo já rastreado, mudado sem estar preparado, sai do `git
    /// status` com o código de estado começando em espaço (`" M arquivo"`);
    /// a saída inteira é trimada antes de virar linhas, o que apaga esse
    /// espaço quando o arquivo é o primeiro. A lista de arquivos mudados
    /// continua trazendo o nome inteiro, sem a primeira letra cortada.
    #[test]
    fn the_wave_is_found_by_the_paths_of_the_call() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");

        let copy1 = sent_copy(root, 1);
        std::fs::write(Path::new(&copy1).join("src").join("a.rs"), "fn dois() {}\n").unwrap();

        let second = round(root, "x", None);
        let running = second["running"].as_array().cloned().unwrap_or_default();
        let one = running.iter().find(|r| r["wave"] == json!(1)).cloned().unwrap_or_else(|| panic!("wave 1: {running:?}"));
        assert_eq!(one["files"], json!(["src/a.rs"]), "{one}");
    }

    /// A ordem de despacho sai do grafo de dependências que cada onda
    /// declara, com desempate pela quantidade de ondas que cada uma destrava
    /// — direta ou por outra —, da maior para a menor, e só depois pelo
    /// número: a onda 21 destrava a 22 e a 20 não destrava nada, então a 21
    /// sai primeiro mesmo tendo o número maior. O mesmo conjunto de ondas, aprovada duas
    /// vezes com as ondas declaradas em ordem diferente, despacha sempre na
    /// mesma sequência através das rodadas — a prova atravessa `round`, o
    /// comando de verdade, não a função auxiliar que só ordena.
    #[test]
    fn same_set_of_waves_always_produces_the_same_dispatch_order() {
        let plan_a: [(u64, &[&str], &[u64]); 4] =
            [(10, &["src/a.rs"], &[]), (20, &["src/b.rs"], &[10]), (21, &["src/c.rs"], &[10]), (22, &["src/d.rs"], &[21])];
        let plan_b: [(u64, &[&str], &[u64]); 4] =
            [(22, &["src/d.rs"], &[21]), (21, &["src/c.rs"], &[10]), (20, &["src/b.rs"], &[10]), (10, &["src/a.rs"], &[])];
        let mut sequences = Vec::new();
        for plan in [plan_a.as_slice(), plan_b.as_slice()] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", plan);
            let first = round(root, "x", None);
            let second = round(root, "x", Some(&delivered(root, 10, "Saiu.", &["src/a.rs"])));
            let third = round(root, "x", Some(&format!("{}\n{}", delivered(root, 21, "Saiu.", &["src/c.rs"]), delivered(root, 20, "Saiu.", &["src/b.rs"]))));
            sequences.push(vec![waves_in(&first, "dispatch"), waves_in(&second, "dispatch"), waves_in(&third, "dispatch")]);
        }
        let expected = vec![vec![10], vec![21, 20], vec![22]];
        assert_eq!(sequences[0], expected, "o backlog declarada numa ordem: {sequences:?}");
        assert_eq!(sequences[1], expected, "o mesmo conjunto de ondas, declarada noutra ordem, despacha igual: {sequences:?}");
    }

    /// Ondas que dependem umas das outras em círculo são recusadas pela
    /// própria rodada, mostrando o ciclo: nenhuma sai, e a resposta não é uma
    /// fila vazia muda. O backlog nunca passa pela aprovação do plano — o
    /// fixture grava o ciclo direto, como um evento gravado à mão poderia —
    /// e a prova atravessa `round`, o comando de verdade.
    #[test]
    fn set_of_waves_with_a_circular_dependency_is_refused_by_the_round_showing_the_cycle() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(4, &["src/a.rs"], &[7]), (7, &["src/b.rs"], &[4])]);
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("waves-loop"), "{out}");
        assert_eq!(out["hint"], json!(WAVE_LOOP_4_7), "a recusa nomeia as ondas do ciclo: {out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "nenhuma onda sai com o ciclo: {out}");
        // Sem relatório, a rodada não gravou entrega nem comitou nada antes
        // de achar o ciclo: a resposta não inventa gravação.
        assert_eq!(out["recorded"], json!([]), "{out}");
        assert!(out.get("commit").is_none(), "{out}");
    }

    /// A recusa das ondas 4 e 7, que dependem uma da outra, como a pessoa a
    /// lê: as duas pelo número, e nenhuma outra.
    const WAVE_LOOP_4_7: &str = "As ondas 4, 7 dependem umas das outras em círculo, e nenhuma pode começar. Corte uma das dependências.";

    /// A rodada que recebe a entrega da onda 1 junta, comita e grava a
    /// entrega, e só depois, ao escolher as ondas seguintes, acha as ondas 4 e
    /// 7 em círculo. A recusa não troca a resposta: ela vem junto do que foi
    /// gravado, do commit e da indicação do painel local, e nomeia só as
    /// ondas do ciclo — a 1, entregue, fica fora dela. A prova atravessa
    /// `round`, o comando de verdade.
    #[test]
    fn cycle_refusal_returns_what_the_round_wrote() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // O ciclo entra depois do despacho da onda 1, gravado direto, como um
        // evento gravado à mão poderia: a rodada seguinte só o encontra
        // depois de juntar a entrega.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let visible = log.visible();
        let said = visible.iter().find(|e| e.event_type == "message").unwrap().id;
        let crit = visible.iter().find(|e| e.event_type == "criterion").unwrap().id;
        for (n, on, file) in [(4u64, 7u64, "src/b.rs"), (7, 4, "src/c.rs")] {
            write(
                root,
                "x",
                "wave",
                json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "depends_on": [on], "origin": said}),
            );
            write(
                root,
                "x",
                "task",
                json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": file}], "depends_on": [], "origin": said}),
            );
        }

        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("waves-loop"), "{out}");
        assert_eq!(out["hint"], json!(WAVE_LOOP_4_7), "a recusa nomeia as ondas do ciclo, e só elas: {out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "nenhuma onda sai com o ciclo: {out}");

        // O que foi gravado: a entrega da onda 1, que está no arquivo de
        // eventos com o número que a resposta traz.
        let recorded = out["recorded"].as_array().cloned().unwrap_or_default();
        let entry = recorded
            .iter()
            .find(|r| r["type"] == json!("delivered") && r["wave"] == json!(1))
            .unwrap_or_else(|| panic!("a entrega da onda 1 na resposta: {out}"));
        let log = store::read(&path).unwrap().unwrap();
        let written = log.get(entry["id"].as_u64().unwrap_or_default()).unwrap_or_else(|| panic!("{out}"));
        assert_eq!(written.event_type, "delivered", "{out}");

        // O commit da rodada: na resposta e no git.
        assert!(out.get("commit").is_some_and(|c| !c.is_null()), "o commit vem na resposta: {out}");
        let subject = Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(root).output().unwrap();
        assert!(String::from_utf8_lossy(&subject.stdout).contains("a entrega 1 saiu"), "{out}");

        // The refusal keeps the recorded result and the local panel entry point.
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(WAVE_LOOP_4_7), "{next}");
        assert!(out.get("copy").is_none() && out.get("publish").is_none());
        assert!(!root.join(".claude/spec/x/copy").exists());
    }

    /// A tarefa que sai de uma onda de duas por evento de remoção, depois de a
    /// onda já ter sido formada e não enviada — a rodada a desfaz e monta de
    /// novo só com as tarefas visíveis —, não deixa rastro no pedido dela: nem na lista de
    /// tarefas, nem no parágrafo de abertura que abre pelo `done_when` — que,
    /// sem critério com prova (o caso do item combinado sem dono, que não tem
    /// prova), é o texto das próprias tarefas unido por espaço, o caminho
    /// pelo qual o texto de uma tarefa retirada podia vazar no pedido. A
    /// prova atravessa `round`, o comando de verdade, para exercitar o pedido
    /// como o agente o recebe.
    #[test]
    fn wave_request_does_not_cite_a_withdrawn_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").unwrap().id;

        // Dois itens combinados sem dono (uma decisão cada), sem prova — o
        // caso que o backlog empacota quando o levantamento marca um item sem
        // onda: `waves` os dá dono antes mesmo de a onda 2 existir.
        let item1 = id_of(&write(root, "x", "decision", json!({"text": "Um item.", "keys": ["k1"], "why": "porque sim", "waves": [2], "origin": said})));
        let item2 = id_of(&write(root, "x", "decision", json!({"text": "Outro item.", "keys": ["k2"], "why": "porque sim", "waves": [2], "origin": said})));

        // Duas tarefas soltas no mesmo arquivo, cada uma cobrindo um item sem
        // prova: o pronto-quando da onda cai nos títulos delas — o mesmo
        // caminho pelo qual o texto da tarefa retirada chegaria ao pedido.
        let t1 = id_of(&write(
            root,
            "x",
            "task",
            json!({"title": "Gravar a versao nova de uma decisao", "text": "A decisao ja foi feita fora da onda.",
                "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [item1, "MSTD-CRIT-0001"], "origin": said}),
        ));
        id_of(&write(
            root,
            "x",
            "task",
            json!({"title": "Trocar a mensagem de erro do campo vazio", "text": "Hoje a mensagem nao diz o campo.",
                "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [item2, "MSTD-CRIT-0001"], "origin": said}),
        ));

        let log = store::read(&path).unwrap().unwrap();
        let formed = dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed).expect("formou o lote");
        assert_eq!(formed, vec![2], "as duas tarefas do mesmo arquivo viram uma onda só: {formed:?}");

        // A tarefa 1 sai do backlog por remoção, depois de o lote já ter sido
        // formado: o trabalho dela já foi feito fora da onda. A gravação do
        // lote deu a ela uma versão nova, com o número da onda; a remoção a
        // aponta pelo código, que tira todas as versões, como o número de
        // qualquer versão da tarefa também tira.
        let log = store::read(&path).unwrap().unwrap();
        let t1_code = log.codes().get(&t1).cloned().expect("a tarefa 1 tem código");
        write(root, "x", "remove", json!({"targets": [t1_code], "reason": "o trabalho ja foi feito fora da onda"}));

        let out = round(root, "x", None);
        let waves = waves_in(&out, "dispatch");
        assert_eq!(waves.len(), 2, "{out}");
        let prompt = &request_of(&out, waves[1]);
        assert!(!prompt.is_empty(), "the batch wave: {out}");
        assert!(!prompt.contains("Gravar a versao nova de uma decisao"), "o texto da tarefa retirada nao pode aparecer no pedido: {prompt}");
        assert!(prompt.contains("Trocar a mensagem de erro do campo vazio\n"), "o pronto-quando nasce das tarefas visiveis agora: {prompt}");
        for whole in ["A decisao ja foi feita", "Hoje a mensagem nao diz"] {
            assert!(!prompt.contains(whole), "o texto inteiro da tarefa nao entra no pedido: {prompt}");
        }
    }

    /// Os motivos dos avisos do plano que conferiam onda como desenho: ondas
    /// da mesma rodada dividindo arquivo, onda ou spec que podia sair
    /// dividida, e tarefa cujo texto não casa com o da onda.
    const WAVE_DRAWING_REASONS: [&str; 5] = ["waves-share-a-file", "wave-should-split", "spec-should-split", "task-in-the-wrong-wave", "task-matches-no-wave"];

    /// Um projeto com `src/a.rs` e `src/b.rs` no git e uma spec aprovada sem
    /// onda nenhuma: as tarefas vão para o backlog. Devolve o número da fala
    /// do usuário e o do critério, que as tarefas citam.
    pub(crate) fn backlog_project(root: &Path) -> (u64, u64) {
        backlog_project_with(root, |_| {})
    }

    /// [`backlog_project`] com o que o teste grava antes da aprovação
    /// (`before`), que recebe o número da fala do usuário.
    fn backlog_project_with(root: &Path, before: impl FnOnce(u64)) -> (u64, u64) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.rs", "b.rs"] {
            std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
        }
        approved_with(root, "x", &[], |said| {
            // O levantamento fechado, ponto a ponto, como o plano exige.
            let goal = "Mexer no código de um e de dois.";
            id_of(&write(root, "x", "context", json!({"text": goal, "origin": said})));
            let opts =
                crate::commands::flow::grill::GrillOpts { root: root.to_path_buf(), spec: Some("x".into()), kinds: Some("fix".into()), condensed: false };
            // O `grill` grava os pontos abertos; cada um é fechado pelo número.
            let listed = crate::commands::flow::grill::grill_for(&opts, None);
            for item in listed["points"].as_array().cloned().unwrap_or_default() {
                let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap",
                    "status": "not_applicable", "closes": id_of(&item), "reason": "Já respondido.", "origin": said});
                assert_eq!(write(root, "x", "point", closing)["ok"], json!(true));
            }
            before(said);
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        (first("message"), first("criterion"))
    }

    /// Uma tarefa do backlog, gravada pela porta do modelo, num arquivo só.
    fn backlog_task(root: &Path, said: u64, crit: u64, text: &str, file: &str) -> u64 {
        id_of(&write(
            root,
            "x",
            "task",
            json!({"text": text, "files": [{"path": file}], "depends_on": [],
            "covers": [crit], "origin": said}),
        ))
    }

    /// A spec reaberta, de volta ao levantamento, e o plano rodado de novo
    /// nela, com as anotações que ele gravou.
    fn replanned(root: &Path) -> (Value, Vec<Value>) {
        let reopened = crate::commands::flow::reopen::reopen_for(
            &crate::commands::flow::reopen::ReopenOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                reason: "Mais trabalho na mesma obra.".into(),
                fix: false,
            },
            None,
        );
        assert_eq!(reopened["ok"], json!(true), "{reopened}");
        let report = crate::commands::flow::plan::plan_for(&crate::commands::flow::plan::PlanOpts { root: root.to_path_buf(), spec: Some("x".into()) }, None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let notes = log.visible().into_iter().filter(|e| e.event_type == "note").map(|e| Value::Object(e.fields.clone())).collect();
        (report, notes)
    }

    /// Os motivos de desenho de onda que o plano disse: na resposta, entre as
    /// travas e os avisos, e nas anotações que ele gravou.
    fn wave_drawing_said(report: &Value, notes: &[Value]) -> Vec<String> {
        let answered = ["blocking", "warnings"]
            .iter()
            .flat_map(|field| report[*field].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f["reason"].as_str().map(str::to_string));
        let noted = notes.iter().flat_map(|n| n["keys"].as_array().cloned().unwrap_or_default()).filter_map(|k| k.as_str().map(str::to_string));
        answered.chain(noted).filter(|r| WAVE_DRAWING_REASONS.contains(&r.as_str())).collect()
    }

    /// Numa spec reaberta, a primeira rodada leva a tarefa de `src/a.rs` na
    /// onda 1, que entrega; a segunda leva uma tarefa nova de `src/a.rs` na
    /// onda 2 e outra de `src/b.rs` na onda 3, cada uma com o seu assunto. As
    /// ondas de lote nascem sem dependência entre elas, e a 2 toca o mesmo
    /// arquivo da 1, já entregue: o plano rodado depois não fala de ondas
    /// dividindo arquivo, nem de onda ou spec que podia sair dividida.
    #[test]
    fn plan_does_not_talk_about_a_wave_sharing_a_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let record = write(root, "x", "delivered", json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a.rs"]}));
        assert_eq!(record["ok"], json!(true), "{record}");

        let again = backlog_task(root, said, crit, "Mexer de novo no código de um.", "src/a.rs");
        let other = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![2, 3]), "cada tarefa nova vira a onda do seu assunto");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let waves: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "wave").collect();
        assert_eq!(waves.len(), 3, "{waves:?}");
        assert!(waves.iter().all(|w| w.ints("depends_on").is_empty() && w.str_field("author") == Some("binary")));
        assert_eq!(waves[1].ints("order"), vec![again], "a onda 2 leva a tarefa de `src/a.rs`");
        assert_eq!(waves[2].ints("order"), vec![other], "a onda 3 leva a tarefa de `src/b.rs`");

        let (report, notes) = replanned(root);
        assert_eq!(wave_drawing_said(&report, &notes), Vec::<String>::new(), "{report}");
    }

    /// Todas as linhas do arquivo de eventos da spec `x` do tipo `kind`,
    /// também as versões já substituídas, na ordem em que foram gravadas.
    fn every_line_of(root: &Path, kind: &str) -> Vec<Value> {
        let text = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap();
        text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|event| event["type"] == json!(kind)).collect()
    }

    /// Duas rodadas ao mesmo tempo, com uma tarefa pronta no backlog, leem a
    /// spec antes de qualquer uma pegar a trava. A que pega primeiro forma a
    /// onda de lote e a solta; a outra relê a spec já com a trava presa, acha
    /// a tarefa na onda formada e não forma nem solta nada. Sai uma onda só,
    /// com um envio só, e a tarefa ganha uma versão só com o número da onda.
    #[test]
    fn two_rounds_at_once_form_and_send_a_single_batch_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");

        let outs = two_rounds_at_once(root, None);
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        let sent: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(sent, vec![1], "só uma das rodadas solta a onda: {outs:?}");
        let waves: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(waves, vec![json!(1)], "a onda de lote é gravada uma vez: {outs:?}");
        // A versão do envio que marca o agente lançado não é envio novo.
        let sends: Vec<Value> = every_line_of(root, "send").iter().filter(|s| s.get("replaces").is_none()).map(|s| s["wave"].clone()).collect();
        assert_eq!(sends, vec![json!(1)], "um envio só: {outs:?}");
        let numbered = every_line_of(root, "task").iter().filter(|t| t["wave"] == json!(1)).count();
        assert_eq!(numbered, 1, "a tarefa ganha o número da onda uma vez: {outs:?}");
    }

    /// A rodada forma o lote com a tarefa pronta nas duas leituras — a da
    /// entrada e a feita com a trava presa — e numera a onda nova pela
    /// segunda. A tarefa um estava pronta na entrada desta rodada, mas outra
    /// rodada já a levou na onda 1, que segue em andamento, antes de esta
    /// pegar a trava; a tarefa dois, pronta nas duas e com os seis arquivos
    /// com que um lote sai ao lado de outra onda, sai sozinha na onda 2. A
    /// tarefa três, gravada depois da entrada, só está pronta na leitura com a
    /// trava e espera a rodada seguinte.
    #[test]
    fn the_batch_takes_only_the_task_ready_in_both_readings_and_numbers_after_the_lock() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let path = store::spec_file(root, "x").unwrap();
        let one = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let first_entry = store::read(&path).unwrap().unwrap();
        let two = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs", "lib/2.rs", "lib/3.rs", "lib/4.rs", "lib/5.rs", "lib/6.rs"]);
        let second_entry = store::read(&path).unwrap().unwrap();

        // A outra rodada, que entrou antes da tarefa dois, forma a onda 1 com a
        // tarefa um e solta a trava.
        assert_eq!(dispatch_backlog(root, "x", &first_entry, &first_entry, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);
        let three = backlog_task(root, said, crit, "Mexer de novo no código de dois.", "src/b.rs");

        let locked = store::read(&path).unwrap().unwrap();
        assert_eq!(dispatch_backlog(root, "x", &second_entry, &locked, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![2]), "a onda nova é a 2");

        let log = store::read(&path).unwrap().unwrap();
        let order = |n: u64| {
            let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(n));
            wave.map(|w| w.ints("order")).unwrap_or_default()
        };
        assert_eq!(order(1), vec![one], "a onda 1 segue só com a tarefa um");
        assert_eq!(order(2), vec![two], "a onda 2 leva só a tarefa pronta nas duas leituras");
        let waves: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(waves, vec![json!(1), json!(2)], "nenhum número de onda se repete");
        let task_three = log.current(three).expect("a tarefa três segue viva");
        assert_eq!(task_three.wave(), None, "a tarefa três fica no backlog para a rodada seguinte");
    }

    /// A tarefa pronta quando a rodada entrou ganha versão nova — outro texto,
    /// outro número — antes de a rodada pegar a trava. O lote sai com a
    /// versão em vigor: a onda nova leva o número novo, e a tarefa, com o
    /// texto revisto, ganha o número da onda. A prova atravessa a rodada de
    /// verdade, com a revisão entre a leitura de entrada e o resto dela.
    #[test]
    fn a_task_revised_between_the_two_readings_goes_out_in_the_batch_with_its_current_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");

        let entry = round_entry(root, None);
        let revised = id_of(&write(
            root,
            "x",
            "task",
            json!({"replaces": first, "text": "Mexer de outro jeito no código de um.",
            "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said}),
        ));
        let out = round_from(root, None, entry);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a tarefa revista sai no lote: {out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1));
        assert_eq!(wave.map(|w| w.ints("order")), Some(vec![revised]), "a onda leva o número da versão em vigor");
        let task = log.current(revised).expect("a tarefa segue viva");
        assert_eq!(task.wave(), Some(1), "a tarefa ganha o número da onda");
        assert_eq!(task.str_field("text"), Some("Mexer de outro jeito no código de um."), "com o texto revisto");
    }

    /// A tarefa da onda de lote em andamento ganha versão nova, pela porta do
    /// modelo, com um texto que não casa com o da onda — o texto da onda de
    /// lote é a junção dos textos da versão velha. O plano rodado depois não
    /// trava, e nenhum aviso fala de tarefa na onda errada ou sem onda.
    #[test]
    fn plan_does_not_check_the_task_text_against_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(waves_in_progress(&log).contains_key(&1), "a onda 1 está em andamento");
        let current = log.current(task).expect("a tarefa tem versão vigente");
        assert_eq!(current.wave(), Some(1), "o lote deu a ela o número da onda");
        let revised = crate::commands::spec_events::write::write_at(&crate::commands::spec_events::write::WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: "task".into(),
            json: json!({"replaces": current.id, "wave": 1, "title": "Somar", "text": "Somar dois números inteiros.",
                "agent": "- somar", "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said})
            .to_string(),
        });
        assert_eq!(revised["ok"], json!(true), "{revised}");

        let (report, notes) = replanned(root);
        assert_eq!(report["blocking"], Value::Null, "o plano não trava: {report}");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(wave_drawing_said(&report, &notes), Vec::<String>::new(), "{report}");
    }

    /// Duas tarefas soltas do backlog formariam duas ondas na mesma rodada, e
    /// a versão nova da segunda, com o número da onda, seria recusada: num
    /// caso ela traz um campo que esta versão não conhece, gravado por um
    /// Mustard mais novo; no outro, não declara as dependências, como uma
    /// tarefa de antes da regra. A rodada responde a recusa da gravação e não
    /// grava nada: nem as ondas, nem a versão da primeira tarefa. A spec
    /// fica igual byte a byte. A prova atravessa `round`, o comando de
    /// verdade.
    #[test]
    fn refused_round_leaves_no_wave_written() {
        let cases: [(&str, &str, &str); 2] =
            [("campo novo", ",\"text\":", "unknown-field"), ("sem dependências", ",\"depends_on\":[]", "task-declaration-missing")];
        for (case, cut, reason) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let (said, crit) = backlog_project(root);
            backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
            let bad = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
            // A spec já está em execução: a rodada não tem fase a gravar antes
            // de formar o lote.
            assert!(crate::commands::spec_events::write::record_phase(root, "x", "running", None), "{case}");

            let path = store::spec_file(root, "x").unwrap();
            let mark = format!("\"id\":{bad},");
            let original = std::fs::read_to_string(&path).unwrap();
            let edited: String = original
                .lines()
                .map(|line| match (line.contains(&mark), reason) {
                    (true, "unknown-field") => line.replacen(cut, ",\"futuro\":1,\"text\":", 1),
                    (true, _) => line.replacen(cut, "", 1),
                    (false, _) => line.to_string(),
                })
                .map(|line| line + "\n")
                .collect();
            assert_ne!(edited, original, "{case}: a linha da segunda tarefa mudou");
            std::fs::write(&path, &edited).unwrap();

            let out = round(root, "x", None);

            assert_eq!(out["ok"], json!(false), "{case}: {out}");
            assert_eq!(out["reason"], json!(reason), "{case}: {out}");
            assert_eq!(
                super::super::tests::without_measurements(&std::fs::read_to_string(&path).unwrap()),
                super::super::tests::without_measurements(&edited),
                "{case}: a spec fica igual byte a byte"
            );
            let log = store::read(&path).unwrap().unwrap();
            assert!(log.visible().iter().all(|e| e.event_type != "wave"), "{case}: nenhuma onda gravada");
        }
    }

    /// Uma tarefa de limpeza do backlog — nasceu de uma sobra que só muda
    /// comentário —, gravada pela porta do modelo, num arquivo só.
    fn cleanup_task(root: &Path, said: u64, crit: u64, text: &str, file: &str) -> u64 {
        id_of(&write(
            root,
            "x",
            "task",
            json!({"text": text, "files": [{"path": file}], "depends_on": [],
            "covers": [crit], "origin": said, "cleanup": true}),
        ))
    }

    /// A spec `x` como está no arquivo agora.
    pub(crate) fn spec_now(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A ordem gravada na onda `n` da spec `x`.
    pub(crate) fn wave_order(root: &Path, n: u64) -> Vec<u64> {
        let log = spec_now(root);
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(n));
        wave.map(|w| w.ints("order")).unwrap_or_default()
    }

    /// A limpeza que divide o arquivo com uma tarefa comum sai na mesma onda
    /// dela, já na primeira rodada.
    #[test]
    fn a_cleanup_sharing_a_file_with_a_task_leaves_in_its_wave() {
        let dir = tempdir().unwrap(); let root = dir.path();
        let (said, crit) = backlog_project(root);
        let normal = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let tidy = cleanup_task(root, said, crit, "Acertar o comentário de um.", "src/a.rs");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(wave_order(root, 1), vec![normal, tidy], "{out}");
    }

    /// A limpeza de outro arquivo não espera a tarefa comum terminar: a
    /// primeira rodada solta as duas, cada uma na sua onda.
    #[test]
    fn a_cleanup_on_another_file_leaves_in_the_same_round_as_the_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let normal = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let tidy = cleanup_task(root, said, crit, "Acertar o comentário de dois.", "src/b.rs");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
        assert_eq!((wave_order(root, 1), wave_order(root, 2)), (vec![normal], vec![tidy]), "{out}");
    }

    /// As limpezas se agrupam por arquivo como as outras tarefas: as que
    /// dividem um arquivo saem juntas numa onda, e a de outro arquivo sai na
    /// sua, uma onda por assunto.
    #[test]
    fn cleanups_group_by_file_like_any_other_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = cleanup_task(root, said, crit, "Acertar o comentário 1.", "src/a.rs");
        let second = cleanup_task(root, said, crit, "Acertar o comentário 2.", "src/a.rs");
        let apart = cleanup_task(root, said, crit, "Acertar o comentário 3.", "src/b.rs");
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1, 2]), "um assunto por onda");
        assert_eq!(wave_order(root, 1), vec![first, second], "as duas do mesmo arquivo saem juntas");
        assert_eq!(wave_order(root, 2), vec![apart], "a de outro arquivo sai na sua");
    }

    /// Uma tarefa do backlog sem `covers`, gravada pelo programa, como a sobra
    /// que uma onda sem critério deixa: o modelo não consegue gravá-la, o
    /// programa sim. Ela vai para o arquivo `file` e depende das tarefas
    /// `depends_on`.
    fn uncovered_task(root: &Path, text: &str, file: &str, depends_on: &[u64]) -> u64 {
        let draft = json!({"title": "Tarefa sem itens", "text": text, "agent": "- mexer", "files": [{"path": file}],
            "depends_on": depends_on, "author": "wave"});
        let draft = draft.as_object().cloned().unwrap();
        record(root, "x", "task", draft, PhaseWriter::Binary).unwrap().written.id
    }

    /// A tarefa do backlog sem `covers` não forma onda, porque a onda leva os
    /// critérios que as tarefas dela cobrem e a gravação recusa a que não
    /// leva nenhum. A rodada solta a onda da tarefa que cobre, deixa a outra
    /// no backlog sem número de onda e a nomeia num aviso — e a recusa dela
    /// não derruba a rodada inteira.
    #[test]
    fn a_backlog_task_without_covers_stays_out_of_the_waves_and_the_round_names_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let covered = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let bare = uncovered_task(root, "Mexer no código de dois.", "src/b.rs", &[]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "a tarefa sem covers não derruba a rodada: {out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(wave_order(root, 1), vec![covered], "a onda leva só a tarefa que cobre");
        let log = spec_now(root);
        assert_eq!(log.current(bare).and_then(SpecEvent::wave), None, "a tarefa sem covers segue sem onda");
        let codes = log.codes();
        let hint = warning_of(&out, "task-without-covers")["hint"].as_str().unwrap_or_default().to_string();
        assert!(hint.contains(&codes[&bare]), "o aviso nomeia a tarefa: {hint}");
        assert!(!hint.contains(&codes[&covered]), "e só a que não cobre: {hint}");
    }

    /// Sozinha no backlog, a tarefa sem `covers` não conta como pronta: a
    /// rodada não manda rodar de novo, que daria no mesmo, e a nomeia, com a
    /// que espera por ela, entre as presas. Sem onda formada e sem recusa.
    #[test]
    fn a_task_without_covers_alone_in_the_backlog_is_held_and_not_offered_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, _) = backlog_project(root);
        let bare = uncovered_task(root, "Mexer no código de um.", "src/a.rs", &[]);
        let after = uncovered_task(root, "Mexer no código de dois.", "src/b.rs", &[bare]);
        assert_eq!(backlog_ready(&spec_now(root)), Vec::<u64>::new(), "nenhuma está pronta");

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "{out}");
        assert!(out.get("command").is_none(), "não há o que rodar de novo: {out}");
        let codes = spec_now(root).codes();
        let held = format!("{}, {}", codes[&bare], codes[&after]);
        let expected = translate("round.backlog_stuck", Locale::PtBr).replace("{tasks}", &held);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&expected), "{out}");
        assert!(every_line_of(root, "wave").is_empty(), "nenhuma onda foi formada: {out}");
    }

    /// A tarefa sem `covers` que espera só por uma tarefa do lote e divide
    /// arquivo com ela não pega carona na onda: a onda prova o que as tarefas
    /// dela cobrem, e esta não cobre nada.
    #[test]
    fn a_task_without_covers_waiting_on_a_batch_task_does_not_ride_in_its_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let covered = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let bare = uncovered_task(root, "Conferir o código de um.", "src/a.rs", &[covered]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        assert_eq!(wave_order(root, 1), vec![covered], "a onda leva só a tarefa que cobre");
        assert_eq!(spec_now(root).current(bare).and_then(SpecEvent::wave), None, "a outra segue no backlog");
    }

    /// Uma tarefa do backlog em vários arquivos, gravada pela porta do modelo.
    /// Six distinct declared files for overlap and packing fixtures.
    pub(crate) const SIX_FILES: [&str; 6] = ["lib/1.rs", "lib/2.rs", "lib/3.rs", "lib/4.rs", "lib/5.rs", "lib/6.rs"];

    /// Um pedido aberto da onda `n`, com o processo deste teste por trás: a
    /// onda está em andamento.
    pub(crate) fn seed_running(root: &Path, n: u64) {
        let (claude_pid, claude_started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            json!({"wave": n, "role": "wave",
            "text": "pedido", "lines": 1, "chars": 6, "items": [1], "mustard": "0", "author": "binary",
            "claude_pid": claude_pid, "claude_started": claude_started}),
        );
    }

    pub(crate) fn backlog_task_on(root: &Path, said: u64, crit: u64, text: &str, files: &[&str]) -> u64 {
        let files: Vec<Value> = files.iter().map(|file| json!({ "path": file })).collect();
        id_of(&write(
            root,
            "x",
            "task",
            json!({"text": text, "files": files, "depends_on": [],
            "covers": [crit], "origin": said}),
        ))
    }

    /// Três assuntos no backlog, cada um num arquivo, e duas vagas: saem duas
    /// ondas, uma por assunto, e o terceiro assunto fica no backlog sem onda
    /// nenhuma. Com as duas vagas ocupadas, a rodada seguinte não monta onda.
    #[test]
    fn three_subjects_and_two_slots_make_two_waves_and_the_third_waits_in_the_backlog() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        let one = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let two = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        let three = backlog_task(root, said, crit, "Mexer no código de três.", "src/c.rs");

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
        assert_eq!(wave_order(root, 1), vec![one], "a onda 1 leva o primeiro assunto");
        assert_eq!(wave_order(root, 2), vec![two], "a onda 2 leva o segundo");
        let log = spec_now(root);
        assert_eq!(log.current(three).and_then(SpecEvent::wave), None, "o terceiro assunto fica sem onda");
        assert!(backlog_left(&log).contains(&three), "e segue no backlog");
        assert_eq!(log.planned_waves().len(), 2, "só existem as duas ondas que rodam");

        let idle = round(root, "x", None);
        assert_eq!(waves_in(&idle, "dispatch"), Vec::<u64>::new(), "as duas vagas seguem ocupadas: {idle}");
        let formed: Vec<Value> = every_line_of(root, "wave").iter().map(|w| w["n"].clone()).collect();
        assert_eq!(formed, vec![json!(1), json!(2)], "nenhuma onda nova nasce: {idle}");
    }

    /// Quando uma onda entrega, a rodada que a assume monta o assunto que
    /// esperava, na vaga que abriu, e a onda segue ao lado da que ainda roda.
    #[test]
    fn when_a_wave_delivers_the_round_forms_the_subject_that_was_waiting() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        let three = backlog_task_on(root, said, crit, "Mexer no código de três.", &SIX_FILES);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "a vaga que abriu leva o assunto que esperava: {out}");
        assert_eq!(wave_order(root, 3), vec![three], "a onda 3 leva só ele");
        let running: Vec<u64> = waves_in_progress(&spec_now(root)).into_keys().collect();
        assert_eq!(running, vec![2, 3], "a 2 segue rodando ao lado da 3");
    }

    /// Duas tarefas que dividem um arquivo e uma terceira que só divide outro
    /// arquivo com a segunda formam um assunto só, pela corrente de arquivos:
    /// saem juntas numa onda.
    #[test]
    fn a_chain_of_tasks_through_their_files_goes_out_in_one_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task_on(root, said, crit, "Mexer no começo.", &["src/a.rs"]);
        let second = backlog_task_on(root, said, crit, "Mexer no meio.", &["src/a.rs", "src/b.rs"]);
        let third = backlog_task_on(root, said, crit, "Mexer no fim.", &["src/b.rs"]);
        let apart = backlog_task_on(root, said, crit, "Mexer em outro assunto.", &["src/c.rs"]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1, 2]));
        assert_eq!(wave_order(root, 1), vec![first, second, third], "a corrente é um assunto só");
        assert_eq!(wave_order(root, 2), vec![apart], "o outro assunto sai à parte");
    }

    /// A onda não tem teto de tarefas: mais de cinco tarefas no mesmo arquivo
    /// saem juntas numa onda só.
    #[test]
    fn more_than_five_tasks_on_one_file_go_out_in_one_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let tasks: Vec<u64> = (1..=7).map(|n| backlog_task(root, said, crit, &format!("Mexer no código {n}."), "src/a.rs")).collect();

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]), "uma onda só");
        assert_eq!(wave_order(root, 1), tasks, "a onda 1 leva as sete tarefas");
    }

    /// O Jev de mentira da montagem: dá a cada tarefa do backlog o tipo que
    /// `kind_of` diz, pelo número da tarefa, com confiança alta.
    fn judging(
        kind_of: impl Fn(u64) -> crate::shared::dag::TaskKind,
    ) -> impl Fn(&crate::shared::judgement::Board) -> Result<crate::shared::judgement::Judged, mustard_core::domain::map_filter::FilterError> {
        move |board| {
            let tasks = board
                .backlog
                .iter()
                .map(|task| {
                    let judgement = crate::shared::dag::Judgement { kind: kind_of(task.id), confidence: 0.9, clash: 0.0 };
                    (task.id, judgement)
                })
                .collect();
            Ok(crate::shared::judgement::Judged { tasks, usage: mustard_core::domain::map_filter::FilterUsage::default() })
        }
    }

    /// Independent small work uses a free slot while another wave runs.
    #[test]
    fn an_independent_small_group_uses_a_free_slot_while_another_wave_runs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Mexer na parte grande.", &SIX_FILES);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]), "seis arquivos saem");
        seed_running(root, 1);
        let small = backlog_task_on(root, said, crit, "Mexer na parte pequena.", &["src/a.rs", "src/b.rs", "src/c.rs"]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![2]));
        assert_eq!(spec_now(root).current(small).and_then(SpecEvent::wave), Some(2));
    }

    /// O mesmo grupo de três arquivos sai quando nenhuma onda está em
    /// andamento.
    #[test]
    fn the_same_group_of_three_files_leaves_when_nothing_runs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let small = backlog_task_on(root, said, crit, "Mexer na parte pequena.", &["src/a.rs", "src/b.rs", "src/c.rs"]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        assert_eq!(wave_order(root, 1), vec![small]);
    }

    /// Independent kinds use separate slots; compatible kinds can pack together.
    ///
    /// Três candidatas, e não duas: a chamada que decide o tipo de cada uma só
    /// sai com mais de duas, e com duas a montagem sairia pela regra de
    /// arquivos, sem tipo nenhum para separar.
    #[test]
    fn independent_kinds_use_slots_and_one_kind_can_pack_together() {
        use crate::shared::dag::TaskKind;
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        // A onda grande em andamento e duas ao lado dela: três vagas.
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        backlog_task_on(root, said, crit, "Mexer na parte grande.", &SIX_FILES);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);
        let one = backlog_task_on(root, said, crit, "Mexer na primeira.", &["src/a.rs", "src/b.rs", "src/c.rs"]);
        let two = backlog_task_on(root, said, crit, "Mexer na segunda.", &["src/d.rs", "src/e.rs", "src/f.rs"]);
        let three = backlog_task_on(root, said, crit, "Mexer na terceira.", &["doc/1.md", "doc/2.md", "doc/3.md"]);

        let apart = judging(|id| if id == one { TaskKind::Defect } else { TaskKind::Feature });
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (Some(&apart), None)).map(|(formed, _)| formed), Ok(vec![2, 3]), "different independent types can use separate slots");

        let together = judging(|_| TaskKind::Feature);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (Some(&together), None)).map(|(formed, _)| formed), Ok(vec![4]));
        assert_eq!(
            wave_order(root, 4),
            vec![log.current(one).unwrap().id, log.current(two).unwrap().id, log.current(three).unwrap().id],
            "as três, do mesmo tipo, numa onda só"
        );
    }

    /// Duas ondas de tipos diferentes que dividem `src/a.rs` saem juntas, ao
    /// lado da onda em andamento: a de depois não espera a de antes.
    ///
    /// Três candidatas, e não duas, pelo mesmo motivo do teste dos tipos
    /// independentes: a chamada que decide o tipo só sai com mais de duas.
    #[test]
    fn a_later_group_sharing_a_file_with_an_earlier_one_leaves_beside_it() {
        use crate::shared::dag::TaskKind;
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        // A onda grande em andamento e duas ao lado dela: três vagas.
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        backlog_task_on(root, said, crit, "Mexer na parte grande.", &SIX_FILES);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);
        let small = backlog_task_on(root, said, crit, "Mexer na parte pequena.", &["src/a.rs", "src/b.rs"]);
        let later = backlog_task_on(root, said, crit, "Mexer na parte seguinte.", &["src/a.rs", "doc/1.md", "doc/2.md", "doc/3.md", "doc/4.md", "doc/5.md"]);
        backlog_task_on(root, said, crit, "Mexer na parte de fora.", &["lib/x.rs"]);

        let judge = judging(|id| if id == small { TaskKind::Defect } else { TaskKind::Feature });
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (Some(&judge), None)).map(|(formed, _)| formed), Ok(vec![2, 3]));
        assert_eq!(spec_now(root).current(small).and_then(SpecEvent::wave), Some(2), "o defeito sai primeiro");
        assert_eq!(spec_now(root).current(later).and_then(SpecEvent::wave), Some(3), "o de seis sai ao lado dele");
    }

    /// A tarefa com o curinga da árvore inteira cruza com todas: sai sozinha,
    /// numa onda só dela, e segura os outros assuntos no backlog enquanto roda.
    #[test]
    fn a_task_with_the_whole_tree_wildcard_goes_out_alone_and_holds_the_rest() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let narrow = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let wide = backlog_task(root, said, crit, "Mexer em todo o código.", "**");

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        assert_eq!(wave_order(root, 1), vec![wide], "o curinga sai sozinho, antes dos outros");
        seed_send(root, 1);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![]), "o resto espera");
        assert_eq!(spec_now(root).current(narrow).and_then(SpecEvent::wave), None, "e segue no backlog");
    }

    /// A tarefa de uma onda desfeita que cai num assunto montado na mesma
    /// rodada ganha uma versão só, com o número da onda nova — não uma versão
    /// sem onda e outra com ela.
    #[test]
    fn a_task_of_an_undone_wave_that_is_packed_again_gets_one_version_with_the_new_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, 2, (None, None)).map(|(formed, _)| formed), Ok(vec![1]));
        let in_wave_one = spec_now(root).current(task).unwrap().id;

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, 2, (None, None)).map(|(formed, _)| formed), Ok(vec![2]), "a onda 1 se desfaz e a 2 a refaz");

        let log = spec_now(root);
        assert_eq!(log.planned_waves().into_iter().collect::<Vec<_>>(), vec![2], "a entrega 1 saiu do plano");
        let now = log.current(task).unwrap();
        assert_eq!(now.wave(), Some(2));
        assert_eq!(now.int("replaces"), Some(in_wave_one), "a versão nova substitui a da onda desfeita, direto");
        let versions = every_line_of(root, "task").iter().filter(|line| line["wave"].is_null()).count();
        assert_eq!(versions, 1, "nenhuma versão sem onda no meio: só a original");
    }

    /// Com uma vaga só, a limpeza fica no backlog enquanto a onda da tarefa
    /// comum roda. Entregue essa onda, a limpeza sai na seguinte, e o
    /// fechamento recusa pela onda dela, ainda sem commit.
    #[test]
    fn closing_refuses_with_a_cleanup_to_do() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        cleanup_task(root, said, crit, "Acertar o comentário de dois.", "src/b.rs");
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let assumed = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(assumed["ok"], json!(true), "{assumed}");

        assert_eq!(crate::commands::flow::close::finished_refusal(&spec_now(root)), Some(("wave-without-commit".into(), Some(2))), "{assumed}");
    }

    /// As versões de tarefa com o código `code` que a leitura mostra.
    fn shown_task_versions(log: &SpecLog, code: &str) -> Vec<u64> {
        let codes = log.codes();
        log.visible().into_iter().filter(|e| e.event_type == "task" && codes.get(&e.id).map(String::as_str) == Some(code)).map(|e| e.id).collect()
    }

    /// A tarefa `task` solta pela rodada numa onda de lote, e a versão dela
    /// que a rodada gravou com o número da onda.
    fn batched(root: &Path, task: u64) -> u64 {
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]), "a tarefa sai na onda 1");
        let log = spec_now(root);
        let version = log.current(task).expect("a tarefa segue viva");
        assert_eq!(version.wave(), Some(1), "a rodada gravou a versão com a onda");
        version.id
    }

    /// O usuário remove, pelo número, a versão que a rodada gravou com a
    /// onda. A tarefa sai inteira: a versão sem onda não volta ao backlog, a
    /// onda que ficou vazia sai do plano, e a rodada seguinte não forma lote.
    #[test]
    fn removing_the_round_version_of_a_task_takes_out_the_whole_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let code = spec_now(root).codes().get(&task).cloned().expect("a tarefa tem código");

        let removed = write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "nenhuma versão da tarefa fica na leitura");
        assert!(!log.planned_waves().contains(&1), "a onda sem tarefa sai do plano");
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![]), "nada volta ao backlog");
    }

    /// A tarefa tem três versões: a sem onda, a da rodada e a do agente, com
    /// texto novo. Remover pelo número a do meio tira as três.
    #[test]
    fn removing_the_middle_version_of_a_task_takes_out_every_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let code = spec_now(root).codes().get(&task).cloned().expect("a tarefa tem código");
        let by_agent = id_of(&write(
            root,
            "x",
            "task",
            json!({"replaces": with_wave, "wave": 1,
            "text": "Mexer de outro jeito no código de um.", "files": [{"path": "src/a.rs"}],
            "depends_on": [], "covers": [crit], "origin": said}),
        ));
        assert_eq!(shown_task_versions(&spec_now(root), &code), vec![by_agent], "a versão do agente é a vigente");

        let removed = write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "as três versões saem");
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![]), "nada volta ao backlog");
    }

    /// A rodada monta a versão nova de uma tarefa com a onda 2, e antes de
    /// ela gravar, o usuário remove a tarefa pelo código. A gravação da
    /// rodada é recusada, e a tarefa segue fora.
    #[test]
    fn the_round_version_written_after_the_removal_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let log = spec_now(root);
        let code = log.codes().get(&task).cloned().expect("a tarefa tem código");
        let draft = task_revision(&log, with_wave, Map::from_iter([("wave".to_string(), json!(2))])).expect("o rascunho");

        let removed = write(root, "x", "remove", json!({"targets": [code], "reason": "Não é mais preciso."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");

        let Err(refusal) = record(root, "x", "task", draft, PhaseWriter::Binary) else { panic!("a versão nova sobre a tarefa removida foi gravada") };
        assert_eq!(refusal.reason(), "replaces-removed", "{refusal:?}");
        let log = spec_now(root);
        assert_eq!(shown_task_versions(&log, &code), Vec::<u64>::new(), "a tarefa segue fora");
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![]), "nada volta ao backlog");
    }

    /// A rodada monta a versão nova de uma tarefa, e antes de ela gravar,
    /// outra gravação revê a tarefa com texto novo. A versão da rodada é
    /// recusada como substituída, e o texto novo segue na leitura.
    #[test]
    fn a_task_revised_after_the_round_reading_keeps_its_new_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let draft = task_revision(&spec_now(root), task, Map::from_iter([("wave".to_string(), json!(1))])).expect("o rascunho");
        let revised = id_of(&write(
            root,
            "x",
            "task",
            json!({"replaces": task, "text": "Mexer de outro jeito no código de um.",
            "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [crit], "origin": said}),
        ));

        let Err(refusal) = record(root, "x", "task", draft, PhaseWriter::Binary) else { panic!("a versão da rodada passou por cima do texto novo") };
        assert_eq!(refusal.reason(), "replaces-superseded", "{refusal:?}");
        let log = spec_now(root);
        assert_eq!(log.current(task).map(|e| e.id), Some(revised), "a versão com texto novo segue vigente");
    }

    /// Com a tarefa removida pela versão da rodada e outra tarefa revista uma
    /// vez, cada versão que a rodada seguinte grava aponta a versão mais nova
    /// do mesmo código.
    #[test]
    fn the_round_version_replaces_the_newest_version_of_the_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        write(root, "x", "remove", json!({"targets": [with_wave], "reason": "Não é mais preciso."}));
        let other = backlog_task(root, said, crit, "Mexer no código de dois.", "src/b.rs");
        id_of(&write(
            root,
            "x",
            "task",
            json!({"replaces": other, "text": "Mexer de outro jeito no código de dois.",
            "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [crit], "origin": said}),
        ));

        let before = spec_now(root);
        let codes = before.codes();
        let newest = |code: &str| before.events.iter().filter(|e| codes.get(&e.id).map(String::as_str) == Some(code)).map(|e| e.id).max();
        assert_eq!(dispatch_backlog(root, "x", &before, &before, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![2]), "a outra tarefa sai na onda 2");

        let after = spec_now(root);
        let written: Vec<&SpecEvent> = after.events.iter().filter(|e| e.id > before.max_id() && e.event_type == "task").collect();
        let after_codes = after.codes();
        for version in &written {
            let code = after_codes.get(&version.id).expect("a versão tem código");
            assert_eq!(version.int("replaces"), newest(code), "a versão da rodada aponta a mais nova de {code}");
        }
        assert_eq!(written.len(), 1, "só a outra tarefa ganha versão nova");
    }

    /// A versão de tarefa que a rodada monta leva o autor `binary`, ainda que
    /// a versão anterior seja de outro autor; os demais campos seguem os da
    /// anterior, e quem passa `author` em `extra` vence.
    #[test]
    fn the_round_task_version_carries_the_binary_author() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let log = spec_now(root);
        let before = log.get(task).expect("a tarefa gravada");
        assert_ne!(before.str_field("author"), Some("binary"), "a versão de partida não é da rodada");

        let draft = task_revision(&log, task, Map::from_iter([("wave".to_string(), json!(1))])).expect("o rascunho");
        assert_eq!(draft["author"], json!("binary"));
        let mut kept = before.fields.clone();
        for key in ["v", "id", "code", "at", "type", "search", "author"] {
            kept.remove(key);
        }
        let mut written = draft.clone();
        for key in ["replaces", "wave", "author"] {
            written.remove(key);
        }
        assert_eq!(written, kept, "os demais campos são os da versão anterior");

        let asked = task_revision(&log, task, Map::from_iter([("author".to_string(), json!("review"))])).expect("o rascunho");
        assert_eq!(asked["author"], json!("review"), "o `extra` vence");
    }

    /// A versão que a rodada grava ao soltar o lote do backlog sai com o autor
    /// `binary`, e a tarefa segue com os mesmos arquivos e o mesmo texto.
    #[test]
    fn the_version_written_by_the_batch_release_is_by_the_binary() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        assert_ne!(spec_now(root).get(task).and_then(|e| e.str_field("author")), Some("binary"));

        let with_wave = batched(root, task);
        let log = spec_now(root);
        let version = log.get(with_wave).expect("a versão da rodada");
        assert_eq!(version.str_field("author"), Some("binary"), "{version:?}");
        assert_eq!(version.str_field("text"), Some("Mexer no código de um."));
    }

    /// Numa spec antiga, a remoção de só a versão com onda devolveu a versão
    /// sem onda ao backlog. A rodada não a solta de novo: a versão nova
    /// aponta a versão removida, e a gravação a recusa sem gravar nada.
    #[test]
    fn an_old_removal_of_the_round_version_does_not_send_the_task_again() {
        use std::io::Write as _;

        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let path = store::spec_file(root, "x").unwrap();
        let by = spec_now(root).max_id() + 1;
        let old_removal = json!({"v": 1, "id": by, "at": "2026-09-20T10:00:00-03:00", "type": "remove",
            "targets": [with_wave], "reason": "Não é mais preciso.", "gives_back": true, "author": "assistant"});
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{old_removal}").unwrap();

        let log = spec_now(root);
        assert_eq!(log.current(task).map(|e| e.id), Some(task), "a leitura antiga devolve a versão sem onda");
        assert_eq!(
            dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed),
            Err(mustard_core::domain::spec_events::Refusal::ReplacesRemoved { id: with_wave, by }),
            "a rodada não solta a tarefa removida"
        );
        assert_eq!(spec_now(root).max_id(), by, "nada foi gravado");
    }

    /// A tarefa de uma onda de lote que ainda não saiu ganha, por uma versão
    /// nova pela porta do modelo, a dependência de uma tarefa do backlog. Com
    /// a onda, a gravação é recusada com o conserto, e nada é gravado; sem a
    /// onda, passa, e a rodada seguinte solta a dependência antes dela, no
    /// mesmo lote.
    #[test]
    fn a_wave_task_that_gains_a_backlog_dependency_goes_out_after_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let task = backlog_task(root, said, crit, "Mexer no código de um.", "src/a.rs");
        let with_wave = batched(root, task);
        let dependency = backlog_task(root, said, crit, "Preparar o código de um.", "src/a.rs");
        let version = |wave: Option<u64>| {
            let mut body = json!({"replaces": with_wave, "text": "Mexer no código de um.", "files": [{"path": "src/a.rs"}],
                "depends_on": [dependency], "covers": [crit], "origin": said});
            if let Some(n) = wave {
                body["wave"] = json!(n);
            }
            write(root, "x", "task", body)
        };

        let before = spec_now(root).max_id();
        let refused = version(Some(1));
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("depends-outside-wave"), "{refused}");
        assert!(refused["hint"].as_str().is_some_and(|hint| hint.contains("sem wave")), "{refused}");
        assert_eq!(spec_now(root).max_id(), before, "nada foi gravado");

        let accepted = id_of(&version(None));
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
        assert_eq!(wave_order(root, 2), vec![dependency, accepted], "a dependência sai antes da tarefa");
    }

    /// Três tarefas do backlog no mesmo arquivo, cada uma esperando a
    /// anterior: o lote leva as três, na ordem da cadeia, e a versão com a
    /// onda de cada uma passa na conferência da gravação.
    #[test]
    fn a_backlog_batch_with_a_chain_inside_it_passes_the_check() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task(root, said, crit, "Preparar o código de um.", "src/a.rs");
        let after = |on: u64, text: &str| {
            id_of(&write(
                root,
                "x",
                "task",
                json!({"text": text, "files": [{"path": "src/a.rs"}], "depends_on": [on],
                "covers": [crit], "origin": said}),
            ))
        };
        let second = after(first, "Mexer no código de um.");
        let third = after(second, "Testar o código de um.");

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).map(|(formed, _)| formed), Ok(vec![1]), "a cadeia vira um lote só");
        assert_eq!(wave_order(root, 1), vec![first, second, third], "na ordem da cadeia");
        let log = spec_now(root);
        for task in [first, second, third] {
            assert_eq!(log.current(task).and_then(SpecEvent::wave), Some(1), "a tarefa {task} ganhou a onda");
        }
    }

    /// O backlog da onda que volta sem fazer uma das tarefas, depois do
    /// "Aceitar" do usuário na mudança de plano dela.
    pub(crate) struct UndoneReturn {
        /// O código de cada tarefa: A, B, C e D.
        pub(crate) a: String,
        pub(crate) b: String,
        pub(crate) c: String,
        pub(crate) d: String,
        /// O código da decisão que só B cobre.
        pub(crate) b_decision: String,
        /// A mudança de plano que a onda propôs.
        pub(crate) change: String,
        /// O aviso da rodada que segurou a onda pela mudança, antes do clique.
        pub(crate) stopped: Value,
        /// A resposta da rodada que assumiu a volta, depois do clique.
        pub(crate) accepted: Value,
    }

    /// A versão vigente da tarefa de código `code` na spec `x`.
    pub(crate) fn task_now(root: &Path, code: &str) -> SpecEvent {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        log.visible()
            .into_iter()
            .find(|e| e.event_type == "task" && codes.get(&e.id).map(String::as_str) == Some(code))
            .cloned()
            .unwrap_or_else(|| panic!("sem a tarefa {code}"))
    }

    /// Um backlog com quatro tarefas: A cobre uma decisão e B cobre outra, e
    /// as duas dividem `src/a.rs`; C, em `src/c.rs`, depende de B, e D, em
    /// `src/d.rs`, depende de A. Nem C nem D divide arquivo com A ou com B, e
    /// por isso nenhuma entra na onda de A e B pela espera. A primeira rodada
    /// leva A e B numa onda só, o assunto de `src/a.rs`; a onda volta com A
    /// feita e B por fazer, com a mudança de plano, a rodada para, o usuário
    /// clica em "Aceitar", e a rodada seguinte assume a volta.
    pub(crate) fn return_with_an_undone_task(root: &Path) -> UndoneReturn {
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["c.rs", "d.rs"] {
            std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
        }
        let decisions = std::cell::Cell::new((0, 0));
        let (said, crit) = backlog_project_with(root, |said| {
            let decision = |text: &str| id_of(&write(root, "x", "decision", json!({"text": text, "why": "w", "keys": ["k"], "origin": said})));
            decisions.set((decision("A soma arredonda para baixo."), decision("A lista vazia soma zero.")));
        });
        let (for_a, for_b) = decisions.get();
        let task = |text: &str, file: &str, covers: Vec<u64>, depends: Vec<u64>| {
            id_of(&write(
                root,
                "x",
                "task",
                json!({"text": text, "files": [{"path": file}], "depends_on": depends,
                "covers": covers, "origin": said}),
            ))
        };
        let a = task("Arredondar a soma.", "src/a.rs", vec![crit, for_a], vec![]);
        let b = task("Somar a lista vazia.", "src/a.rs", vec![crit, for_b], vec![]);
        let c = task("Mostrar a soma da lista vazia.", "src/c.rs", vec![crit], vec![b]);
        let d = task("Mostrar a soma arredondada.", "src/d.rs", vec![crit], vec![a]);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let code = |id: u64| codes[&id].clone();
        let (a, b, c, d, b_decision) = (code(a), code(b), code(c), code(d), code(for_b));

        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let batch: Vec<String> = [&a, &b, &c, &d].into_iter().filter(|task| task_now(root, task).wave() == Some(1)).cloned().collect();
        assert_eq!(batch, vec![a.clone(), b.clone()], "a onda leva A e B: {first}");

        let session = "s-tarefa-nao-feita";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma arredonda.\n").unwrap();
        let change = "B precisa de uma decisão sobre a lista vazia antes.";
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let agreed: Vec<Value> = super::super::agreed::request_agreed(&log, 1)
            .iter()
            .map(|item| {
                let code = log.codes()[&item.id].clone();
                if code == b_decision { json!({"item": code, "met": false, "text": "B não foi feita."}) } else { json!({"item": code, "met": true}) }
            })
            .collect();
        assert_eq!(agreed.len(), 2, "o pedido leva as duas decisões: {agreed:?}");
        let back = json!({"wave": 1, "text": "A saiu; B ficou por fazer.", "files": ["src/a.rs"],
            "commit": "a soma arredonda", "replan": change, "changes_decision": DECISION,
            "undone": [b], "agreed": agreed});
        let wrote = returned(root, back);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");

        let stopped = change_asked(&round(root, "x", None));
        assert_eq!(stopped["wave"], json!(1), "{stopped}");
        let question = QUESTION.to_string();
        let header = stopped["header"].as_str().unwrap_or_default().to_string();
        click(root, session, &question, &header, "Aceitar");
        let accepted = round(root, "x", None);
        assert_eq!(accepted["ok"], json!(true), "{accepted}");
        UndoneReturn { a, b, c, d, b_decision, change: change.to_string(), stopped, accepted }
    }

    /// A tarefa que a onda não fez volta ao backlog sem a onda que a levou,
    /// e só sai na rodada seguinte à que a soltou. A tarefa que depende dela
    /// segue esperando, e a que dependia só da tarefa feita sai, no assunto
    /// dela, numa onda ao lado da devolvida. A onda entregue não sai de novo.
    #[test]
    fn a_task_the_wave_did_not_do_goes_back_to_the_queue_and_holds_its_dependents() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let UndoneReturn { a, b, c, d, accepted, .. } = return_with_an_undone_task(root);

        assert_eq!(task_now(root, &a).wave(), Some(1), "A segue na onda que a fez");
        let returned_b = task_now(root, &b);
        assert_eq!(returned_b.wave(), None, "B voltou sem onda: {:?}", returned_b.fields);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(backlog_left(&log).contains(&returned_b.id), "B está no backlog");
        assert_eq!(waves_in(&accepted, "dispatch"), vec![2], "D foi liberada por A nesta chamada: {accepted}");
        assert_eq!(task_now(root, &d).wave(), Some(2));
        assert_eq!(task_now(root, &b).wave(), None, "o corte de B espera outra chamada");
        assert_eq!(task_now(root, &c).wave(), None, "C ainda depende de B");
        std::fs::write(root.join("src/d.rs"), "fn one() {}\n// D terminou.\n").unwrap();
        returned(root, json!({"wave":2,"text":"D saiu.","files":["src/d.rs"],"commit":"mostrar soma"}));
        let next = round(root, "x", None);
        assert_eq!(task_now(root, &b).str_field("blocked_by"), None, "a rodada seguinte ao aceite solta B: {next}");
        assert_eq!(waves_in(&next, "dispatch"), vec![3], "sai B, nunca a onda entregue: {next}");
        assert_eq!(task_now(root, &b).wave(), Some(3));
        assert_eq!(task_now(root, &a).wave(), Some(1));
        assert_eq!(task_now(root, &c).wave(), None);

        let quiet = round(root, "x", None);
        assert!(!waves_in(&quiet, "dispatch").contains(&1), "a onda entregue não sai de novo: {quiet}");
    }
    #[test]
    fn the_actual_dispatched_request_carries_root_and_touched_directory_rules_in_full() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        super::super::tests::approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("CLAUDE.md"), "# Root\nPreserve root-contract in full.\n").unwrap();
        std::fs::write(root.join("src/CLAUDE.md"), "# Source\nPreserve source-contract in full.\n").unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/CLAUDE.md"), "Unrelated directory contract.").unwrap();
        let sent = round(root, "x", None);
        assert_eq!(sent["ok"], true, "{sent}");
        let request = super::super::tests::request_at(&sent, 0);
        assert!(request.contains("Preserve root-contract in full."), "{request}");
        assert!(request.contains("Preserve source-contract in full."), "{request}");
        assert!(!request.contains("Unrelated directory contract."), "{request}");
    }


    /// Duas ondas saem, uma num arquivo TypeScript e outra num C#, e as duas
    /// estão em andamento. A única tarefa da onda 1 é removida pelo programa
    /// (a triagem tira assim as tarefas de uma onda parada; a remoção do `run
    /// write` seria recusada enquanto o agente trabalha), sem entrega e com o
    /// pedido ainda aberto: só a remoção tira a onda 1 do andamento. A rodada
    /// com relatório já não a lista em andamento e grava a nota do binário
    /// que diz por quê; a rodada de despacho seguinte também não a lista e
    /// não grava outra nota; a leitura do andamento e a das vagas ocupadas,
    /// que a retomada e a fila usam, também a deixam de fora.
    #[test]
    fn a_wave_whose_tasks_were_all_removed_leaves_the_air_in_every_round() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["web/app.ts"], &[]), (2, &["src/Program.cs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let log = spec_now(root);
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).unwrap().id;
        let code = log.codes().get(&task).cloned().unwrap();
        let send = log.codes().get(&log.last_by_wave("send")[&1]).cloned().unwrap();
        assert_eq!(waves_in_progress(&log).into_keys().collect::<Vec<_>>(), vec![1, 2], "antes da remoção as duas ondas estão em andamento");
        let removal = json!({"targets": [code], "reason": "Já foi feito fora da onda."});
        let refused = write(root, "x", "remove", removal.clone());
        assert_eq!(refused["reason"], json!("wave-agent-working"), "o run write recusa enquanto o agente trabalha: {refused}");
        record(root, "x", "remove", removal.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
        let log = spec_now(root);
        assert!(!log.waves_returned().contains(&1), "o agente da onda 1 não voltou");
        assert_eq!(log.waves_left_without_tasks().into_keys().collect::<Vec<_>>(), vec![1], "a remoção tirou a onda 1 do ar");

        let notes = || -> Vec<String> {
            spec_now(root)
                .visible()
                .into_iter()
                .filter(|e| e.event_type == "note" && e.str_field("author") == Some("binary"))
                .filter_map(|e| e.str_field("text").map(str::to_string))
                .collect()
        };
        let said = translate("round.wave_left_without_tasks", Locale::PtBr).replace("{wave}", "1").replace("{send}", &send);

        let reported = round(root, "x", Some(&line("USAGE", json!({"wave": 2}))));
        assert_eq!(reported["ok"], json!(true), "{reported}");
        assert_eq!(waves_in(&reported, "running"), vec![2], "a onda sem tarefa sai do ar na rodada com relatório: {reported}");
        assert_eq!(notes(), vec![said.clone()], "a rodada grava a nota que diz por quê: {reported}");
        let recorded = reported["recorded"].as_array().cloned().unwrap_or_default();
        assert!(recorded.iter().any(|r| r["type"] == json!("note") && r["wave"] == json!(1)), "{reported}");

        let dispatched = round(root, "x", None);
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");
        assert_eq!(waves_in(&dispatched, "running"), vec![2], "nem a rodada de despacho a lista: {dispatched}");
        assert_eq!(notes(), vec![said], "a nota sai uma vez só: {dispatched}");

        let log = spec_now(root);
        assert_eq!(waves_in_progress(&log).into_keys().collect::<Vec<_>>(), vec![2]);
        assert_eq!(open_sends(&log).into_keys().collect::<Vec<_>>(), vec![2]);
    }
    /// A remoção da única tarefa de uma onda enviada, depois da entrega do
    /// agente, responde com o número dessa onda, que sai do ar com ela, e
    /// nunca com a onda que segue com tarefa. A remoção seguinte, de uma
    /// tarefa do backlog, fora de qualquer onda, responde sem onda nenhuma: a
    /// onda que já tinha saído do ar não volta à lista. O projeto é em
    /// TypeScript e C#.
    #[test]
    fn removing_the_only_task_of_a_sent_wave_answers_with_that_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["web/app.ts"], &[]), (2, &["src/Program.cs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let log = spec_now(root);
        let first_of = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        let loose = backlog_task_on(root, first_of("message"), first_of("criterion"), "Mexer na tela de fora.", &["web/other.ts"]);
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).unwrap().id;

        delivered(root, 1, "Saiu.", &["web/app.ts"]);
        let removed = write(root, "x", "remove", json!({"targets": [task], "reason": "Já foi feito fora da onda."}));
        assert_eq!(removed["ok"], json!(true), "{removed}");
        assert_eq!(removed["waves_left_without_tasks"], json!([1]), "a remoção diz que a entrega 1 saiu do ar: {removed}");

        let outside = write(root, "x", "remove", json!({"targets": [loose], "reason": "Já foi feito fora da obra."}));
        assert_eq!(outside["ok"], json!(true), "{outside}");
        assert!(outside.get("waves_left_without_tasks").is_none(), "a tarefa de fora de qualquer onda não tira onda do ar: {outside}");
    }
}
