//! `mustard-rt run read <bloco> [--spec <nome>]` — devolve só o bloco pedido
//! do arquivo de eventos da spec, sem os itens removidos ou substituídos e sem
//! o campo `search`.
//!
//! Sem `--spec`, lê a spec atual, pela mesma escada de todas as portas: a
//! variável `MUSTARD_ACTIVE_SPEC`, depois a branch do checkout, depois a spec
//! ligada à sessão. Sem nenhuma, recusa.
//!
//! A saída é um JSON com um evento por linha:
//!
//! ```text
//! {"ok":true,"spec":"teste","block":"wave-2","count":2,"events":[
//! {"code":"MSTD-WAVE-0002","type":"wave",…},
//! {"code":"MSTD-TASK-0002","type":"task",…}
//! ]}
//! ```
//!
//! **O bloco sai na forma enxuta** ([`super::lean`]): cada item com o
//! conteúdo e as ligações pelo código, sem a versão do formato, a sessão, o
//! número, a hora e o autor fora da conversa, e sem as marcas de conferência
//! dos fatos; o fato repetido sai uma vez. O mesmo vale para `delivered-<n>`
//! e para `dispatch-<n>` lido de fora da cópia. `item-<código>` e
//! `item-<número>` seguem com o evento inteiro.
//!
//! **Quem conduz lê com teto.** Um bloco sai do mais novo para o mais
//! antigo — menos o `state`, que é o retrato de agora, e a `wave-<n>`, que é
//! a história de uma onda e sai na ordem do arquivo. Sem `--all`, o bloco
//! mostra só os [`BLOCK_CAP`] mais novos, e o campo `more` diz "N de M"; a
//! `wave-list` mostra só as ondas abertas; e o `metrics` com um termo que não
//! é código de item vira um resumo por tipo. Quem lê por necessidade — o
//! agente com o pedido, a rodada, o fechamento, o painel — lê o pedido, o
//! item, a onda ou o `--all`, nunca o bloco cortado.
//!
//! Uma linha do arquivo que não se entende entra em `warnings`, no idioma do
//! projeto, e o resto é lido.
//!
//! Com `--term`, a leitura devolve o que o termo acha, do mais forte para o
//! menos forte: um código de item devolve aquele item, o nome de um comando
//! (`map search`) devolve as chamadas dele, e qualquer outro termo passa pela
//! busca por nota, que não exige que o item tenha todas as palavras.
//!
//! O bloco `lessons` foge dessa regra: fica fora da spec, vive no banco do
//! projeto (`.claude/spec/lessons.ndjson`) e o `--term` é o número da lição
//! no banco, não uma busca. É assim que o pedido de uma onda leva a lição só
//! pelo número dela, sem copiar o texto.
//!
//! `dispatch-<n>` também foge: não é um bloco, é tudo o que o pedido da onda
//! `n` lista — os itens da spec, cada um com o código, e as lições do banco —,
//! pela lista que o envio gravou, numa leitura só. `dispatch-review` faz o
//! mesmo com o pedido mais novo do revisor final. O `--term` filtra dentro
//! disso.
//!
//! Cinco leituras respondem o que antes saía de script escrito na hora, e
//! nelas o `--term` não vale:
//!
//! - `wave-list` traz uma linha curta por onda, na ordem do número: o código,
//!   o número, a primeira linha do texto e as ondas de que ela depende. É o
//!   que quem conduz tirava do bloco `waves` inteiro, que traz também as
//!   tarefas, os pedidos e as entregas de todas as ondas.
//! - `item-<código>` traz a versão vigente do item, e `item-<número>`, aquela
//!   versão; as duas com `changed`, o que mudou da versão anterior. O item
//!   removido vem com `removed`, e a versão já substituída, com
//!   `replaced_by`. A mensagem do usuário sai com o texto inteiro, como
//!   qualquer outro item.
//! - `delivered-<n>` traz a entrega vigente da onda: a que a rodada assumiu
//!   ou, sem ela, a volta do agente. Do `agreed`, só o que não foi cumprido.
//! - `backlog` traz as tarefas ainda por entregar, uma por linha, e no fim o
//!   total de tarefas e de arquivos. A tarefa presa sem arquivo nenhum, que
//!   não sai numa onda até ganhar arquivos e dependências, vem marcada com
//!   `needs_planning`.
//! - `request-<n>` devolve, em texto puro, o pedido gravado no último envio da
//!   onda, igual byte a byte: é por ele que o agente lê o próprio pedido.
//!   `request-review` faz o mesmo com o último envio do revisor final, que
//!   não tem onda: o fechamento manda o revisor ler o pedido por ele.
//!   `request-review-preview` devolve, também em texto puro, o pedido que o
//!   fechamento daria ao revisor final agora, pela mesma montagem dele
//!   ([`final_review`]), sem gravar envio nem leitura: é por ele que o
//!   usuário vê o pedido real antes de ele valer.
//!
//! `calls` soma as chamadas de cada comando, uma linha por comando: quantas,
//! as falhas por motivo, a mediana, o p90 e o pior do tempo da chamada e do
//! tempo do filtro, e os tokens e o custo somados. O `--term` nela é o nome do
//! comando, e a linha dele vem só.
//!
//! O bloco `state` traz também o número da última mensagem do usuário, sem o
//! texto, que é o `origin` de quem grava a partir dela.
//!
//! **O painel sem termo vem sem o texto longo.** `metrics`, lido sem `--term`,
//! traz o envio, a injeção e a entrega sem o `text`: o texto que o gancho
//! colocou na conversa e o relato da entrega eram mais da metade da saída, e
//! quem lê o painel olha o comando, o tempo, as fichas e o motivo do bloqueio,
//! nunca esses textos. O campo `chars` diz o tamanho da injeção, e o texto
//! inteiro de cada uma sai por `item-<código>`; o da entrega, também por
//! `delivered-<n>`. Com `--term`, o leitor pede o conteúdo, e o texto vem.
//! `waves` segue com tudo: quem conduz lê ali o `text`, os `files`, o
//! `done_when` e o `agreed`; para só escolher a ordem das ondas, basta a
//! `wave-list`.
//!
//! **Quem trabalha na cópia de um pedido lê a linha do item sem as marcas do
//! binário e sem as palavras de busca.** De dentro da cópia de um pedido
//! aberto, `item-<código>`, `item-<número>` e `dispatch-<n>` deixam de fora
//! `v`, `at`, `author` e `keys` — e o `author` e o `keys` de dentro de
//! `changed` —, que ninguém lê ali: nenhuma instrução manda o agente usá-los,
//! o item novo que ele grava leva só `title`, `text` e `agent`, e nas
//! conversas guardadas as palavras de busca de um item lido por agente não
//! voltam em nada que ele escreve. Quem conduz a spec, de fora da cópia, lê a
//! linha inteira, porque a versão nova de um item repete o `keys` dele. A
//! mensagem do usuário guarda o `at` e o `author`: dizem quem falou e quando.
//! O `id`, o `origin`, o `replaces` e o `covers` ficam: são números que se
//! seguem com `item-<número>`.
//!
//! **A leitura do pedido fica registrada.** Quem lê `item-<código>`,
//! `item-<número>`, `lessons --term <número>` ou `dispatch-<n>` de dentro da
//! cópia de um pedido aberto (a pasta atual é a vaga gravada no envio, ou
//! uma pasta dentro dela) deixa um recibo: uma chamada `read` com o pedido e
//! todos os itens achados, numa gravação só (`read_record`). A leitura de
//! outra pasta, e a que não acha nada, não grava nada. É desses recibos que a
//! entrega da onda e o veredito da revisão final conferem que o agente leu
//! tudo o que o pedido lista.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use mustard_core::ClaudePaths;
use mustard_core::domain::lessons::kept;
use mustard_core::domain::mustard_id;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{
    Block, BlockQuery, EventRef, Hidden, NOT_SEARCHED, ReadQuery, Refusal, SpecEvent, SpecLog, calls_command, found_by, shown_line,
};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::io::spec_events as store;
use mustard_core::domain::wave_prompt::{item_body, item_title};
use mustard_core::io::wave_prompt::{final_review, lesson_bank, request_items};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::read_record::{self, Request};
use crate::commands::flow::round::task_release::held_in_wave;
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

/// Options for `mustard-rt run read`.
pub struct ReadOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec pedida; sem ela, a spec atual.
    pub spec: Option<String>,
    pub block: String,
    pub term: Option<String>,
    /// Só os registros desta sessão do Claude Code, em `calls` e `metrics`.
    /// Sem ela, os de todas: duas conversas no mesmo projeto gravam na mesma
    /// spec, e o consumo das duas saía somado.
    pub session: Option<String>,
}

/// Quantos eventos um bloco mostra a quem conduz, sem `--all`: os mais
/// novos. Um bloco inteiro chegava a 4,6 MB e era lido em pedaços; trinta
/// eventos cabem numa leitura, e a linha "N de M" diz que há mais.
pub(crate) const BLOCK_CAP: usize = 30;

/// Quantos códigos cada linha do resumo da busca no painel traz.
const SUMMARY_CODES: usize = 5;

/// Até onde a leitura vai.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Tudo: o `--all` e quem lê por dentro do programa.
    All,
    /// A leitura de quem conduz: o bloco até [`BLOCK_CAP`], a lista das
    /// ondas só com as abertas e a busca com termo no painel em resumo.
    Capped,
}

/// O núcleo testável de [`run`] com `--all`: a saída inteira, ou a recusa. A
/// sessão vem do ambiente, e a pasta de onde se lê, da pasta atual. Nunca
/// entra em pânico.
pub(crate) fn read_at(opts: &ReadOpts) -> Result<String, Value> {
    let from = std::env::current_dir().unwrap_or_else(|_| opts.root.clone());
    read_for(opts, session_from_env().as_deref(), &from)
}

/// [`read_at`] com o teto de quem conduz ([`Reach::Capped`]): é a leitura de
/// [`run`] sem `--all`.
fn read_capped(opts: &ReadOpts) -> Result<String, Value> {
    let from = std::env::current_dir().unwrap_or_else(|_| opts.root.clone());
    read_reach(opts, session_from_env().as_deref(), &from, Reach::Capped)
}

/// [`read_at`] com a sessão e a pasta recebidas, que é como um teste as
/// escolhe. A pasta `from` diz de qual pedido é a leitura: dentro da cópia de
/// um pedido aberto, o que a leitura acha fica registrado como lido.
pub(crate) fn read_for(opts: &ReadOpts, session: Option<&str>, from: &Path) -> Result<String, Value> {
    read_reach(opts, session, from, Reach::All)
}

/// [`read_for`] até onde `reach` diz.
fn read_reach(opts: &ReadOpts, session: Option<&str>, from: &Path, reach: Reach) -> Result<String, Value> {
    let started = Instant::now();
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let block = opts.block.trim();
    if block == "lessons" {
        let (report, found) = read_lessons(&project.root, opts.spec.as_deref(), opts.term.as_deref(), lang)?;
        if !found.is_empty()
            && let Some((spec, log)) = quiet_spec(opts, session, &project.root)
        {
            let read: Vec<String> = found.iter().map(|id| format!("lesson-{id}")).collect();
            let request = read_record::request_of(&log, from);
            note_reading(&project.root, &spec, request.as_ref(), None, &read, started);
        }
        return Ok(report);
    }
    // O nome que o agente chutou vale pelo bloco que ele quis, e a resposta
    // diz o nome certo: cinquenta e uma recusas eram só isso, e cada uma
    // custava uma volta inteira do agente.
    let alias = ReadQuery::alias_of(block);
    let asked = alias.unwrap_or(block);
    let reading = ReadQuery::parse(asked).ok_or_else(|| refuse(Refusal::UnknownBlock { found: block.to_string() }))?;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session).ok_or_else(|| refuse(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(&project.root, &spec).map_err(refuse)?;
    let Some(log) = store::read(&path).map_err(refuse)? else {
        return Err(refuse(Refusal::NoSpecFile { spec }));
    };
    let codes = log.codes();
    let term = opts.term.as_deref().unwrap_or_default();
    // Quem lê de dentro da cópia de um pedido aberto lê para implementar.
    let request = read_record::request_of(&log, from);
    let item_view = if request.is_some() { Shown::Agent } else { Shown::Whole };
    // O que a leitura dá além dos eventos, depois da lista.
    let mut extra: Vec<(&str, Value)> = Vec::new();
    // O que a leitura achou do pedido, para o registro da leitura, e a onda a
    // que ele se restringe: o `dispatch-<n>` só conta na cópia da onda `n`.
    let mut read: Vec<String> = Vec::new();
    let mut only_wave: Option<u64> = None;
    // A linha que diz o que a leitura de quem conduz deixou de fora.
    let mut more: Option<String> = None;
    // Os eventos saem na forma enxuta ([`super::lean`]); o item lido pelo
    // código ou pelo número, e as linhas curtas da lista das ondas, do
    // backlog e das chamadas, saem como são.
    let mut lean = false;
    let events: Vec<String> = match reading {
        ReadQuery::Request(wave) => return request_text(&log, wave).ok_or_else(|| no_request(Some(wave), lang)),
        ReadQuery::ReviewRequest => return review_request_text(&log).ok_or_else(|| no_request(None, lang)),
        ReadQuery::WaveReviewRequest(wave) => return crate::commands::flow::round::wave_review_text(&log, wave).ok_or_else(|| no_request(Some(wave), lang)),
        ReadQuery::ReviewPreview => return Ok(final_review(&project.root, &spec, &log, lang).text),
        ReadQuery::ReviewDispatch => {
            let mut listed = review_listed(&log).ok_or_else(|| no_request(None, lang))?;
            // A cópia de uma onda não lê por aqui o que ela ainda segura.
            if let Some(Request { wave: Some(wave), .. }) = &request {
                let held = held_in_wave(&log, *wave);
                listed.retain(|code| !held.contains_key(code));
            }
            let (lines, found) = listed_lines(&project.root, &log, &listed, term, &codes, &project.languages, item_view, lang);
            // De dentro da cópia do revisor, a leitura sai em texto e grava
            // um recibo só; a cópia de uma onda não lê o pedido do revisor.
            if item_view == Shown::Agent {
                let review = request.as_ref().filter(|request| request.wave.is_none());
                note_reading(&project.root, &spec, review, None, &found, started);
                return Ok(lines.join("\n\n"));
            }
            lean = true;
            lines
        }
        ReadQuery::Dispatch(wave) => {
            // De dentro da cópia da onda, só o que as tarefas já liberadas
            // trazem: as outras chegam, uma por vez, na resposta do passo.
            let held = match &request {
                Some(Request { wave: Some(own), .. }) if *own == wave => held_in_wave(&log, wave),
                _ => BTreeMap::new(),
            };
            let (lines, listed) = dispatch_lines(&project.root, &log, wave, term, &codes, &project.languages, item_view, lang, &held);
            // De dentro da cópia, a leitura da onda sai em texto, um bloco por
            // item.
            if item_view == Shown::Agent {
                note_reading(&project.root, &spec, request.as_ref(), Some(wave), &listed, started);
                return Ok(lines.join("\n\n"));
            }
            read = listed;
            only_wave = Some(wave);
            lean = true;
            lines
        }
        ReadQuery::Item(target) => {
            let found = find_item(&log, &target, &codes);
            // De dentro da cópia da onda, ler adiante é recusado: a tarefa
            // ainda não liberada, ou o item que só ela traz, chega na resposta
            // do passo que termina a tarefa atual.
            if let (Some(Request { wave: Some(wave), .. }), Some(event)) = (&request, found) {
                let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
                if let Some(task) = held_in_wave(&log, *wave).remove(&code) {
                    return Err(refuse(Refusal::TaskNotReleased { task }));
                }
            }
            read.extend(found.map(|event| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string())));
            found.map(|event| item_line(&log, event, &codes, item_view)).into_iter().collect()
        }
        ReadQuery::WaveList => {
            let (lines, total) = wave_list_lines(&log, &codes, reach == Reach::Capped);
            if lines.len() < total {
                let shown = lines.len().to_string();
                more = Some(translate("spec_events.read_open_waves", lang).replace("{shown}", &shown).replace("{total}", &total.to_string()));
            }
            lines
        }
        ReadQuery::Delivered(wave) => {
            lean = true;
            delivered_lines(&log, wave, &codes)
        }
        ReadQuery::Backlog => {
            let (lines, total) = backlog_lines(&log, &codes);
            extra.push(("total", total));
            lines
        }
        ReadQuery::Calls => {
            let seen = super::conversation::observed(&project.root, &spec, &log);
            let seen: Vec<SpecEvent> = seen.into_iter().filter(|event| of_session(event, opts.session.as_deref())).collect();
            calls_lines(&seen, term)
        }
        ReadQuery::Block(query) => {
            lean = true;
            if query == BlockQuery::Block(Block::State)
                && let Some(id) = last_user_message(&log)
            {
                extra.push(("last_user_message", json!(id)));
            }
            // O pedido de cada envio é o texto maior da spec: a lista das
            // ondas e o painel mostram o envio sem ele, e só a leitura de uma
            // onda o traz inteiro. O painel sem termo tira também o texto da
            // injeção e da entrega.
            let shown = match query {
                BlockQuery::Block(Block::Metrics) if term.trim().is_empty() => Shown::Panel,
                BlockQuery::Block(Block::Waves | Block::Metrics) => Shown::Plan,
                _ => Shown::Whole,
            };
            if let Some(text) = text_search(&log, &codes, query, term, &project, &spec) {
                return Ok(text);
            }
            let found = found_by(log.block(query), term, &codes, &project.languages);
            let found = if query == BlockQuery::Block(Block::State) { latest_copies(found) } else { found };
            // Só os registros da sessão pedida, quando ela foi pedida.
            let found: Vec<&SpecEvent> = found.into_iter().filter(|event| of_session(event, opts.session.as_deref())).collect();
            let mut found: Vec<(&SpecEvent, &BTreeMap<u64, String>)> = found.into_iter().map(|e| (e, &codes)).collect();
            // O painel junta o que o programa observou de si e mora no arquivo
            // de operações, cada evento com o código que ele tem lá, na ordem
            // da hora com os da spec.
            let operations = if query == BlockQuery::Block(Block::Metrics) {
                super::conversation::recorded_operations(&project.root, &spec)
            } else {
                SpecLog::default()
            };
            let operation_codes = operations.codes();
            let observed: Vec<&SpecEvent> = found_by(operations.block(query), term, &operation_codes, &project.languages)
                .into_iter()
                .filter(|event| of_session(event, opts.session.as_deref()))
                .collect();
            if !observed.is_empty() {
                found.extend(observed.into_iter().map(|e| (e, &operation_codes)));
                found.sort_by_key(|(e, _)| super::conversation::time_of(e));
            }
            // O bloco sai do mais novo para o mais antigo; o estado, que já é
            // o retrato de agora, e a onda, que é a história dela, ficam na
            // ordem do arquivo.
            let newest_first = matches!(query, BlockQuery::Block(block) if block != Block::State);
            if newest_first {
                found.reverse();
            }
            let a_code = |codes: &BTreeMap<u64, String>| codes.values().any(|code| code.eq_ignore_ascii_case(term.trim()));
            let summary = reach == Reach::Capped
                && query == BlockQuery::Block(Block::Metrics)
                && !term.trim().is_empty()
                && !found.is_empty()
                && !a_code(&codes)
                && !a_code(&operation_codes);
            if summary {
                more = Some(translate("spec_events.read_summary", lang).to_string());
                metrics_summary(&found)
            } else {
                if reach == Reach::Capped && newest_first && found.len() > BLOCK_CAP {
                    let total = found.len().to_string();
                    more = Some(translate("spec_events.read_more", lang).replace("{shown}", &BLOCK_CAP.to_string()).replace("{total}", &total));
                    found.truncate(BLOCK_CAP);
                }
                // De dentro da cópia da onda, o que as tarefas ainda não
                // liberadas trazem sai só pelo código e pelo título.
                let held = match &request {
                    Some(Request { wave: Some(wave), .. }) => held_in_wave(&log, *wave),
                    _ => BTreeMap::new(),
                };
                found.into_iter().map(|(e, codes)| held_or_shown(&log, e, codes, shown, &held)).collect()
            }
        }
    };
    // A forma enxuta tira dos fatos a situação da fonte: só a linha inteira
    // a confere de novo.
    let events = if lean {
        let mut shown = super::lean::Lean::new(&codes);
        events.iter().map(|line| shown.line(line)).collect::<Vec<_>>()
    } else {
        events
            .into_iter()
            .map(|line| {
                let Ok(mut value) = serde_json::from_str::<Value>(&line) else {
                    return line;
                };
                if crate::shared::evidence::revalidate(&project.root, &mut value) { value.to_string() } else { line }
            })
            .collect::<Vec<_>>()
    };
    note_reading(&project.root, &spec, request.as_ref(), only_wave, &read, started);
    let mut warnings: Vec<String> = log.skipped.iter().map(|s| s.message(lang)).collect();
    // O apelido entregou o bloco, e a resposta diz o nome certo: a próxima
    // leitura sai direto, sem a volta que a recusa custava.
    if alias.is_some() {
        warnings.push(
            crate::shared::say::say("spec_events.block_alias", lang, &[("{asked}", block), ("{block}", asked)]),
        );
    }
    Ok(render(&spec, asked, more.as_deref(), &events, &extra, &warnings))
}

/// A recusa da leitura de um pedido que nunca saiu: o da onda `wave` ou, sem
/// onda, o do revisor final. Responder um texto vazio passaria por pedido.
fn no_request(wave: Option<u64>, lang: Locale) -> Value {
    let hint = match wave {
        Some(wave) => translate("spec_events.no_wave_request", lang).replace("{wave}", &wave.to_string()),
        None => translate("spec_events.no_review_request", lang).to_string(),
    };
    json!({ "ok": false, "reason": "no-request", "hint": hint })
}

/// Um evento com os códigos da fonte dele: a spec ou o arquivo de operações.
type Coded<'a> = (&'a SpecEvent, &'a BTreeMap<u64, String>);

/// A busca com termo no painel, resumida: uma linha por tipo, na ordem em
/// que `found` vem (do mais novo), com quantos o termo achou, a hora do mais
/// antigo (`from`) e do mais novo (`to`) e os códigos dos mais novos, que se
/// leem por `item-<código>`.
fn metrics_summary(found: &[Coded<'_>]) -> Vec<String> {
    let mut by_type: Vec<(&str, Vec<Coded<'_>>)> = Vec::new();
    for &(event, codes) in found {
        let kind = event.event_type.as_str();
        match by_type.iter_mut().find(|(seen, _)| *seen == kind) {
            Some((_, list)) => list.push((event, codes)),
            None => by_type.push((kind, vec![(event, codes)])),
        }
    }
    by_type
        .into_iter()
        .map(|(kind, list)| {
            let mut line = Map::new();
            line.insert("type".into(), json!(kind));
            line.insert("count".into(), json!(list.len()));
            line.insert("from".into(), json!(list.last().map(|(event, _)| event.at())));
            line.insert("to".into(), json!(list.first().map(|(event, _)| event.at())));
            let newest: Vec<String> =
                list.iter().take(SUMMARY_CODES).map(|(event, codes)| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string())).collect();
            line.insert("newest".into(), json!(newest));
            shown_line(&line)
        })
        .collect()
}

/// A busca por texto num bloco de conteúdo: a mesma procura do `run find`,
/// com o bloco como filtro de tipos, em texto. O estado, o painel, o código de
/// um item e o nome de um comando seguem pela leitura de sempre.
fn text_search(log: &SpecLog, codes: &BTreeMap<u64, String>, query: BlockQuery, term: &str, project: &super::Project, spec: &str) -> Option<String> {
    let term = term.trim();
    let content = matches!(query, BlockQuery::Block(block) if !matches!(block, Block::State | Block::Metrics));
    if term.is_empty() || !content || codes.values().any(|code| code.eq_ignore_ascii_case(term)) {
        return None;
    }
    let block = log.block(query);
    if block.iter().any(|event| calls_command(event, term)) {
        return None;
    }
    let pool: Vec<&SpecEvent> = block.into_iter().filter(|event| !NOT_SEARCHED.contains(&event.event_type.as_str())).collect();
    let asked = super::find::Asked { spec, term, show_type: true, lang: project.lang, languages: &project.languages };
    let root = project.root.clone();
    Some(super::find::found_text(log, codes, pool, &asked, &move |spec, payload| super::find::ask_jev(&root, spec, payload)))
}

/// Os tipos do estado de que só o mais novo de cada página vale: a cópia
/// para o banco e a publicação.
const ONE_PER_PAGE: &[&str] = &["copy", "publish"];

/// Do estado, só a cópia e a publicação mais novas de cada página: cada
/// rodada grava uma cópia nova, cada marco publica de novo, e a anterior fica
/// sem leitor, porque a seguinte parte da última. Sem esse corte, o estado de
/// uma spec longa trazia centenas de cópias, mais de cem mil letras, para
/// quem só queria a fase e a branch. Os outros eventos do estado seguem
/// todos, na ordem do arquivo.
fn latest_copies(events: Vec<&SpecEvent>) -> Vec<&SpecEvent> {
    let per_page = |event: &SpecEvent| ONE_PER_PAGE.contains(&event.event_type.as_str());
    let mut newest: BTreeMap<(String, Option<String>), u64> = BTreeMap::new();
    let key = |event: &SpecEvent| (event.event_type.clone(), event.str_field("page").map(str::to_string));
    for event in events.iter().filter(|event| per_page(event)) {
        let id = newest.entry(key(event)).or_insert(event.id);
        *id = (*id).max(event.id);
    }
    events.into_iter().filter(|event| !per_page(event) || newest.get(&key(event)) == Some(&event.id)).collect()
}

/// A spec da leitura e o arquivo dela, sem recusar: a leitura das lições não
/// exige spec, e só o registro da leitura precisa dela. Sem spec, sem arquivo
/// ou com o arquivo ilegível, `None`.
fn quiet_spec(opts: &ReadOpts, session: Option<&str>, root: &Path) -> Option<(String, SpecLog)> {
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session)?,
    };
    let log = store::read(&store::spec_file(root, &spec).ok()?).ok()??;
    Some((spec, log))
}

/// Registra o que a leitura achou como lido, quando ela veio da cópia de um
/// pedido aberto da spec (`request`, de `read_record::request_of`): um recibo
/// com todos os itens. Com `only_wave`, só vale na cópia dessa onda. Sem item
/// achado, ou fora de uma cópia de pedido, nada é gravado.
fn note_reading(root: &Path, spec: &str, request: Option<&Request>, only_wave: Option<u64>, read: &[String], started: Instant) {
    if read.is_empty() {
        return;
    }
    let Some(request) = request else { return };
    if only_wave.is_some_and(|wanted| request.wave != Some(wanted)) {
        return;
    }
    read_record::record(root, spec, request, read, started);
}

/// O pedido gravado no último envio da onda `wave`, igual byte a byte, em
/// texto puro. A onda sem envio não tem pedido: `None`.
fn request_text(log: &SpecLog, wave: u64) -> Option<String> {
    let sent = log.last_by_wave("send").get(&wave).copied()?;
    log.send_text(sent).map(str::to_string)
}

/// O pedido gravado no último envio do revisor final, igual byte a byte, em
/// texto puro. O envio dele não tem onda, então `request-<n>` não o alcança.
/// Sem envio de revisão na spec, `None`.
fn review_request_text(log: &SpecLog) -> Option<String> {
    let sent = log.visible().into_iter().filter(|event| event.event_type == "send" && event.str_field("role") == Some("review")).max_by_key(|event| event.id)?;
    log.send_text(sent.id).map(str::to_string)
}

/// O número da última mensagem do usuário que a leitura mostra, sem o texto:
/// é o `origin` de quem grava um item a partir do que ele acabou de dizer.
fn last_user_message(log: &SpecLog) -> Option<u64> {
    log.block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|event| event.event_type == "message" && event.str_field("author") == Some("user"))
        .map(|event| event.id)
        .max()
}

/// O item que `item-<código>` ou `item-<número>` pede. Pelo código, a versão
/// vigente — ou, com o item todo removido, a última versão dele; pelo
/// número, aquela versão. Sem item, `None`.
fn find_item<'a>(log: &'a SpecLog, target: &EventRef, codes: &BTreeMap<u64, String>) -> Option<&'a SpecEvent> {
    let hidden = log.hidden();
    match target {
        EventRef::Id(id) => log.get(*id),
        EventRef::Code(code) => {
            let versions: Vec<&SpecEvent> = log.events.iter().filter(|e| codes.get(&e.id) == Some(code)).collect();
            versions.iter().rev().find(|e| !hidden.contains_key(&e.id)).or(versions.last()).copied()
        }
    }
}

/// A linha de um item, como `item-<código>` e `item-<número>` a mostram: os
/// campos dele, com o código, com `removed` quando uma remoção o tirou, com
/// `replaced_by` quando outra versão já o substitui, e com `changed` quando
/// ele substitui uma anterior. A mensagem do usuário sai como os outros
/// itens, com o texto inteiro. `shown` diz se é a linha de quem lê da cópia
/// de um pedido ([`Shown::Agent`]).
fn item_line(log: &SpecLog, event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown) -> String {
    let hidden = log.hidden();
    let mut fields = log.whole_fields(event);
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), json!(code));
    }
    match hidden.get(&event.id) {
        Some(Hidden::Removed { .. }) => {
            fields.insert("removed".into(), json!(true));
        }
        Some(Hidden::Replaced { by }) => {
            fields.insert("replaced_by".into(), json!(by));
        }
        _ => {}
    }
    let previous = event.replaced().first().and_then(|old| log.get(*old));
    if let Some(changed) = previous.map(|old| changed_fields(&log.whole_fields(old), &log.whole_fields(event))).filter(|c| !c.is_empty()) {
        fields.insert("changed".into(), Value::Object(changed));
    }
    if shown == Shown::Agent {
        without_agent_omissions(&mut fields, &event.event_type);
    }
    shown_line(&fields)
}

/// Os campos de uma linha que não são do item: o envelope, a busca, o
/// apontador da versão anterior e a sessão de quem gravou. A sessão não é
/// conteúdo do item, é registro de quem o escreveu: uma versão nova gravada
/// noutra conversa não mudou o item por isso.
const NOT_COMPARED: &[&str] = &["v", "id", "code", "at", "type", "search", "replaces", "session"];

/// O que mudou de `old` para `new`, campo a campo. Uma lista diz o que entrou
/// (`added`) e o que saiu (`removed`) — os arquivos pelo caminho —; um
/// número, uma marca ou um campo que surgiu ou sumiu diz de quanto para
/// quanto (`from`, `to`); um texto ou um objeto só diz que mudou (`true`),
/// porque as duas versões se leem pelo número.
pub(crate) fn changed_fields(old: &Map<String, Value>, new: &Map<String, Value>) -> Map<String, Value> {
    let keys: BTreeSet<&str> = old.keys().chain(new.keys()).map(String::as_str).filter(|key| !NOT_COMPARED.contains(key)).collect();
    let mut changed = Map::new();
    for key in keys {
        let (before, after) = (old.get(key), new.get(key));
        if before == after {
            continue;
        }
        let scalar = |value: Option<&Value>| value.is_none_or(|v| v.is_number() || v.is_boolean() || v.is_null());
        let change = if before.is_some_and(Value::is_array) || after.is_some_and(Value::is_array) {
            list_change(before, after)
        } else if scalar(before) && scalar(after) {
            json!({ "from": before, "to": after })
        } else {
            Value::Bool(true)
        };
        changed.insert(key.to_string(), change);
    }
    changed
}

/// O que entrou e o que saiu de uma lista, na ordem de cada versão. Uma lista
/// que só mudou de ordem, ou de um detalhe de um arquivo que continua nela,
/// só diz que mudou.
fn list_change(before: Option<&Value>, after: Option<&Value>) -> Value {
    let items = |value: Option<&Value>| -> Vec<Value> { value.and_then(Value::as_array).map(|list| list.iter().map(list_key).collect()).unwrap_or_default() };
    let (old, new) = (items(before), items(after));
    let only_in = |these: &[Value], those: &[Value]| -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for item in these.iter().filter(|item| !those.contains(item)) {
            if !out.contains(item) {
                out.push(item.clone());
            }
        }
        out
    };
    let (added, removed) = (only_in(&new, &old), only_in(&old, &new));
    if added.is_empty() && removed.is_empty() {
        return Value::Bool(true);
    }
    json!({ "added": added, "removed": removed })
}

/// Como um elemento de lista se compara entre versões: o arquivo pelo
/// caminho, o resto pelo valor inteiro.
fn list_key(item: &Value) -> Value {
    item.get("path").filter(|path| path.is_string()).cloned().unwrap_or_else(|| item.clone())
}

/// A entrega vigente da onda `wave`: a que a rodada assumiu ou, sem ela, a
/// última volta do agente que a rodada ainda não assumiu — esta com
/// `returned`. Do `agreed`, fica só o item não cumprido. Sem entrega, a lista
/// vem vazia.
fn delivered_lines(log: &SpecLog, wave: u64, codes: &BTreeMap<u64, String>) -> Vec<String> {
    let assumed = log.last_by_wave("delivered").get(&wave).and_then(|id| log.get(*id));
    let delivery = assumed.or_else(|| log.unassumed_returns().into_iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(wave)));
    let Some(delivery) = delivery else {
        return Vec::new();
    };
    let mut fields = delivery.fields.clone();
    if let Some(code) = codes.get(&delivery.id) {
        fields.insert("code".into(), json!(code));
    }
    if let Some(Value::Array(agreed)) = fields.get_mut("agreed") {
        agreed.retain(|answer| answer.get("met") != Some(&Value::Bool(true)));
    }
    vec![shown_line(&fields)]
}

/// As ondas que a leitura mostra, uma linha curta por onda, na ordem do
/// número: a versão vigente, o código, o número, a primeira linha não vazia
/// do texto e as ondas de que ela depende, vazia quando não depende de
/// nenhuma. A onda removida ou substituída fica de fora, como no bloco. Com
/// `open_only`, fica de fora também a onda que terminou. Devolve as linhas e
/// quantas ondas a lista teria sem esse corte.
fn wave_list_lines(log: &SpecLog, codes: &BTreeMap<u64, String>, open_only: bool) -> (Vec<String>, usize) {
    let mut waves: Vec<&SpecEvent> = log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave").collect();
    waves.sort_by_key(|wave| (wave.wave(), wave.id));
    let total = waves.len();
    if open_only {
        let states = crate::commands::flow::round::wave_states(log);
        waves.retain(|wave| !wave.wave().and_then(|n| states.get(&n)).is_some_and(|state| state.finished()));
    }
    let lines = waves
        .into_iter()
        .map(|wave| {
            let text = wave.str_field("text").unwrap_or_default();
            let first_line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
            let mut line = Map::new();
            line.insert("id".into(), json!(wave.id));
            if let Some(code) = codes.get(&wave.id) {
                line.insert("code".into(), json!(code));
            }
            line.insert("n".into(), json!(wave.wave()));
            line.insert("first_line".into(), json!(first_line));
            line.insert("depends_on".into(), json!(wave.ints("depends_on")));
            shown_line(&line)
        })
        .collect();
    (lines, total)
}

/// As tarefas ainda por entregar, uma linha por tarefa na ordem do código:
/// o código, o número da versão vigente, a onda (ou `null`), quantos
/// arquivos ela declara e as tarefas ainda por entregar de que depende. A
/// que espera plano ([`needs_planning`](crate::commands::flow::round::needs_planning))
/// leva também `needs_planning`: o `files: 0` sozinho não diz que ela fica
/// fora das ondas até quem conduz a obra planejá-la. O total conta as
/// tarefas e os arquivos distintos entre todas.
fn backlog_lines(log: &SpecLog, codes: &BTreeMap<u64, String>) -> (Vec<String>, Value) {
    let left = crate::commands::flow::round::tasks_left(log);
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let order = |id: u64| codes.get(&id).and_then(|code| mustard_id::parse(code)).map_or(u64::MAX, |(_, n)| n);
    let mut sorted: Vec<_> = left.iter().collect();
    sorted.sort_by_key(|task| (order(task.id), task.id));
    let lines = sorted
        .iter()
        .map(|task| {
            let mut line = Map::new();
            line.insert("id".into(), json!(task.id));
            line.insert("code".into(), json!(code_of(task.id)));
            line.insert("wave".into(), json!(log.get(task.id).and_then(SpecEvent::wave)));
            line.insert("files".into(), json!(task.files.len()));
            let waits: Vec<String> = task.depends_on.iter().map(|id| code_of(*id)).collect();
            line.insert("depends_on".into(), json!(waits));
            if log.get(task.id).is_some_and(crate::commands::flow::round::needs_planning) {
                line.insert("needs_planning".into(), json!(true));
            }
            shown_line(&line)
        })
        .collect();
    let files: BTreeSet<&String> = left.iter().flat_map(|task| &task.files).collect();
    (lines, json!({ "tasks": left.len(), "files": files.len() }))
}

/// A resposta curta do backlog: o total logo depois de `ok`, e cada linha sem
/// a onda vazia e sem a lista vazia de dependências. Com `--full`, a de
/// [`backlog_lines`] inteira.
pub(crate) fn short_backlog(report: &Value) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), report["ok"].clone());
    out.insert("total".into(), report["total"].clone());
    for (key, value) in report.as_object().into_iter().flatten() {
        let value = match key.as_str() {
            "ok" | "total" => continue,
            "events" => json!(value.as_array().into_iter().flatten().map(|line| {
                let mut line = line.clone();
                if let Some(fields) = line.as_object_mut() {
                    fields.retain(|_, field| !field.is_null() && field.as_array().is_none_or(|list| !list.is_empty()));
                }
                line
            }).collect::<Vec<_>>()),
            _ => value.clone(),
        };
        out.insert(key.clone(), value);
    }
    Value::Object(out)
}

/// A leitura do bloco `block` como sai: a do backlog passa pela resposta
/// curta ([`projection`](crate::commands::flow::projection)), salvo com
/// `full`; as outras saem como vieram.
fn shortened(block: &str, text: String, full: bool) -> String {
    if !matches!(ReadQuery::parse(block), Some(ReadQuery::Backlog)) {
        return text;
    }
    let Ok(report) = serde_json::from_str::<Value>(&text) else {
        return text;
    };
    crate::commands::flow::projection("backlog", &report, full).map_or(text, |short| render_value(&short))
}

/// A resposta já projetada, no molde de [`render`]: um evento por linha.
fn render_value(report: &Value) -> String {
    let mut out = String::from("{");
    for (at, (key, value)) in report.as_object().into_iter().flatten().enumerate() {
        if at > 0 {
            out.push(',');
        }
        out.push_str(&json!(key).to_string());
        out.push(':');
        match value.as_array().filter(|_| key == "events") {
            Some(events) => {
                let lines: Vec<String> = events.iter().map(Value::to_string).collect();
                if lines.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\n");
                    out.push_str(&lines.join(",\n"));
                    out.push_str("\n]");
                }
            }
            None => out.push_str(&value.to_string()),
        }
    }
    out.push('}');
    out
}

/// A soma das chamadas de cada comando, uma linha por comando, na ordem do
/// nome: quantas são; as falhas por motivo, a recusa pelo motivo dela (ou
/// `refused`, quando ela não diz) e a falha do filtro, em que a busca volta
/// pelo banco, pelo filtro e o motivo (`jev:busy`); a mediana, o p90 e o pior
/// do tempo da chamada (`ms`) e do tempo do filtro (`filter_ms`); e os tokens
/// e o custo, somados. Os campos que nenhuma chamada do comando traz ficam de
/// fora. Com `term`, só o comando que tem esse nome. As chamadas são as que
/// o programa observou ([`observed`](super::conversation::observed)): as da
/// spec antiga e as do arquivo de operações, sem os recibos de leitura.
fn calls_lines(observed: &[SpecEvent], term: &str) -> Vec<String> {
    let term = term.trim();
    let mut by_command: BTreeMap<&str, Vec<&SpecEvent>> = BTreeMap::new();
    for call in observed {
        if call.event_type == "call" && (term.is_empty() || calls_command(call, term)) {
            by_command.entry(call.str_field("command").unwrap_or_default().trim()).or_default().push(call);
        }
    }
    by_command.into_iter().map(|(command, calls)| shown_line(&calls_sum(command, &calls))).collect()
}

/// A linha de [`calls_lines`] de um comando.
fn calls_sum(command: &str, calls: &[&SpecEvent]) -> Map<String, Value> {
    let mut sum = Map::new();
    sum.insert("command".into(), json!(command));
    sum.insert("count".into(), json!(calls.len()));
    let mut failures: BTreeMap<&str, u64> = BTreeMap::new();
    for call in calls {
        let reason = if call.str_field("result") == Some("refused") {
            Some(call.str_field("refusal").unwrap_or("refused"))
        } else {
            call.str_field("filter").filter(|filter| filter.contains(':'))
        };
        if let Some(reason) = reason {
            *failures.entry(reason).or_default() += 1;
        }
    }
    if !failures.is_empty() {
        sum.insert("failures".into(), json!(failures));
    }
    let numbers = |field: &str| calls.iter().filter_map(|call| call.int(field)).collect::<Vec<u64>>();
    for field in ["ms", "filter_ms"] {
        if let Some(spread) = spread(numbers(field)) {
            sum.insert(field.into(), spread);
        }
    }
    for field in ["tokens", "cost_micro_usd", "requests", "input_bytes", "output_bytes", "estimated_input_tokens", "estimated_output_tokens"] {
        let values = numbers(field);
        if !values.is_empty() {
            sum.insert(field.into(), json!(values.iter().fold(0_u64, |total, n| total.saturating_add(*n))));
        }
    }
    sum
}

/// A mediana, o p90 e o pior de `values`, pela posição mais próxima: o
/// percentil `p` é o valor na posição ⌈n·p/100⌉ da lista em ordem crescente,
/// sem média entre dois valores. `None` sem valor nenhum.
fn spread(mut values: Vec<u64>) -> Option<Value> {
    values.sort_unstable();
    let at = |percent: usize| values.get((values.len() * percent).div_ceil(100).saturating_sub(1)).copied();
    Some(json!({"median": at(50)?, "p90": at(90)?, "max": values.last()?}))
}

/// O que o pedido da onda `wave` lista, pela mesma conta que o monta
/// ([`request_items`], com a escolha gravada no envio): primeiro os itens da
/// spec, cada um com o código, depois as lições do banco do projeto `root`,
/// pelo número delas. Com `term`, um código de item acha só aquele item, e
/// qualquer outro termo passa pela busca por nota nos itens e nas lições,
/// nas línguas `languages`. A onda que o plano não tem não tem pedido: a
/// lista vem vazia. Devolve as linhas e, ao lado, o que cada uma é para o
/// registro da leitura: o código do item e `lesson-<número>` da lição.
#[allow(clippy::too_many_arguments)]
fn dispatch_lines(
    root: &Path,
    log: &SpecLog,
    wave: u64,
    term: &str,
    codes: &BTreeMap<u64, String>,
    languages: &Languages,
    shown: Shown,
    lang: Locale,
    held: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    if !log.planned_waves().contains(&wave) {
        return (Vec::new(), Vec::new());
    }
    // O envio guarda a lista que o pedido imprimiu: o lote é ela, e nunca uma
    // segunda conta que acharia a onda ou um item que o pedido não listou. O
    // envio sem a lista, de antes dela, cai na conta da montagem. O que as
    // tarefas ainda não liberadas seguram (`held`) fica de fora.
    let sent = log.last_by_wave("send").get(&wave).copied();
    if let Some(listed) = sent.and_then(|sent| log.send_field(sent, "read_items")).and_then(Value::as_array) {
        let listed: Vec<String> = listed.iter().filter_map(Value::as_str).filter(|code| !held.contains_key(*code)).map(str::to_string).collect();
        return listed_lines(root, log, &listed, term, codes, languages, shown, lang);
    }
    let bank = lesson_bank(root);
    let found = request_items(log, bank.as_ref(), wave, None, languages);
    let items = found_by(found.items, term, codes, languages);
    // A lição não tem código de item, e o número dela no banco pode ser o de
    // um item da spec: ela passa só pela busca, nunca pelos códigos da spec.
    let lessons = found_by(found.lessons, term, &BTreeMap::new(), languages);
    let read: Vec<String> = items
        .iter()
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .chain(lessons.iter().map(|lesson| format!("lesson-{}", lesson.id)))
        .collect();
    // De dentro da cópia, cada item é um bloco de texto ([`agent_block`]).
    let agent = shown == Shown::Agent;
    let mut lines: Vec<String> = items.into_iter().map(|e| if agent { agent_block(e, codes, lang) } else { shown_with_code(log, e, codes, shown) }).collect();
    lines.extend(lessons.into_iter().map(|lesson| if agent { lesson_block(lesson, lang) } else { shown_line(&lesson.fields) }));
    (lines, read)
}

/// A lista que o pedido mais novo do revisor final gravou (`read_items`).
/// `None` sem pedido do revisor; o pedido sem a lista, de antes dela, lista
/// nada.
fn review_listed(log: &SpecLog) -> Option<Vec<String>> {
    let sent = log.visible().into_iter().filter(|event| event.event_type == "send" && event.str_field("role") == Some("review")).max_by_key(|event| event.id)?;
    let listed = log.send_field(sent.id, "read_items").and_then(Value::as_array).into_iter().flatten();
    Some(listed.filter_map(Value::as_str).map(str::to_string).collect())
}

/// Os itens da lista `listed` que um pedido gravou, na ordem dela: cada código
/// na versão vigente, como `item-<código>` a mostra, e cada `lesson-<número>`
/// pela lição do banco do projeto `root`. O `term` filtra como no
/// [`dispatch_lines`]. Devolve as linhas e o que cada uma é, para o recibo.
#[allow(clippy::too_many_arguments)]
fn listed_lines(
    root: &Path,
    log: &SpecLog,
    listed: &[String],
    term: &str,
    codes: &BTreeMap<u64, String>,
    languages: &Languages,
    shown: Shown,
    lang: Locale,
) -> (Vec<String>, Vec<String>) {
    // O código de cada item, uma vez só: a versão vigente, ou a última
    // quando todas saíram, como a leitura de um item escolhe.
    let hidden = log.hidden();
    let mut by_code: BTreeMap<&str, &SpecEvent> = BTreeMap::new();
    for event in &log.events {
        let Some(code) = codes.get(&event.id) else { continue };
        let keep = by_code.get(code.as_str()).is_none_or(|had| !hidden.contains_key(&event.id) || hidden.contains_key(&had.id));
        if keep {
            by_code.insert(code, event);
        }
    }
    let bank = listed.iter().any(|code| code.starts_with("lesson-")).then(|| lesson_bank(root)).flatten();
    let mut items: Vec<&SpecEvent> = Vec::new();
    let mut lessons: Vec<&SpecEvent> = Vec::new();
    for code in listed {
        match code.strip_prefix("lesson-").and_then(|n| n.parse::<u64>().ok()) {
            Some(n) => lessons.extend(bank.as_ref().and_then(|bank| bank.get(n))),
            None => items.extend(by_code.get(code.as_str()).copied()),
        }
    }
    let items = found_by(items, term, codes, languages);
    let lessons = found_by(lessons, term, &BTreeMap::new(), languages);
    let read: Vec<String> = items
        .iter()
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .chain(lessons.iter().map(|lesson| format!("lesson-{}", lesson.id)))
        .collect();
    let agent = shown == Shown::Agent;
    let mut lines: Vec<String> = items.into_iter().map(|e| if agent { agent_block(e, codes, lang) } else { shown_with_code(log, e, codes, shown) }).collect();
    lines.extend(lessons.into_iter().map(|lesson| if agent { lesson_block(lesson, lang) } else { shown_line(&lesson.fields) }));
    (lines, read)
}

/// O bloco de um item na leitura da onda de dentro da cópia, em texto: o
/// título e o código e, embaixo, o texto inteiro dele ([`item_body`]), com os
/// arquivos que o item declara. A mensagem do usuário leva o dia. A parte do
/// usuário e o exemplo vêm pelo código.
fn agent_block(item: &SpecEvent, codes: &BTreeMap<u64, String>, lang: Locale) -> String {
    let code = codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
    let body = item_body(item, &code, lang);
    if item.event_type == "message" {
        let day = super::find::day_of(item.at()).unwrap_or_default();
        return format!("## {} | {day} | {code}\n{body}", translate("spec_events.find_user_message", lang));
    }
    let title = item.str_field("title").filter(|t| !t.trim().is_empty()).map_or_else(|| super::find::type_word(&item.event_type, lang), str::to_string);
    let mut block = format!("## {title} | {code}\n{body}");
    let files: Vec<&str> = item
        .fields
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
        .collect();
    if !files.is_empty() {
        block.push('\n');
        block.push_str(&translate("spec_events.find_files", lang).replace("{files}", &files.join(", ")));
    }
    block
}

/// A lição na leitura da onda: o número dela no banco e o texto.
fn lesson_block(lesson: &SpecEvent, lang: Locale) -> String {
    format!("## {} {}\n{}", translate("spec_events.find_lesson", lang), lesson.id, lesson.str_field("text").unwrap_or_default())
}

/// O bloco `lessons`: fora da spec, no banco do projeto. `--term` é o número
/// da lição no banco — não uma busca, como no resto dos blocos —, porque é
/// assim que o pedido de uma onda a leva, sem copiar o texto dela. Sem
/// número, ou sem lição vigente com esse número, a lista vem vazia; sem
/// banco no disco, o mesmo. Devolve a saída e o número das lições achadas.
fn read_lessons(root: &Path, spec: Option<&str>, term: Option<&str>, lang: Locale) -> Result<(String, Vec<u64>), Value> {
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);
    let paths = ClaudePaths::for_project(root).map_err(|e| refuse(Refusal::Io { detail: e.to_string() }))?;
    let bank = mustard_core::io::lessons::read(&paths.lessons_path()).map_err(refuse)?.unwrap_or_default();
    let wanted = term.and_then(|t| t.trim().parse::<u64>().ok());
    let found: Vec<_> = kept(&bank).into_iter().filter(|lesson| wanted.is_some_and(|id| lesson.id == id)).collect();
    let events: Vec<String> = found.iter().map(|lesson| shown_line(&lesson.fields)).collect();
    let numbers = found.iter().map(|lesson| lesson.id).collect();
    Ok((render(spec.unwrap_or_default(), "lessons", None, &events, &[], &[]), numbers))
}

/// A linha como a leitura mostra, com o código do item (`MSTD-<sigla>-<NNNN>`),
/// que é o jeito de citá-lo e o endereço dele na página: o gravado na linha
/// ou, numa linha sem código, o que a leitura dá a ela. `shown` diz o que sai
/// dela: o envio sem o pedido (`text`) na lista das ondas e no painel, e no
/// painel sem termo também a injeção e a entrega sem o `text`.
fn shown_with_code(log: &SpecLog, event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown) -> String {
    shown_line(&shown_fields(log, event, codes, shown))
}

/// Os campos que [`shown_with_code`] mostra. O envio que sai sem o pedido
/// sai também sem os blocos das tarefas, que são o resto dele.
fn shown_fields(log: &SpecLog, event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown) -> Map<String, Value> {
    let mut fields = log.whole_fields(event);
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), Value::String(code.clone()));
    }
    let kind = event.event_type.as_str();
    let long_text = match shown {
        Shown::Whole | Shown::Agent => false,
        Shown::Plan => kind == "send",
        Shown::Panel => PANEL_WITHOUT_TEXT.contains(&kind),
    };
    if long_text {
        fields.remove("text");
        fields.remove("tasks");
    }
    if shown == Shown::Agent {
        without_agent_omissions(&mut fields, kind);
    }
    fields
}

/// A linha de um evento lido de dentro da cópia da onda que segura `held`
/// ([`held_in_wave`]): o item que só uma tarefa ainda não liberada traz sai
/// só pelo código, pelo tipo e pelo título, como na lista do pedido; o envio
/// sai sem os blocos das tarefas. Fora da cópia, `held` vem vazio e só o
/// envio muda.
fn held_or_shown(log: &SpecLog, event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown, held: &BTreeMap<String, String>) -> String {
    let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
    if held.contains_key(&code) {
        let mut fields = Map::new();
        fields.insert("code".into(), json!(code));
        fields.insert("type".into(), json!(event.event_type));
        if let Some(title) = item_title(event) {
            fields.insert("title".into(), json!(title));
        }
        return shown_line(&fields);
    }
    let mut fields = shown_fields(log, event, codes, shown);
    if !held.is_empty() {
        fields.remove("tasks");
    }
    shown_line(&fields)
}

/// O que a leitura mostra de cada evento.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// A linha inteira.
    Whole,
    /// A lista das ondas: o envio sai sem o pedido.
    Plan,
    /// O painel sem termo: o envio, a injeção e a entrega saem sem o texto.
    Panel,
    /// A linha de quem lê da cópia de um pedido aberto: sem as marcas do
    /// binário e sem as palavras de busca.
    Agent,
}

/// Os tipos que o painel sem termo mostra sem o `text`.
const PANEL_WITHOUT_TEXT: &[&str] = &["send", "injection", "delivered"];

/// Os campos que quem lê da cópia de um pedido não lê: as marcas que o binário
/// carimba (`v`, `at`, `author`) e as palavras de busca (`keys`).
const AGENT_OMITTED: &[&str] = &["v", "at", "author", "keys"];

/// Tira da linha, e do `changed` dela, o que quem lê da cópia de um pedido não
/// lê ([`AGENT_OMITTED`]). A mensagem do usuário guarda o `at` e o `author`:
/// dizem quem falou e quando.
fn without_agent_omissions(fields: &mut Map<String, Value>, event_type: &str) {
    // O envio, lido de dentro da cópia, sai sem os blocos das tarefas: as
    // seguintes chegam na resposta do passo.
    if event_type == "send" {
        fields.remove("tasks");
        if let Some(Value::Object(changed)) = fields.get_mut("changed") {
            changed.remove("tasks");
        }
    }
    let kept: &[&str] = if event_type == "message" { &["at", "author"] } else { &[] };
    for key in AGENT_OMITTED.iter().filter(|key| !kept.contains(key)) {
        fields.remove(*key);
        if let Some(Value::Object(changed)) = fields.get_mut("changed") {
            changed.remove(*key);
        }
    }
    if fields.get("changed").and_then(Value::as_object).is_some_and(Map::is_empty) {
        fields.remove("changed");
    }
}

/// O evento é da sessão `session`, quando uma sessão foi pedida. Sem sessão
/// pedida, todo evento passa; com ela, só o que a diz. O registro de antes
/// desta versão não tem o campo, e fica de fora da leitura por sessão: dizer
/// que ele é da sessão pedida seria inventar.
fn of_session(event: &SpecEvent, session: Option<&str>) -> bool {
    let Some(session) = session.map(str::trim).filter(|session| !session.is_empty()) else {
        return true;
    };
    event.str_field("session") == Some(session)
}

/// A saída de uma leitura: o envelope, com `more` antes dos eventos quando a
/// leitura deixou algo de fora, um evento por linha, depois os campos de
/// `extra` que a leitura dá além dos eventos, e os avisos por último.
fn render(spec: &str, block: &str, more: Option<&str>, events: &[String], extra: &[(&str, Value)], warnings: &[String]) -> String {
    let mut out = format!("{{\"ok\":true,\"spec\":{},\"block\":{},\"count\":{},", json!(spec), json!(block), events.len());
    if let Some(more) = more {
        out.push_str("\"more\":");
        out.push_str(&json!(more).to_string());
        out.push(',');
    }
    out.push_str("\"events\":[");
    for (i, event) in events.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str(event);
    }
    if !events.is_empty() {
        out.push('\n');
    }
    out.push(']');
    for (key, value) in extra {
        out.push(',');
        out.push_str(&json!(key).to_string());
        out.push(':');
        out.push_str(&value.to_string());
    }
    if !warnings.is_empty() {
        out.push_str(",\"warnings\":");
        out.push_str(&json!(warnings).to_string());
    }
    out.push('}');
    out
}

/// Roda o comando `mustard-rt run read <bloco> --root <raiz> --spec <spec>`
/// que uma resposta do Mustard entrega no lugar do texto, pela mesma leitura
/// do comando, e devolve o que ele imprime. O texto que não é esse comando
/// derruba o teste. Os testes leem por aqui como o agente lê.
#[cfg(test)]
pub(crate) fn read_by_command(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(words.get(..3), Some(&["mustard-rt", "run", "read"][..]), "{command}");
    let flag = |name: &str| words.iter().position(|w| *w == name).and_then(|i| words.get(i + 1)).map(|w| (*w).to_string());
    let opts = ReadOpts {
        root: PathBuf::from(flag("--root").unwrap_or_else(|| panic!("no --root: {command}"))),
        spec: flag("--spec"),
        block: words[3].to_string(),
        term: None,
     session: None };
    read_at(&opts).unwrap_or_else(|refusal| panic!("{command}: {refusal}"))
}

/// Run `read` and print the block; exit 1 on a refusal. The recorded request
/// of a wave, or of the final review, and the final review request the close
/// would build now print byte for byte, with no newline added. The backlog
/// prints its short answer unless `full` asks for the whole one. Without
/// `all`, a block shows its newest [`BLOCK_CAP`] events, the wave list only
/// the open waves and a metrics search a summary by type.
pub fn run(opts: &ReadOpts, full: bool, all: bool) {
    let started = std::time::Instant::now();
    let read = if all { read_at(opts) } else { read_capped(opts) };
    let result = read.map(|text| shortened(&opts.block, text, full));
    // Reads from a dispatched request already record each required item.
    // Keep those receipts separate from the CLI invocation's own measure.
    let report = match &result {
        Ok(text) => serde_json::from_str(text).unwrap_or_else(|_| json!({"ok":true,"spec":opts.spec})),
        Err(refusal) => refusal.clone(),
    };
    let exact=matches!(ReadQuery::parse(&opts.block), Some(ReadQuery::Request(_) | ReadQuery::ReviewRequest | ReadQuery::WaveReviewRequest(_) | ReadQuery::ReviewPreview));
    let output=match &result {
        Ok(text) => if exact {text.clone()} else {format!("{text}\n")},
        Err(refusal) => serde_json::to_string_pretty(refusal).unwrap_or_default()+"\n",
    };
    let _ = super::conversation::record_output_call(&opts.root, "read", opts.spec.as_deref(), started, &report,&output);
    match result {
        Ok(text) if matches!(ReadQuery::parse(&opts.block), Some(ReadQuery::Request(_) | ReadQuery::ReviewRequest | ReadQuery::WaveReviewRequest(_) | ReadQuery::ReviewPreview)) => {
            print!("{text}");
        }
        Ok(report) => println!("{report}"),
        Err(refusal) => {
            println!("{}", serde_json::to_string_pretty(&refusal).unwrap_or_else(|_| "{}".into()));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{WriteOpts, seed_at};
    use tempfile::tempdir;

    fn opts(root: &std::path::Path, block: &str, term: Option<&str>) -> ReadOpts {
        ReadOpts { root: root.to_path_buf(), spec: Some("teste".into()), block: block.into(), term: term.map(str::to_string), session: None }
    }

    fn without_spec(root: &std::path::Path, block: &str) -> ReadOpts {
        ReadOpts { spec: None, ..opts(root, block, None) }
    }

    fn put(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        // A spec já aberta: o arquivo de eventos existe antes da gravação,
        // como o comando que abre a spec o deixa.
        let path = store::spec_file(root, "teste").expect("spec file");
        if !path.exists() {
            std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
            std::fs::File::create(&path).expect("the event file");
        }
        let out = seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("teste".into()), event_type: event_type.into(), json: fields.to_string() });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_u64().unwrap()
    }

    fn events(report: &str) -> Vec<Value> {
        let parsed: Value = serde_json::from_str(report).expect("the report is JSON");
        parsed["events"].as_array().cloned().unwrap_or_default()
    }

    /// O código do evento `id`, que a leitura de um bloco mostra no lugar do
    /// número.
    fn code_of(root: &std::path::Path, id: u64) -> Value {
        let log = store::read(&store::spec_file(root, "teste").expect("spec file")).expect("read").expect("the spec file");
        json!(log.codes()[&id])
    }

    #[test]
    fn reading_one_wave_brings_only_that_wave_and_never_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let c1 = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": said}));
        put(root, "wave", json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": said}));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": said}));
        put(root, "task", json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": said}));

        let report = read_at(&opts(root, "wave-2", None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 2, "{report}");
        assert_eq!(got[0]["code"], json!("MSTD-WAVE-0002"), "each event carries its code");
        assert_eq!(got[1]["code"], json!("MSTD-TASK-0002"));
        for event in &got {
            let wave = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
            assert_eq!(wave, Some(2), "{event}");
        }
        assert!(!report.contains("\"search\""), "{report}");
    }

    #[test]
    fn a_term_filters_the_conversation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "apagando a pasta"}));
        put(root, "message", json!({"author": "user", "text": "outro assunto"}));
        let got = read_at(&opts(root, "conversation", Some("apagar"))).unwrap();
        assert_eq!(got.lines().count(), 1, "{got}");
        assert!(got.starts_with("MSTD-MSG-0001 | ") && got.ends_with("apagando a pasta"), "one text line per item: {got}");
    }

    /// Com a língua do texto em espanhol, a busca na spec corta as palavras
    /// como espanhol: "correr" acha a mensagem que diz "corriendo", que o
    /// português e o inglês não ligariam.
    #[test]
    fn a_spanish_project_cuts_the_spec_search_as_spanish() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language": {"text": "es-ES"}}"#).unwrap();
        put(root, "message", json!({"author": "user", "text": "sigue corriendo la prueba"}));
        put(root, "message", json!({"author": "user", "text": "otro asunto"}));
        let got = read_at(&opts(root, "conversation", Some("correr"))).unwrap();
        assert_eq!(got.lines().count(), 1, "{got}");
        assert!(got.ends_with("sigue corriendo la prueba"), "{got}");
    }

    /// Uma frase que nenhum item tem inteira ainda devolve o que a nota acha,
    /// do mais forte para o menos forte, marcado como parcial; sem Jev, uma
    /// linha diz que ele ficou fora.
    #[test]
    fn a_whole_phrase_brings_the_strongest_items_first_instead_of_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let put_rule = |text: &str, key: &str| put(root, "rule", json!({"text": text, "keys": [key], "example": "e", "origin": said}));
        let weak = put_rule("A página mostra o commit da onda.", "página");
        let strong = put_rule("A rodada formata só os arquivos da rodada antes do commit.", "formatador");
        put_rule("O levantamento pergunta o tipo de trabalho.", "levantamento");

        let phrase = "a rodada formata os arquivos dela antes do commit da onda";
        let got = read_at(&opts(root, "agreed", Some(phrase))).unwrap();
        let code = |id: u64| format!("MSTD-RULE-{:04}", id - said);
        let lines: Vec<&str> = got.lines().collect();
        assert!(lines.len() >= 3, "the phrase finds something: {got}");
        assert!(lines[0].starts_with(&code(strong)), "the strongest comes first: {got}");
        assert!(lines.iter().any(|line| line.starts_with(&code(weak)) && line.contains(" | ")), "a partial match still comes: {got}");
        assert!(lines.last().is_some_and(|line| line.starts_with("Jev ")), "without Jev, a line says so: {got}");
    }

    #[test]
    fn a_term_that_is_an_item_code_finds_that_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "keys": ["C-1"], "form": "ubiquitous", "origin": said}));
        put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "keys": ["C-2"], "form": "ubiquitous", "origin": said}));
        let got = events(&read_at(&opts(root, "criteria", Some("MSTD-CRIT-0002"))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["code"], json!("MSTD-CRIT-0002"));
        assert_eq!(got[0]["when"], json!("c"));
    }

    /// Uma lição gravada no banco no disco (`.claude/spec/lessons.ndjson`).
    fn write_lesson(bank: &std::path::Path, text: &str, keys: &[&str]) -> u64 {
        let draft = json!({"class": "defect", "text": text, "keys": keys,
            "applies_to": {"files": ["**"]}, "found_in": {"source": "apps/rt/CLAUDE.md"}});
        let Value::Object(draft) = draft else { unreachable!() };
        mustard_core::io::lessons::write(bank, draft, None).expect("a lição entra no banco").id
    }

    /// O bloco `lessons` devolve a lição pelo número dela no banco, fora da
    /// spec: `--term` é o número, não uma busca — mesmo um termo que casa o
    /// texto de outra lição não a traz, porque não é pelo texto que a lição
    /// se acha aqui.
    #[test]
    fn the_lessons_block_returns_a_lesson_by_its_number_not_by_search() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bank = mustard_core::ClaudePaths::for_project(root).unwrap().lessons_path();
        let id = write_lesson(&bank, "Nunca comitar sem rodar a suíte inteira.", &["suíte", "commit"]);
        write_lesson(&bank, "Outra lição qualquer, sem relação com a suíte.", &["outra"]);

        let got = events(&read_at(&opts(root, "lessons", Some(&id.to_string()))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["id"], json!(id));
        assert_eq!(got[0]["text"], json!("Nunca comitar sem rodar a suíte inteira."));

        // "suíte" acha as duas lições por texto; pelo número, não acha
        // nenhuma — a leitura de `lessons` nunca é busca por palavra.
        let by_word = events(&read_at(&opts(root, "lessons", Some("suíte"))).unwrap());
        assert!(by_word.is_empty(), "{by_word:?}");
    }

    #[test]
    fn an_unknown_block_and_a_spec_without_file_are_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let unknown = read_at(&opts(root, "everything", None)).unwrap_err();
        assert_eq!(unknown["reason"], json!("unknown-block"));
        assert!(unknown["hint"].as_str().unwrap().contains("everything"));
        assert!(unknown["hint"].as_str().unwrap().contains("criterion"));
        assert!(unknown["hint"].as_str().unwrap().contains("specification"));
        let missing = read_at(&opts(root, "state", None)).unwrap_err();
        assert_eq!(missing["reason"], json!("no-spec-file"));
    }

    /// O nome chutado entrega o bloco que o agente quis, com o aviso do nome
    /// certo, e a resposta diz o nome aceito no campo do bloco. O nome sem
    /// correspondência segue recusado, com a lista.
    ///
    /// Em campo (21/10/2026) foram cinquenta e uma recusas de nome chutado, e
    /// cada uma custava uma volta inteira do agente.
    #[test]
    fn a_guessed_block_name_delivers_the_right_block_with_the_name_said() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "state", json!({"author": "binary", "phase": "survey"}));

        for (asked, name) in [("tarefas", "backlog"), ("Contexto", "specification"), ("ondas", "waves"), ("métricas", "metrics")] {
            let text = read_at(&opts(root, asked, None)).unwrap_or_else(|e| panic!("{asked}: {e}"));
            let out: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{asked}: {text}"));
            assert_eq!(out["block"], json!(name), "a resposta diz o nome aceito: {text}");
            let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
            let said = warnings.iter().filter_map(|w| w.as_str()).collect::<Vec<_>>().join(" ");
            assert!(said.contains(asked) && said.contains(name), "o aviso diz os dois nomes: {text}");
        }

        // O nome que não é apelido de nada segue recusado.
        let unknown = read_at(&opts(root, "everything", None)).unwrap_err();
        assert_eq!(unknown["reason"], json!("unknown-block"), "{unknown}");
    }

    /// Cada registro diz de qual sessão veio, e a leitura por sessão separa
    /// as duas: `calls` e `metrics` com `--session` trazem só os daquela
    /// conversa. Sem a opção, trazem os de todas.
    ///
    /// Em campo (21/10/2026) uma conversa paralela no mesmo projeto gravou as
    /// leituras dela dentro da spec da conversa principal, e os números de
    /// consumo saíam somados.
    #[test]
    fn each_record_says_its_session_and_the_reading_separates_them() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "state", json!({"author": "binary", "phase": "survey"}));
        let call = |session: &str, command: &str| {
            by_program(root, "call", json!({"author": "binary", "command": command, "ms": 1, "result": "ok", "session": session}))
        };
        call("sessao-a", "round");
        call("sessao-b", "close");

        let commands = |session: Option<&str>| -> Vec<Value> {
            let mut read = opts(root, "calls", None);
            read.session = session.map(str::to_string);
            events(&read_at(&read).unwrap()).iter().map(|e| e["command"].clone()).collect()
        };
        assert_eq!(commands(None), [json!("close"), json!("round")], "sem a opção, as duas sessões");
        assert_eq!(commands(Some("sessao-a")), [json!("round")], "só a sessão pedida");
        assert_eq!(commands(Some("sessao-b")), [json!("close")], "só a sessão pedida");
        assert!(commands(Some("sessao-c")).is_empty(), "a sessão que não gravou nada");

        // O mesmo filtro no bloco das medidas.
        let mut metrics = opts(root, "metrics", None);
        metrics.session = Some("sessao-a".to_string());
        let only = events(&read_at(&metrics).unwrap());
        assert_eq!(only.len(), 1, "{only:?}");
        assert_eq!(only[0]["command"], json!("round"), "{only:?}");
    }

    #[test]
    fn the_same_item_lookup_reads_every_event_kind_by_its_canonical_code() {
        use mustard_core::domain::spec_events::{parse_log,TYPES};
        let text=TYPES.iter().enumerate().map(|(at,spec)|json!({"v":1,"id":at+1,"type":spec.name,"text":"Conteúdo completo."}).to_string()).collect::<Vec<_>>().join("\n");
        let log=parse_log(&text);let codes=log.codes();
        for event in &log.events {
            let code=codes.get(&event.id).expect("every event has a code");
            let found=find_item(&log,&EventRef::Code(code.clone()),&codes).expect("universal item lookup");
            assert_eq!(found.id,event.id,"{} {code}",event.event_type);
            assert_eq!(found.str_field("text"),Some("Conteúdo completo."));
        }
    }

    #[test]
    fn a_broken_line_shows_as_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        let path = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{quebrada\n");
        std::fs::write(&path, raw).unwrap();
        let report = read_at(&opts(root, "conversation", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["count"], json!(1));
        assert_eq!(parsed["warnings"].as_array().map(Vec::len), Some(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_of_the_current_branch() {
        // Uma sobreposição herdada responde primeiro, por desenho: o teste
        // pula, em vez de depender do shell que roda a suíte.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::spec_state::stand_on_spec_branch(root, "teste");
        // Uma sessão ligada a outra spec não vence a branch.
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "outra");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura"), root).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "the resolved name is shown: {report}");
        assert_eq!(parsed["count"], json!(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_bound_to_the_session() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "teste");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura"), root).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "{report}");
    }

    #[test]
    fn read_without_spec_and_without_a_current_spec_is_refused_in_both_languages() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let pt = read_for(&without_spec(root, "state"), None, root).unwrap_err();
        assert_eq!(pt["ok"], json!(false));
        assert_eq!(pt["reason"], json!("no-current-spec"));
        assert!(pt["hint"].as_str().unwrap().starts_with("Nenhuma spec atual"), "{pt}");

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let en = read_for(&without_spec(root, "state"), None, root).unwrap_err();
        assert_eq!(en["reason"], json!("no-current-spec"));
        assert!(en["hint"].as_str().unwrap().starts_with("No current spec"), "{en}");
    }

    /// Um evento gravado como o programa grava, sem a conferência de quem
    /// escreve de fora: o envio e a entrega, que só a rodada e o agente gravam.
    fn by_program(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        let path = store::spec_file(root, "teste").expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        store::write(&path, event_type, fields.as_object().cloned().expect("an object"), &[]).expect(event_type).id
    }

    /// A única linha que a leitura devolve.
    fn only_line(root: &std::path::Path, block: &str) -> (String, Value) {
        let report = read_at(&opts(root, block, None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 1, "{report}");
        (report, got[0].clone())
    }

    /// O plano das leituras de item: duas ondas, a tarefa 1 na onda 1 com
    /// `a.rs` e `b.rs`, a tarefa 2 na onda 2, e a versão nova da tarefa 1,
    /// que passa para a onda 2, troca `b.rs` por `d.rs` e passa a depender da
    /// tarefa 2. Devolve a mensagem, a versão antiga, a tarefa 2 e a nova.
    fn plan_with_a_new_version(root: &std::path::Path) -> (u64, u64, u64, u64) {
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        for n in [1, 2] {
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
            put(root, "wave", json!({"n": n, "text": "Onda.", "criteria": [c], "done_when": "x", "origin": said}));
        }
        let old = put(
            root,
            "task",
            json!({"wave": 1, "text": "Somar.", "files": [{"path": "a.rs"}, {"path": "b.rs"}],
            "depends_on": [], "origin": said}),
        );
        let other = put(root, "task", json!({"wave": 2, "text": "Mostrar.", "files": [{"path": "c.rs"}], "depends_on": [], "origin": said}));
        let new = put(
            root,
            "task",
            json!({"wave": 2, "text": "Somar e mostrar.", "files": [{"path": "a.rs"}, {"path": "d.rs"}],
            "depends_on": [other], "origin": said, "replaces": old}),
        );
        (said, old, other, new)
    }

    /// Pelo código, a leitura traz a versão vigente do item, com o código e o
    /// que mudou da versão anterior: o arquivo que entrou e o que saiu, a onda
    /// de quanto para quanto, a dependência nova e o texto que mudou.
    #[test]
    fn an_item_read_by_code_brings_the_current_version_with_what_changed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, old, other, new) = plan_with_a_new_version(root);

        let (report, item) = only_line(root, "item-MSTD-TASK-0001");
        assert_eq!(item["id"], json!(new), "{report}");
        assert_eq!(item["code"], json!("MSTD-TASK-0001"), "{report}");
        assert_eq!(item["replaces"], json!(old), "{report}");
        let expected = json!({
            "depends_on": {"added": [other], "removed": []},
            "files": {"added": ["d.rs"], "removed": ["b.rs"]},
            "text": true,
            "wave": {"from": 1, "to": 2},
        });
        assert_eq!(item["changed"], expected, "{report}");
        assert!(!report.contains("\"search\""), "{report}");
        assert_eq!(report, read_at(&opts(root, "item-MSTD-TASK-0001", None)).unwrap(), "the reading is byte-stable");
    }

    /// Pelo número, a leitura traz aquela versão, mesmo já substituída, e diz
    /// qual versão a substitui; a versão nova, pelo número, sai igual à que o
    /// código traz.
    #[test]
    fn an_item_read_by_number_brings_that_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, old, _, new) = plan_with_a_new_version(root);

        let (report, item) = only_line(root, &format!("item-{old}"));
        assert_eq!(item["id"], json!(old), "{report}");
        assert_eq!(item["wave"], json!(1), "{report}");
        assert_eq!(item["files"], json!([{"path": "a.rs"}, {"path": "b.rs"}]), "{report}");
        assert_eq!(item["replaced_by"], json!(new), "{report}");
        assert_eq!(item.get("changed"), None, "the first version replaces nothing: {report}");
        assert_eq!(item.get("removed"), None, "{report}");

        let (_, current) = only_line(root, &format!("item-{new}"));
        let (_, by_code) = only_line(root, "item-MSTD-TASK-0001");
        assert_eq!(current, by_code);
    }

    /// O registro que toma o lugar de uma linha cortada pelo disco cheio se lê
    /// pelo número e pelo código da linha que ele substituiu, sem aviso de
    /// linha pulada, e nenhuma outra leitura o mostra como item de trabalho:
    /// nem os blocos, nem o pedido de despacho, nem o backlog.
    #[test]
    fn a_cut_line_record_reads_by_number_and_never_shows_as_a_work_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, _, _, _) = plan_with_a_new_version(root);
        let next_id = put(root, "message", json!({"author": "user", "text": "outro"}));
        let path = store::spec_file(root, "teste").expect("spec file");
        let piece = r#"{"v":1,"id":10,"code":"MSTD-RULE-0009","at":"2026-09-11T09:03:00-03:00","type":"rule","text":"Regra do meio","keys":["meio"],"exa"#;
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str(&format!("{piece}\n"));
        std::fs::write(&path, raw).unwrap();
        assert_eq!(next_id, 9);

        let written = put(root, "message", json!({"author": "user", "text": "depois"}));
        assert_eq!(written, 11, "the cut line's number is kept");

        let (report, item) = only_line(root, "item-10");
        assert!(!report.contains("warnings"), "{report}");
        assert_eq!(item["type"], json!("cut_line"), "{report}");
        assert_eq!(item["piece"], json!(piece), "{report}");
        assert_eq!(item["code"], json!("MSTD-RULE-0009"), "{report}");
        let (_, by_code) = only_line(root, "item-MSTD-RULE-0009");
        assert_eq!(by_code, item, "the code of the lost line finds the record");

        let mut blocks: Vec<String> = Block::ALL.iter().map(|b| b.name().to_string()).collect();
        blocks.extend(["dispatch-1", "dispatch-2", "backlog", "wave-1", "wave-2"].map(str::to_string));
        for block in blocks {
            let report = read_at(&opts(root, &block, None)).unwrap();
            assert!(!report.contains("cut_line") && !report.contains("Regra do meio"), "{block}: {report}");
            assert!(!report.contains("warnings"), "{block}: {report}");
        }
    }

    /// O item removido ainda se lê pelo código, com a última versão e a marca
    /// de removido.
    #[test]
    fn a_removed_item_is_marked_removed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, _, _, _) = plan_with_a_new_version(root);
        let gone = put(root, "task", json!({"wave": 1, "text": "Apagar.", "files": [{"path": "e.rs"}], "depends_on": [], "origin": said}));
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));

        let (report, item) = only_line(root, "item-MSTD-TASK-0003");
        assert_eq!(item["id"], json!(gone), "{report}");
        assert_eq!(item["removed"], json!(true), "{report}");
        let (_, kept) = only_line(root, "item-MSTD-TASK-0002");
        assert_eq!(kept.get("removed"), None, "a task still in the plan is not marked");
    }

    /// A mensagem do usuário lida como item sai com o texto inteiro, o autor
    /// e o código, como qualquer outro item: o agente que a tarefa atende lê o
    /// que o usuário disse, pelo código ou pelo número.
    #[test]
    fn a_message_read_as_an_item_shows_its_whole_text_and_code() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "Pode liberar mais espaço, é voce que está lotando o disco"}));

        for block in [format!("item-{said}"), "item-MSTD-MSG-0001".to_string()] {
            let (report, item) = only_line(root, &block);
            assert_eq!(item["id"], json!(said), "{block}: {report}");
            assert_eq!(item["code"], json!("MSTD-MSG-0001"), "{block}: {report}");
            assert_eq!(item["author"], json!("user"), "{block}: {report}");
            assert_eq!(item["text"], json!("Pode liberar mais espaço, é voce que está lotando o disco"), "{block}: {report}");
        }
    }

    /// Com a volta do agente e a versão que a rodada assumiu, a leitura da
    /// entrega traz a assumida, com o que não foi cumprido do combinado, as
    /// sobras, o que ficou por fazer, os arquivos e o texto; sem a assumida,
    /// traz a volta do agente.
    #[test]
    fn the_delivery_reading_brings_the_assumed_delivery_over_the_agents_own() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        plan_with_a_new_version(root);
        let own = by_program(
            root,
            "delivered",
            json!({"author": "wave", "wave": 1, "text": "Do agente.", "files": ["a.rs"],
            "returned": true, "agreed": [{"item": "MSTD-RULE-0001", "met": false, "text": "falta"}]}),
        );

        let (report, alone) = only_line(root, "delivered-1");
        assert_eq!(alone["text"], json!("Do agente."), "without the assumed one, the agent's own: {report}");
        assert_eq!(alone["returned"], json!(true), "{report}");

        let assumed = by_program(
            root,
            "delivered",
            json!({"author": "binary", "wave": 1, "text": "Assumida.",
            "files": ["a.rs", "b.rs"], "replaces": [own],
            "agreed": [{"item": "MSTD-RULE-0001", "met": true}, {"item": "MSTD-RULE-0002", "met": false, "text": "falta o teste"}],
            "leftovers": [{"title": "Sobra", "detail": "no `x.rs`"}], "undone": ["MSTD-TASK-0002"]}),
        );
        let (report, got) = only_line(root, "delivered-1");
        assert_eq!(got["code"], code_of(root, assumed), "{report}");
        assert_eq!(got["text"], json!("Assumida."), "{report}");
        assert_eq!(got["agreed"], json!([{"item": "MSTD-RULE-0002", "met": false, "text": "falta o teste"}]), "{report}");
        assert_eq!(got["files"], json!(["a.rs", "b.rs"]), "{report}");
        assert_eq!(got["leftovers"], json!([{"title": "Sobra", "detail": "no `x.rs`"}]), "{report}");
        assert_eq!(got["undone"], json!(["MSTD-TASK-0002"]), "{report}");
        assert!(events(&read_at(&opts(root, "delivered-2", None)).unwrap()).is_empty(), "a wave with no delivery reads empty");
    }

    /// O backlog deixa de fora a tarefa da onda entregue e a tarefa removida;
    /// cada linha diz a onda, quantos arquivos a tarefa declara e de quais
    /// tarefas ainda por entregar ela depende, e o total conta as tarefas e os
    /// arquivos distintos.
    #[test]
    fn the_backlog_leaves_out_delivered_and_removed_tasks_and_counts_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        let mut criteria = Vec::new();
        for n in [1_u64, 2, 3] {
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
            let after: Vec<u64> = (1..n).collect();
            put(root, "wave", json!({"n": n, "text": "Onda.", "criteria": [c], "done_when": "x", "depends_on": after, "origin": said}));
            criteria.push(c);
        }
        let task = |wave: Option<u64>, files: &[&str], depends_on: &[u64]| -> u64 {
            let files: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
            let mut body = json!({"text": "Fazer.", "files": files, "depends_on": depends_on, "origin": said});
            match wave {
                Some(wave) => body["wave"] = json!(wave),
                None => body["covers"] = json!([criteria[0]]),
            }
            put(root, "task", body)
        };
        let delivered = task(Some(1), &["a.rs", "b.rs"], &[]);
        let second = task(Some(2), &["b.rs", "c.rs"], &[delivered]);
        let gone = task(Some(2), &["e.rs"], &[]);
        let third = task(Some(3), &["d.rs"], &[second]);
        let loose = task(None, &["c.rs"], &[]);
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": ["a.rs", "b.rs"]}));

        let report = read_at(&opts(root, "backlog", None)).unwrap();
        let expected = format!(
            "{{\"ok\":true,\"spec\":\"teste\",\"block\":\"backlog\",\"count\":3,\"events\":[\n\
             {{\"id\":{second},\"code\":\"MSTD-TASK-0002\",\"depends_on\":[],\"files\":2,\"wave\":2}},\n\
             {{\"id\":{third},\"code\":\"MSTD-TASK-0004\",\"depends_on\":[\"MSTD-TASK-0002\"],\"files\":1,\"wave\":3}},\n\
             {{\"id\":{loose},\"code\":\"MSTD-TASK-0005\",\"depends_on\":[],\"files\":1,\"wave\":null}}\n\
             ],\"total\":{{\"tasks\":3,\"files\":3}}}}"
        );
        assert_eq!(report, expected);
    }

    /// A lista das ondas traz uma linha curta por onda, na ordem do número
    /// e não na do arquivo: o código, o número, a primeira linha não vazia do
    /// texto e as ondas de que ela depende. A versão substituída e a onda
    /// removida ficam de fora, e a tarefa, o pedido e a entrega não entram.
    #[test]
    fn the_wave_list_brings_one_short_line_per_wave_in_number_order() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        let wave = |n: u64, text: &str, depends_on: &[u64], replaces: Option<u64>| -> u64 {
            let mut body = json!({"n": n, "text": text, "criteria": [c], "done_when": "x", "depends_on": depends_on, "origin": said});
            if let Some(old) = replaces {
                body["replaces"] = json!(old);
            }
            put(root, "wave", body)
        };
        let old_first = wave(1, "Somar.", &[], None);
        let third = wave(3, "Mostrar o total.\nCom o detalhe de cada parcela.", &[1, 2], None);
        let second = wave(2, "\n  Guardar a soma.  \n\nE o resto.", &[1], None);
        let gone = wave(4, "Sair.", &[], None);
        let first = wave(1, "Somar as parcelas.", &[], Some(old_first));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));
        by_program(
            root,
            "send",
            json!({"author": "binary", "wave": 1, "role": "wave", "text": "O pedido inteiro da onda.",
            "lines": 1, "chars": 25, "mustard": "0.2.0"}),
        );
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": ["a.rs"]}));

        let report = read_at(&opts(root, "wave-list", None)).unwrap();
        let expected = format!(
            "{{\"ok\":true,\"spec\":\"teste\",\"block\":\"wave-list\",\"count\":3,\"events\":[\n\
             {{\"id\":{first},\"code\":\"MSTD-WAVE-0001\",\"depends_on\":[],\"first_line\":\"Somar as parcelas.\",\"n\":1}},\n\
             {{\"id\":{second},\"code\":\"MSTD-WAVE-0003\",\"depends_on\":[1],\"first_line\":\"Guardar a soma.\",\"n\":2}},\n\
             {{\"id\":{third},\"code\":\"MSTD-WAVE-0002\",\"depends_on\":[1,2],\"first_line\":\"Mostrar o total.\",\"n\":3}}\n\
             ]}}"
        );
        assert_eq!(report, expected);
    }

    /// Uma chamada gravada como o programa a grava, com o autor e o comando.
    fn call(root: &std::path::Path, command: &str, fields: Value) -> u64 {
        let mut body = json!({"author": "binary", "command": command, "result": "ok"});
        if let (Some(body), Some(fields)) = (body.as_object_mut(), fields.as_object()) {
            body.extend(fields.clone());
        }
        by_program(root, "call", body)
    }

    /// O nome do comando acha as chamadas dele, da mais nova à mais antiga, sem olhar
    /// maiúsculas nem espaços de sobra; a busca por nota, que acharia a
    /// entrega que cita a busca, não entra, e a chamada de outro comando fica
    /// de fora.
    #[test]
    fn the_name_of_a_command_finds_its_calls() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = call(root, "map search", json!({"ms": 1800}));
        call(root, "round", json!({"ms": 3}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "A map search ficou pronta.", "files": []}));
        let second = call(root, "map search", json!({"ms": 900}));

        let ids =
            |block: &str, term: &str| -> Vec<Value> { events(&read_at(&opts(root, block, Some(term))).unwrap()).iter().map(|e| e["code"].clone()).collect() };
        let (second, first) = (code_of(root, second), code_of(root, first));
        assert_eq!(ids("metrics", "map search"), [second.clone(), first.clone()]);
        assert_eq!(ids("conversation", " Map  Search "), [second, first]);
        assert_eq!(ids("metrics", "pronta").len(), 1, "a word that names no command still searches by score");
    }

    /// A soma das chamadas traz, por comando, quantas foram, as falhas por
    /// motivo, a mediana, o p90 e o pior do tempo da chamada e do tempo do
    /// filtro pela posição mais próxima, sem média entre dois valores, e os
    /// tokens e o custo somados; o comando sem filtro fica sem os campos dele,
    /// e o nome do comando deixa só a linha dele.
    #[test]
    fn the_calls_reading_sums_each_command() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let jev = |ms: u64, filter_ms: u64, tokens: u64, cost: u64| json!({"ms": ms, "filter": "jev", "filter_ms": filter_ms, "tokens": tokens, "cost_micro_usd": cost});
        call(root, "map search", jev(100, 90, 20_000, 840));
        call(root, "map search", jev(900, 800, 21_000, 882));
        call(root, "map search", json!({"ms": 300, "filter": "jev:busy", "filter_ms": 40}));
        call(root, "map search", jev(700, 600, 22_000, 924));
        call(root, "round", json!({"ms": 3}));
        call(root, "map search", jev(500, 400, 20_000, 840));
        call(root, "map search", json!({"ms": 200, "result": "refused", "refusal": "no-map"}));
        call(root, "map search", jev(800, 700, 21_000, 882));
        call(root, "round", json!({"ms": 7, "result": "refused"}));
        call(root, "map search", json!({"ms": 400, "filter": "jev:busy", "filter_ms": 60}));
        call(root, "map search", jev(1000, 950, 22_000, 924));
        call(root, "round", json!({"ms": 5}));
        call(root, "map search", jev(600, 500, 20_000, 840));

        let search = "{\"command\":\"map search\",\"cost_micro_usd\":6132,\"count\":10,\
            \"failures\":{\"jev:busy\":2,\"no-map\":1},\"filter_ms\":{\"max\":950,\"median\":500,\"p90\":950},\
            \"ms\":{\"max\":1000,\"median\":500,\"p90\":900},\"tokens\":146000}";
        let round = "{\"command\":\"round\",\"count\":3,\"failures\":{\"refused\":1},\"ms\":{\"max\":7,\"median\":5,\"p90\":7}}";
        let envelope = |count: usize, lines: &str| format!("{{\"ok\":true,\"spec\":\"teste\",\"block\":\"calls\",\"count\":{count},\"events\":[\n{lines}\n]}}");
        assert_eq!(read_at(&opts(root, "calls", None)).unwrap(), envelope(2, &format!("{search},\n{round}")));
        assert_eq!(read_at(&opts(root, "calls", Some("map search"))).unwrap(), envelope(1, search));
        assert!(events(&read_at(&opts(root, "calls", Some("close"))).unwrap()).is_empty());
    }

    /// O estado traz o número da última mensagem do usuário, sem o texto
    /// dela; a mensagem de outro autor, depois, não conta.
    #[test]
    fn the_state_block_carries_the_last_user_message_number() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "state", json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
        put(root, "message", json!({"author": "user", "text": "primeiro"}));
        let last = put(root, "message", json!({"author": "user", "text": "o último pedido"}));
        by_program(root, "message", json!({"author": "binary", "text": "aviso do programa"}));

        let report = read_at(&opts(root, "state", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).expect("the report is JSON");
        assert_eq!(parsed["last_user_message"], json!(last), "{report}");
        assert!(!report.contains("o último pedido"), "{report}");
    }

    /// O estado traz só a cópia mais nova de cada página, em vez de uma cópia
    /// por rodada: com sete cópias da página da spec e três da página do
    /// projeto, a leitura mostra duas, cada uma a última da sua página, e os
    /// outros eventos do estado seguem todos.
    #[test]
    fn the_state_block_carries_only_the_newest_copy_of_each_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opened = by_program(root, "state", json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
        let mut spec_copies = Vec::new();
        for last in 1..=7 {
            spec_copies.push(by_program(root, "copy", json!({"author": "binary", "page": "spec", "last": last * 10})));
            if last % 3 == 0 {
                by_program(root, "copy", json!({"author": "binary", "page": "project", "phase": "plan"}));
            }
        }
        let running = by_program(root, "state", json!({"author": "binary", "phase": "running"}));

        let report = read_at(&opts(root, "state", None)).unwrap();
        let got = events(&report);
        let kinds: Vec<(&str, Value)> = got.iter().map(|e| (e["type"].as_str().unwrap(), e["code"].clone())).collect();
        let copies: Vec<&Value> = got.iter().filter(|e| e["type"] == json!("copy")).collect();
        assert_eq!(copies.len(), 2, "one copy per page: {report}");
        assert_eq!(
            copies.iter().find(|e| e["page"] == json!("spec")).map(|e| e["code"].clone()),
            spec_copies.last().map(|id| code_of(root, *id)),
            "the spec page keeps its newest copy: {report}"
        );
        assert!(copies.iter().any(|e| e["page"] == json!("project")), "the project page keeps a copy: {report}");
        assert!(kinds.contains(&("state", code_of(root, opened))) && kinds.contains(&("state", code_of(root, running))), "{report}");
        assert_eq!(got.len(), 4, "{report}");
    }

    /// O plano dos testes do registro: uma onda com uma tarefa e uma regra,
    /// e o envio dela com a vaga `copy` (a pasta `copias/a` dentro do
    /// projeto, com uma subpasta de código). Devolve a vaga, o código da regra
    /// e o número do envio.
    fn open_request(root: &std::path::Path) -> (PathBuf, u64) {
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        put(root, "rule", json!({"title": "Regra", "text": "Vale sempre.", "agent": "detalhe", "keys": ["regra"], "example": "e", "origin": said}));
        let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        put(root, "wave", json!({"n": 1, "text": "Onda.", "criteria": [c], "done_when": "x", "origin": said}));
        put(root, "task", json!({"wave": 1, "text": "Fazer.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        let copy = root.join("copias").join("a");
        std::fs::create_dir_all(copy.join("apps").join("rt")).unwrap();
        let sent = by_program(
            root,
            "send",
            json!({"author": "binary", "wave": 1, "role": "wave", "text": "pedido", "lines": 1,
            "chars": 6, "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&copy)}),
        );
        (copy, sent)
    }

    /// As leituras registradas, na ordem do arquivo: o pedido e cada item dos
    /// recibos `read`.
    fn recorded_reads(root: &std::path::Path) -> Vec<(String, String)> {
        let path = store::spec_file(root, "teste").expect("spec file");
        let log = store::read(&path).unwrap().unwrap();
        log.visible()
            .into_iter()
            .filter(|e| e.event_type == "call" && e.str_field("command") == Some("read"))
            .flat_map(|e| {
                assert_eq!(e.str_field("author"), Some("binary"));
                let request = e.str_field("request").unwrap_or_default().to_string();
                let items: Vec<String> = e.fields["items"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                items.into_iter().map(move |item| (request.clone(), item))
            })
            .collect()
    }

    fn read_from(root: &std::path::Path, from: &std::path::Path, block: &str, term: Option<&str>) -> String {
        read_for(&opts(root, block, term), None, from).unwrap()
    }

    /// Ler um item de dentro da vaga do envio aberto, ou de uma pasta dentro
    /// dela, grava a leitura com o pedido e o item; ler o mesmo item de
    /// outra pasta não grava, nem ler um código que não existe.
    #[test]
    fn an_item_read_from_inside_the_copy_of_an_open_request_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);

        read_from(root, &copy, "item-MSTD-RULE-0001", None);
        read_from(root, &copy.join("apps").join("rt"), "item-MSTD-TASK-0001", None);
        assert_eq!(recorded_reads(root), [("request-1".into(), "MSTD-RULE-0001".into()), ("request-1".into(), "MSTD-TASK-0001".into())]);

        read_from(root, root, "item-MSTD-RULE-0001", None);
        read_from(root, &copy.parent().unwrap().join("outra"), "item-MSTD-RULE-0001", None);
        read_from(root, &copy, "item-MSTD-RULE-0099", None);
        assert_eq!(recorded_reads(root).len(), 2, "outra pasta e código que não existe não gravam");
    }

    /// A onda cuja tarefa ganhou versão nova depois do envio segue com o
    /// agente na cópia dela: a leitura de dentro da vaga ainda grava o recibo
    /// do pedido, que a entrega confere.
    #[test]
    fn an_item_read_from_the_copy_of_a_replanned_wave_is_still_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        put(root, "task", json!({"wave": 1, "text": "Fazer de outro jeito.", "files": [{"path": "a.rs"}], "depends_on": [], "replaces": "MSTD-TASK-0001", "origin": 1}));
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        assert!(crate::commands::flow::round::queue::open_sends(&log).is_empty(), "the wave was replanned after the send");
        read_from(root, &copy, "item-MSTD-RULE-0001", None);
        assert_eq!(recorded_reads(root), [("request-1".into(), "MSTD-RULE-0001".into())]);
    }

    /// Pelo número, o item grava o código dele; a lição grava
    /// `lesson-<número>` e só quando o banco a tem.
    #[test]
    fn a_read_by_number_records_the_code_and_a_lesson_records_its_number() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let rule = put(root, "rule", json!({"title": "Outra", "text": "Vale também.", "agent": "d", "keys": ["outra"], "example": "e", "origin": 1}));
        let bank = mustard_core::ClaudePaths::for_project(root).unwrap().lessons_path();
        let lesson = write_lesson(&bank, "Nunca comitar sem rodar a suíte inteira.", &["suíte"]);

        read_from(root, &copy, &format!("item-{rule}"), None);
        read_from(root, &copy, "lessons", Some(&lesson.to_string()));
        read_from(root, &copy, "lessons", Some("999"));
        assert_eq!(recorded_reads(root), [("request-1".into(), "MSTD-RULE-0002".into()), ("request-1".into(), format!("lesson-{lesson}"))]);
    }

    /// Ler `dispatch-<n>` de dentro da cópia da onda `n` conta todos os
    /// itens e lições que ele lista; de dentro da cópia de outra onda, ou de
    /// outra pasta, não conta nada.
    #[test]
    fn the_dispatch_read_from_the_copy_of_its_wave_records_everything_it_lists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let report = read_from(root, root, "dispatch-1", None);
        let listed: Vec<String> = events(&report).iter().map(|e| e["code"].as_str().expect("every listed item has a code").to_string()).collect();
        assert!(listed.contains(&"MSTD-TASK-0001".to_string()) && listed.contains(&"MSTD-WAVE-0001".to_string()), "{listed:?}");
        assert!(recorded_reads(root).is_empty(), "lido de fora da cópia não grava");

        read_from(root, &copy, "dispatch-1", None);
        let recorded: Vec<String> = recorded_reads(root)
            .into_iter()
            .map(|(request, item)| {
                assert_eq!(request, "request-1");
                item
            })
            .collect();
        assert_eq!(recorded, listed);

        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": 1}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": 1}));
        read_from(root, &copy, "dispatch-2", None);
        assert_eq!(recorded_reads(root).len(), listed.len(), "o pedido da onda 2 não se lê da cópia da onda 1");
    }

    /// Ler `dispatch-<n>` de dentro da cópia grava um recibo só, com todos os
    /// itens que a leitura listou: um recibo por item regravava a spec
    /// inteira a cada um, e a leitura de uma onda grande levava vinte
    /// segundos.
    #[test]
    fn a_dispatch_read_from_the_copy_records_one_receipt_with_every_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let listed: Vec<Value> = events(&read_from(root, root, "dispatch-1", None)).iter().map(|e| e["code"].clone()).collect();
        assert!(listed.len() > 1, "{listed:?}");
        let path = store::spec_file(root, "teste").unwrap();
        let before = store::read(&path).unwrap().unwrap().events.len();

        read_from(root, &copy, "dispatch-1", None);
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.events.len(), before + 1, "one write for the whole read");
        let receipt = log.events.last().unwrap();
        assert_eq!((receipt.event_type.as_str(), receipt.str_field("request")), ("call", Some("request-1")));
        assert_eq!(receipt.fields.get("items"), Some(&Value::Array(listed)));
    }

    /// O envio da onda 1 de `open_request` com a lista do que o pedido
    /// listou, como a rodada o grava: a onda, o critério e a mensagem ficam de
    /// fora dela.
    fn listed_request(root: &std::path::Path, listed: &[&str]) -> PathBuf {
        let (copy, sent) = open_request(root);
        let path = store::spec_file(root, "teste").unwrap();
        let mut fields = store::read(&path).unwrap().unwrap().get(sent).unwrap().fields.clone();
        for key in ["v", "id", "code", "at", "type", "search"] {
            fields.remove(key);
        }
        fields.insert("replaces".into(), json!(sent));
        fields.insert("read_items".into(), json!(listed));
        by_program(root, "send", Value::Object(fields));
        copy
    }

    /// O lote da onda traz a lista que o envio gravou, na ordem dela: nem um
    /// item a mais que o pedido — a onda, o critério e a mensagem que ele não
    /// lista —, nem um a menos. O recibo leva a mesma lista.
    #[test]
    fn the_dispatch_read_brings_exactly_the_items_the_request_listed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = listed_request(root, &["MSTD-TASK-0001", "MSTD-RULE-0001"]);
        let text = read_from(root, &copy, "dispatch-1", None);
        assert_eq!(recorded_reads(root), [("request-1".into(), "MSTD-TASK-0001".into()), ("request-1".into(), "MSTD-RULE-0001".into())]);
        assert!(text.contains("MSTD-TASK-0001") && text.contains("MSTD-RULE-0001"), "{text}");
        assert!(!text.contains("MSTD-WAVE-0001") && !text.contains("MSTD-CRIT-0001"), "{text}");
        let outside: Vec<Value> = events(&read_from(root, root, "dispatch-1", None)).iter().map(|e| e["code"].clone()).collect();
        assert_eq!(outside, [json!("MSTD-TASK-0001"), json!("MSTD-RULE-0001")]);
    }

    /// O pedido de `open_request` com uma segunda tarefa, que atende uma
    /// decisão só dela, enviado como a rodada o grava: a lista do que ele
    /// lista e o bloco de cada tarefa. Só a primeira está liberada. Devolve a
    /// vaga.
    fn two_task_request(root: &std::path::Path) -> PathBuf {
        let (copy, sent) = open_request(root);
        put(root, "decision", json!({"title": "Em reais", "text": "O total fica em reais.", "agent": "centavos inteiros", "why": "o caixa", "keys": ["total"], "origin": 1}));
        put(root, "task", json!({"wave": 1, "text": "Escrever o total.", "files": [{"path": "a.rs"}], "depends_on": [], "covers": ["MSTD-DEC-0001"], "origin": 1}));
        let path = store::spec_file(root, "teste").unwrap();
        let mut fields = store::read(&path).unwrap().unwrap().get(sent).unwrap().fields.clone();
        for key in ["v", "id", "code", "at", "type", "search"] {
            fields.remove(key);
        }
        fields.insert("replaces".into(), json!(sent));
        fields.insert("read_items".into(), json!(["MSTD-TASK-0001", "MSTD-MSG-0001", "MSTD-RULE-0001", "MSTD-TASK-0002", "MSTD-DEC-0001"]));
        fields.insert(
            "tasks".into(),
            json!([
                {"task": "MSTD-TASK-0001", "text": "### Tarefa 1 de 2", "items": ["MSTD-TASK-0001", "MSTD-MSG-0001"]},
                {"task": "MSTD-TASK-0002", "text": "### Tarefa 2 de 2", "items": ["MSTD-TASK-0002", "MSTD-DEC-0001"]},
            ]),
        );
        by_program(root, "send", Value::Object(fields));
        copy
    }

    /// De dentro da cópia, ler a tarefa que ainda não foi liberada, ou o item
    /// que só ela atende, é recusado com o código da tarefa; a tarefa atual e
    /// as regras da onda se leem. De fora da cópia, quem conduz e quem revisa
    /// leem tudo. Liberada, a tarefa se lê de dentro também.
    #[test]
    fn a_task_not_yet_released_is_refused_from_inside_the_copy_and_read_from_outside() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_request(root);
        let expected = translate("spec_events.task_not_released", Locale::PtBr).replace("{task}", "MSTD-TASK-0002");
        for ahead in ["item-MSTD-TASK-0002", "item-MSTD-DEC-0001"] {
            let refused = read_for(&opts(root, ahead, None), None, &copy).expect_err(ahead);
            assert_eq!(refused["reason"], json!("task-not-released"), "{ahead}: {refused}");
            assert_eq!(refused["hint"], json!(expected), "{ahead}: {refused}");
            assert!(read_for(&opts(root, ahead, None), None, root).is_ok(), "{ahead} reads from outside the copy");
        }
        for current in ["item-MSTD-TASK-0001", "item-MSTD-RULE-0001"] {
            assert!(read_for(&opts(root, current, None), None, &copy).is_ok(), "{current}");
        }
        assert!(!recorded_reads(root).iter().any(|(_, item)| item == "MSTD-TASK-0002"), "a refused read leaves no receipt");

        call(root, "read", json!({"ms": 0, "request": "request-1", "items": ["MSTD-TASK-0002", "MSTD-DEC-0001"], "released": "MSTD-TASK-0002"}));
        assert!(read_for(&opts(root, "item-MSTD-DEC-0001", None), None, &copy).is_ok(), "released, it reads from inside");
    }

    /// `dispatch-<n>` de dentro da cópia traz só o que as tarefas liberadas e
    /// a onda inteira pedem, e o recibo leva só isso; de fora, a lista
    /// inteira, para a revisão.
    #[test]
    fn the_dispatch_read_from_inside_the_copy_brings_only_the_released_tasks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_request(root);
        let inside = read_from(root, &copy, "dispatch-1", None);
        assert!(inside.contains("MSTD-TASK-0001") && inside.contains("MSTD-RULE-0001"), "{inside}");
        assert!(!inside.contains("MSTD-TASK-0002") && !inside.contains("MSTD-DEC-0001"), "{inside}");
        let read: Vec<String> = recorded_reads(root).into_iter().map(|(_, item)| item).collect();
        assert_eq!(read, ["MSTD-TASK-0001", "MSTD-MSG-0001", "MSTD-RULE-0001"]);
        let outside: Vec<Value> = events(&read_from(root, root, "dispatch-1", None)).iter().map(|e| e["code"].clone()).collect();
        assert_eq!(outside.len(), 5, "{outside:?}");
    }

    /// De dentro da cópia, as leituras em bloco — a onda, o combinado, um
    /// envio pelo código e o lote do revisor — mostram a tarefa ainda não
    /// liberada, e o que só ela atende, só pelo código, pelo tipo e pelo
    /// título, e o envio sem os blocos das tarefas. De fora, tudo inteiro.
    #[test]
    fn the_block_reads_from_inside_the_copy_never_bring_a_task_not_released() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_request(root);
        let path = store::spec_file(root, "teste").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.last_by_wave("send")[&1];
        let send_code = log.codes()[&sent].clone();
        let review = root.join("copias").join("revisao");
        std::fs::create_dir_all(&review).unwrap();
        by_program(
            root,
            "send",
            json!({"author": "binary", "role": "review", "text": "revise", "lines": 1, "chars": 6, "mustard": "0",
            "copy": mustard_core::io::wave_prompt::shown(&review), "read_items": ["MSTD-TASK-0002", "MSTD-DEC-0001"]}),
        );
        let line_of = |report: &str, code: &str| events(report).into_iter().find(|e| e["code"] == json!(code));

        let wave = read_from(root, &copy, "wave-1", None);
        let task = line_of(&wave, "MSTD-TASK-0002").unwrap_or_else(|| panic!("the rest of the wave by code: {wave}"));
        assert_eq!(task.as_object().map(|fields| fields.len()), Some(3), "only code, type and title: {task}");
        let send = line_of(&wave, &send_code).unwrap_or_else(|| panic!("{wave}"));
        assert!(send.get("tasks").is_none(), "the send comes without the task blocks: {send}");
        let agreed = read_from(root, &copy, "agreed", None);
        assert!(!agreed.contains("centavos inteiros"), "{agreed}");
        let item = read_from(root, &copy, &format!("item-{send_code}"), None);
        assert!(!item.contains("### Tarefa 2 de 2"), "{item}");
        let batch = read_from(root, &copy, "dispatch-review", None);
        assert!(!batch.contains("centavos inteiros") && !batch.contains("Escrever o total."), "{batch}");

        let outside = read_from(root, root, "wave-1", None);
        assert!(line_of(&outside, "MSTD-TASK-0002").is_some_and(|task| task.get("text").is_some()), "{outside}");
        assert!(read_from(root, root, "agreed", None).contains("centavos inteiros"));
    }

    /// `dispatch-review` traz, numa leitura só, os itens que o pedido mais
    /// novo do revisor final listou. De dentro da cópia do revisor grava um
    /// recibo só, com todos; de fora, não grava nada.
    #[test]
    fn the_review_dispatch_read_records_one_receipt_with_every_listed_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        open_request(root);
        let review = root.join("copias").join("revisao");
        std::fs::create_dir_all(&review).unwrap();
        by_program(
            root,
            "send",
            json!({"author": "binary", "role": "review", "text": "revise", "lines": 1, "chars": 6, "mustard": "0",
            "copy": mustard_core::io::wave_prompt::shown(&review), "read_items": ["MSTD-RULE-0001", "MSTD-TASK-0001"]}),
        );
        let outside: Vec<Value> = events(&read_from(root, root, "dispatch-review", None)).iter().map(|e| e["code"].clone()).collect();
        assert_eq!(outside, [json!("MSTD-RULE-0001"), json!("MSTD-TASK-0001")]);
        assert!(recorded_reads(root).is_empty(), "lido de fora da cópia não grava");

        let path = store::spec_file(root, "teste").unwrap();
        let before = store::read(&path).unwrap().unwrap().events.len();
        let text = read_from(root, &review, "dispatch-review", None);
        assert!(text.contains("MSTD-RULE-0001") && text.contains("MSTD-TASK-0001"), "{text}");
        assert_eq!(store::read(&path).unwrap().unwrap().events.len(), before + 1, "one receipt for the whole read");
        assert_eq!(recorded_reads(root), [("request-review".into(), "MSTD-RULE-0001".into()), ("request-review".into(), "MSTD-TASK-0001".into())]);
    }

    /// O bloco das chamadas e o painel juntam as duas fontes: a chamada que
    /// a spec antiga gravou nela mesma e a que mora no arquivo de operações.
    #[test]
    fn the_calls_and_metrics_blocks_join_the_spec_and_the_operations_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        call(root, "round", json!({"ms": 3}));
        let ok = json!({"ok": true, "spec": "teste"});
        assert!(crate::commands::spec_events::conversation::record_output_call(root, "search", Some("teste"), Instant::now(), &ok, "x\n").is_some());

        let commands: Vec<Value> = events(&read_at(&opts(root, "calls", None)).unwrap()).iter().map(|e| e["command"].clone()).collect();
        assert_eq!(commands, [json!("round"), json!("search")]);
        let panel: Vec<Value> =
            events(&read_at(&opts(root, "metrics", None)).unwrap()).iter().filter(|e| e["type"] == json!("call")).map(|e| e["command"].clone()).collect();
        assert_eq!(panel, [json!("search"), json!("round")], "the panel lists the newest first");
    }

    /// O envio de revisão aberto grava `request-review`, e o pedido que já
    /// tem entrega ou veredito deixa de estar aberto e não grava mais.
    #[test]
    fn the_review_request_and_a_closed_request_are_told_apart() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let review = root.join("copias").join("revisao");
        std::fs::create_dir_all(&review).unwrap();
        by_program(
            root,
            "send",
            json!({"author": "binary", "role": "review", "text": "revise", "lines": 1, "chars": 6,
            "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&review)}),
        );

        read_from(root, &review, "item-MSTD-RULE-0001", None);
        assert_eq!(recorded_reads(root), [("request-review".into(), "MSTD-RULE-0001".into())]);

        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": []}));
        read_from(root, &copy, "item-MSTD-RULE-0001", None);
        assert_eq!(recorded_reads(root).len(), 1, "a onda entregue não tem mais pedido aberto");
    }

    /// Os campos que a leitura de quem trabalha na cópia deixa de fora.
    const STAMPS: [&str; 4] = ["v", "at", "author", "keys"];

    /// A regra do plano de `open_request` ganha uma versão nova, com outro
    /// autor e outras palavras de busca, para o `changed` ter o que dizer.
    fn revise_the_rule(root: &std::path::Path) {
        let old = events(&read_from(root, root, "item-MSTD-RULE-0001", None))[0]["id"].as_u64().unwrap();
        by_program(
            root,
            "rule",
            json!({"author": "binary", "title": "Regra", "text": "Vale sempre, agora.", "agent": "detalhe",
            "keys": ["regra", "nova"], "example": "e", "origin": 1, "replaces": old}),
        );
    }

    /// De dentro da cópia de um pedido aberto, o item vem sem as marcas do
    /// binário e sem as palavras de busca, e o `changed` sem o autor e sem as
    /// palavras; o texto, o título, o que é para o agente, o exemplo e os
    /// números que se seguem ficam. De fora da cópia, a linha vem inteira.
    #[test]
    fn an_item_read_from_inside_the_copy_comes_without_the_stamps_and_the_search_words() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        revise_the_rule(root);

        let whole = events(&read_from(root, root, "item-MSTD-RULE-0001", None))[0].clone();
        for key in STAMPS {
            assert!(whole.get(key).is_some(), "outside the copy the line keeps {key}: {whole}");
        }
        assert_eq!(whole["changed"]["author"], json!(true), "{whole}");
        assert!(whole["changed"]["keys"].is_object(), "{whole}");

        for from in [copy.clone(), copy.join("apps").join("rt")] {
            let seen = events(&read_from(root, &from, "item-MSTD-RULE-0001", None))[0].clone();
            for key in STAMPS {
                assert!(seen.get(key).is_none(), "inside the copy the line leaves out {key}: {seen}");
            }
            assert_eq!(seen["changed"], json!({"text": true}), "{seen}");
            for key in ["id", "code", "type", "title", "text", "agent", "example", "origin", "replaces"] {
                assert_eq!(seen[key], whole[key], "{key} stays: {seen}");
            }
        }
    }

    /// A mensagem do usuário guarda o `at` e o `author`, que dizem quem falou
    /// e quando, mesmo lida de dentro da cópia; as marcas do binário que não
    /// dizem nada dela saem.
    #[test]
    fn a_user_message_read_from_inside_the_copy_keeps_who_said_it_and_when() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);

        let message = events(&read_from(root, &copy, "item-1", None))[0].clone();
        assert_eq!(message["type"], json!("message"), "{message}");
        assert_eq!(message["author"], json!("user"), "{message}");
        assert!(message["at"].is_string(), "{message}");
        assert_eq!(message["text"], json!("o pedido"), "{message}");
        assert!(message.get("v").is_none(), "{message}");
    }

    /// De dentro da cópia de um pedido aberto, a leitura da onda sai em texto:
    /// um bloco por item, com o título e o código e, embaixo, só a parte do
    /// agente; a parte do usuário e o exemplo ficam para a leitura do item. De
    /// fora da cópia, a linha vem na forma enxuta, com as palavras de busca.
    #[test]
    fn the_dispatch_read_from_inside_the_copy_comes_as_text_with_the_title_and_the_agent_part() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        put(
            root,
            "rule",
            json!({"title": "Global", "text": "Vale para tudo.", "agent": "Conferir o mapa antes.", "keys": ["global"], "example": "Um exemplo longo.", "origin": 1,
            "applies_to": {"files": ["**"]}}),
        );

        let outside = events(&read_from(root, root, "dispatch-1", None));
        assert!(outside.iter().any(|e| e["type"] == json!("rule")), "the dispatch lists the global rule: {outside:?}");
        assert!(outside.iter().all(|e| e.get("v").is_none() && e.get("code").is_some()), "{outside:?}");
        assert!(outside.iter().any(|e| e.get("keys").is_some()), "{outside:?}");

        let inside = read_from(root, &copy, "dispatch-1", None);
        assert!(!inside.contains('{'), "the copy reads text, not JSON: {inside}");
        let heads = inside.lines().filter(|line| line.starts_with("## ")).count();
        assert_eq!(heads, outside.len(), "one block per item: {inside}");
        let global = outside.iter().find(|e| e["title"] == json!("Global")).and_then(|e| e["code"].as_str()).expect("the global rule").to_string();
        assert!(inside.contains(&format!("## Global | {global}\nConferir o mapa antes.")), "{inside}");
        assert!(!inside.contains("Vale para tudo.") && !inside.contains("Um exemplo longo."), "the user part and the example stay out: {inside}");
        assert!(inside.lines().any(|line| line.starts_with("## ") && line.contains(" | MSTD-MSG-")), "the user message keeps its block: {inside}");
        for key in STAMPS {
            assert!(!inside.contains(&format!("\"{key}\"")), "{key} left out: {inside}");
        }
    }

    /// O painel sem termo vem sem o texto do envio, da injeção e da entrega,
    /// que eram mais da metade da saída; o tamanho, o gancho, o bloqueio e o
    /// tempo seguem. Com um termo, o texto vem; o código do item traz o texto
    /// inteiro; e a lista das ondas segue com o texto da entrega.
    #[test]
    fn the_panel_without_a_term_leaves_out_the_long_text_of_injections_and_deliveries() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let injection = by_program(root, "injection", json!({"author": "hook", "hook": "prompt_entry", "chars": 23, "text": "Texto longo da injecao."}));
        by_program(root, "hook", json!({"author": "hook", "hook": "write_gate", "tool": "Write", "action": "block", "reason": "o motivo do bloqueio"}));
        call(root, "round", json!({"ms": 3}));
        by_program(
            root,
            "send",
            json!({"author": "binary", "wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0", "copy": "x"}),
        );
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "O relato da entrega.", "files": ["a.rs"]}));

        let panel = events(&read_at(&opts(root, "metrics", None)).unwrap());
        let of = |kind: &str| panel.iter().find(|e| e["type"] == json!(kind)).unwrap_or_else(|| panic!("no {kind}: {panel:?}")).clone();
        for kind in ["injection", "delivered", "send"] {
            assert!(of(kind).get("text").is_none(), "the panel leaves out the text of {kind}: {}", of(kind));
        }
        assert_eq!((of("injection")["chars"].clone(), of("injection")["hook"].clone()), (json!(23), json!("prompt_entry")));
        assert_eq!(of("delivered")["files"], json!(["a.rs"]));
        assert_eq!(of("hook")["reason"], json!("o motivo do bloqueio"));
        assert_eq!(of("call")["ms"], json!(3));

        let found = events(&read_at(&opts(root, "metrics", Some("longo"))).unwrap());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0]["text"], json!("Texto longo da injecao."), "a term asks for the content");

        let by_code = events(&read_at(&opts(root, &format!("item-{}", of("injection")["code"].as_str().unwrap()), None)).unwrap());
        assert_eq!(by_code[0]["id"], json!(injection));
        assert_eq!(by_code[0]["text"], json!("Texto longo da injecao."));

        let waves = events(&read_at(&opts(root, "waves", None)).unwrap());
        let delivered = waves.iter().find(|e| e["type"] == json!("delivered")).expect("the waves list the delivery");
        assert_eq!(delivered["text"], json!("O relato da entrega."), "the plan keeps the delivery text");
        let send = waves.iter().find(|e| e["type"] == json!("send")).expect("the waves list the send");
        assert!(send.get("text").is_none(), "{send}");
    }

    /// O estado traz só a publicação mais nova de cada página, como faz com
    /// as cópias: cada marco publica de novo, e a anterior fica sem leitor.
    #[test]
    fn the_state_block_carries_only_the_newest_publication_of_each_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "state", json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
        let publish = |page: &str, milestone: &str| {
            by_program(root, "publish", json!({"author": "binary", "page": page, "milestone": milestone, "ok": true, "url": format!("https://x/{page}")}))
        };
        publish("spec", "approval");
        publish("project", "approval");
        publish("spec", "round");
        let newest_project = publish("project", "round");
        let newest_spec = publish("spec", "close");

        let report = read_at(&opts(root, "state", None)).unwrap();
        let published: Vec<Value> = events(&report).iter().filter(|e| e["type"] == json!("publish")).map(|e| e["code"].clone()).collect();
        assert_eq!(published.len(), 2, "one publication per page: {report}");
        assert!(published.contains(&code_of(root, newest_project)) && published.contains(&code_of(root, newest_spec)), "{report}");
    }

    /// A onda que nunca saiu não tem pedido: a leitura dele é recusada, nos
    /// dois idiomas, em vez de responder um texto vazio como se desse certo.
    /// O mesmo vale para o pedido do revisor final sem envio de revisão.
    #[test]
    fn a_request_never_sent_is_refused_instead_of_read_empty() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "send", json!({"author": "binary", "wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0"}));
        assert_eq!(read_at(&opts(root, "request-1", None)), Ok("o pedido".to_string()));

        for (block, lang) in [("request-2", "pt-BR"), ("request-review", "en-US")] {
            std::fs::write(root.join("mustard.json"), format!(r#"{{"language": {{"text": "{lang}"}}}}"#)).unwrap();
            let refused = read_at(&opts(root, block, None)).expect_err(block);
            assert_eq!(refused["ok"], json!(false), "{refused}");
            assert_eq!(refused["reason"], json!("no-request"), "{refused}");
            let hint = refused["hint"].as_str().unwrap_or_default();
            let expected = if lang == "pt-BR" { "não tem pedido" } else { "has no recorded request" };
            assert!(hint.contains(expected), "{block} in {lang}: {refused}");
        }
    }

    /// Um bloco sai do mais novo para o mais antigo: quem conduz lê primeiro
    /// o que acabou de acontecer.
    #[test]
    fn a_block_lists_its_newest_events_first() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = put(root, "message", json!({"author": "user", "text": "primeiro"}));
        let second = put(root, "message", json!({"author": "user", "text": "segundo"}));
        let third = put(root, "message", json!({"author": "user", "text": "terceiro"}));
        let ids: Vec<Value> = events(&read_at(&opts(root, "conversation", None)).unwrap()).iter().map(|e| e["code"].clone()).collect();
        assert_eq!(ids, [code_of(root, third), code_of(root, second), code_of(root, first)]);
    }

    /// Na leitura de quem conduz, o código de um item no painel ainda traz o
    /// item inteiro: só a busca por palavras vira resumo.
    #[test]
    fn a_metrics_item_code_still_brings_the_whole_item_under_the_cap() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "injection", json!({"author": "hook", "hook": "prompt_entry", "chars": 5, "text": "Texto"}));
        let capped = |term: &str| read_reach(&opts(root, "metrics", Some(term)), None, root, Reach::Capped).unwrap();
        let by_code = events(&capped("MSTD-INJ-0001"));
        assert_eq!((by_code.len(), &by_code[0]["text"]), (1, &json!("Texto")), "{by_code:?}");
        let summary = events(&capped("texto"));
        assert_eq!((&summary[0]["type"], &summary[0]["newest"]), (&json!("injection"), &json!(["MSTD-INJ-0001"])), "{summary:?}");
    }
}
